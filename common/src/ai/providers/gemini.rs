use anyhow::{Result, anyhow};
use async_stream::try_stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::stream::{StreamEvent, StreamEventStream, assistant_message, sse_data_lines};
use crate::ai::types::{
    ChatCompletionChoice, ChatCompletionRequest, ChatCompletionResponse, Content, ContentPart,
    Role, Tool, Usage,
};

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

//
// Older Gemini models do not return function-call ids. We synthesize ids
// with this prefix so results can be paired locally, and strip them again
// when replaying so the API never sees ids it did not issue.
//
const SYNTHETIC_ID_PREFIX: &str = "gcall_";

/// Google Gemini API client
pub struct GeminiClient {
    api_key: String,
    http_client: reqwest::Client,
}

impl GeminiClient {
    /// Create a new Gemini client
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http_client: reqwest::Client::new(),
        }
    }

    async fn send(
        &self,
        model: &str,
        method: &str,
        body: &GeminiRequest,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/{}:{}", GEMINI_API_BASE, model, method);
        let sep = if url.contains('?') { '&' } else { '?' };

        //
        // Wire dump at debug — log URL without the API key query param.
        // Full body uses tracing::debug (not event-log macros) so multi-KB
        // payloads do not flood the application event log.
        //
        if let Ok(json) = serde_json::to_string(body) {
            tracing::debug!(
                target: "common::ai::provider",
                "Gemini request to {}: {}",
                url,
                json
            );
        }

        let response = self
            .http_client
            .post(format!("{}{}key={}", url, sep, self.api_key))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| anyhow!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::debug!(
                target: "common::ai::provider",
                "Gemini response error {}: {}",
                status,
                body
            );
            return Err(anyhow!("Gemini API error {}: {}", status, body));
        }
        Ok(response)
    }

    /// Send a chat completion request
    pub async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        let body = build_request(&request);
        let response = self.send(&request.model, "generateContent", &body).await?;

        let response_body = response
            .text()
            .await
            .map_err(|e| anyhow!("Failed to read response body: {}", e))?;
        tracing::debug!(
            target: "common::ai::provider",
            "Gemini response from {}/{}:generateContent: {}",
            GEMINI_API_BASE,
            request.model,
            response_body
        );

        let api_response: GeminiResponse = serde_json::from_str(&response_body)
            .map_err(|e| anyhow!("Failed to parse response: {}", e))?;

        let mut acc = StreamAccumulator::default();
        acc.handle(api_response);
        Ok(acc.finish(request.model))
    }

    /// Stream a chat completion as structured events over
    /// `streamGenerateContent` (SSE).
    pub fn stream_events(&self, request: ChatCompletionRequest) -> StreamEventStream<'_> {
        let body = build_request(&request);
        Box::pin(try_stream! {
            let response = self
                .send(&request.model, "streamGenerateContent?alt=sse", &body)
                .await?;
            let mut acc = StreamAccumulator::default();
            let mut lines = sse_data_lines(response);
            while let Some(data) = lines.next().await {
                let data = data?;
                tracing::debug!(
                    target: "common::ai::provider",
                    "Gemini stream SSE data: {}",
                    data
                );
                let chunk: GeminiResponse = match serde_json::from_str(&data) {
                    Ok(c) => c,
                    Err(e) => {
                        if let Ok(err) = serde_json::from_str::<GeminiErrorEnvelope>(&data) {
                            Err(anyhow!("Gemini stream error: {}", err.error.message))?;
                        }
                        crate::log_warn!(
                            "Gemini stream: unparseable SSE line ({}): {}",
                            e,
                            crate::truncate_str(&data, 200)
                        );
                        continue;
                    }
                };
                for out in acc.handle(chunk) {
                    yield out;
                }
            }
            yield StreamEvent::Completed(acc.finish(request.model.clone()));
        })
    }
}

//
// Request construction.
//

fn build_request(request: &ChatCompletionRequest) -> GeminiRequest {
    let mut system_instruction: Option<String> = None;
    let mut contents: Vec<GeminiContent> = Vec::new();

    for msg in &request.messages {
        let role = match msg.role {
            //
            // Gemini uses system_instruction for system messages.
            //
            Role::System => {
                system_instruction = Some(msg.joined_text());
                continue;
            }
            Role::User => "user",
            Role::Assistant => "model",
        };
        let parts: Vec<GeminiPart> = match &msg.content {
            Content::Text(t) => vec![GeminiPart::text(t.clone())],
            Content::Parts(parts) => parts.iter().filter_map(to_part).collect(),
        };
        if parts.is_empty() {
            continue;
        }
        contents.push(GeminiContent {
            role: role.to_string(),
            parts,
        });
    }

    GeminiRequest {
        contents,
        system_instruction: system_instruction.map(|text| GeminiSystemInstruction {
            parts: vec![GeminiPart::text(text)],
        }),
        generation_config: Some(GeminiGenerationConfig {
            max_output_tokens: request.max_tokens,
            temperature: request.temperature,
            top_p: request.top_p,
            thinking_config: request
                .reasoning
                .as_ref()
                .filter(|_| supports_thinking(&request.model))
                .map(|_| GeminiThinkingConfig {
                    include_thoughts: true,
                }),
        }),
        tools: if request.tools.is_empty() {
            Vec::new()
        } else {
            vec![GeminiTool {
                function_declarations: request.tools.iter().map(to_declaration).collect(),
            }]
        },
    }
}

//
// Thinking (and therefore thought summaries) arrived with Gemini 2.5;
// earlier models reject a thinkingConfig.
//
fn supports_thinking(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    !(m.contains("gemini-1") || m.contains("gemini-2.0") || m.contains("gemma"))
}

fn to_part(part: &ContentPart) -> Option<GeminiPart> {
    match part {
        ContentPart::Text { text } if text.is_empty() => None,
        ContentPart::Text { text } => Some(GeminiPart::text(text.clone())),
        //
        // Thought summaries carry no state of their own; only replay those
        // that hold a signature the API needs back.
        //
        ContentPart::Thinking {
            thinking,
            signature: Some(sig),
        } => Some(GeminiPart {
            text: Some(thinking.clone()),
            thought: true,
            thought_signature: Some(sig.clone()),
            ..Default::default()
        }),
        ContentPart::Thinking { .. } | ContentPart::RedactedThinking { .. } => None,
        ContentPart::ToolUse {
            id,
            name,
            input,
            signature,
        } => Some(GeminiPart {
            function_call: Some(GeminiFunctionCall {
                id: real_id(id),
                name: name.clone(),
                args: input.clone(),
            }),
            thought_signature: signature.clone(),
            ..Default::default()
        }),
        ContentPart::ToolResult {
            tool_use_id,
            name,
            content,
            is_error,
        } => {
            //
            // functionResponse.response must be a JSON object.
            //
            let parsed = serde_json::from_str::<Value>(content)
                .unwrap_or_else(|_| Value::String(content.clone()));
            let response = match parsed {
                Value::Object(_) if !is_error => parsed,
                other if *is_error => serde_json::json!({ "error": other }),
                other => serde_json::json!({ "result": other }),
            };
            Some(GeminiPart {
                function_response: Some(GeminiFunctionResponse {
                    id: real_id(tool_use_id),
                    name: name.clone(),
                    response,
                }),
                ..Default::default()
            })
        }
    }
}

fn real_id(id: &str) -> Option<String> {
    (!id.starts_with(SYNTHETIC_ID_PREFIX) && !id.is_empty()).then(|| id.to_string())
}

fn to_declaration(tool: &Tool) -> GeminiFunctionDeclaration {
    GeminiFunctionDeclaration {
        name: tool.name.clone(),
        description: tool.description.clone().unwrap_or_default(),
        //
        // MCP schemas are full JSON Schema, which `parameters` (an OpenAPI
        // subset) can reject; `parametersJsonSchema` accepts it as-is.
        //
        parameters_json_schema: tool
            .parameters
            .clone()
            .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}})),
    }
}

//
// Gemini streams whole parts per chunk; text and thought text arrive split
// across chunks and are merged by assistant_message at the end.
//

#[derive(Default)]
struct StreamAccumulator {
    parts: Vec<ContentPart>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
    calls: usize,
}

impl StreamAccumulator {
    fn handle(&mut self, chunk: GeminiResponse) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if let Some(u) = chunk.usage_metadata {
            self.usage = Some(Usage {
                prompt_tokens: u.prompt_token_count,
                completion_tokens: u.candidates_token_count,
                total_tokens: u.total_token_count,
            });
        }
        let Some(candidate) = chunk.candidates.into_iter().next() else {
            return out;
        };
        if candidate.finish_reason.is_some() {
            self.finish_reason = candidate.finish_reason;
        }
        let parts = candidate.content.map(|c| c.parts).unwrap_or_default();
        for part in parts {
            if let Some(call) = part.function_call {
                self.calls += 1;
                let id = call
                    .id
                    .filter(|i| !i.is_empty())
                    .unwrap_or_else(|| format!("{}{}", SYNTHETIC_ID_PREFIX, self.calls));
                self.parts.push(ContentPart::ToolUse {
                    id,
                    name: call.name,
                    input: if call.args.is_object() {
                        call.args
                    } else {
                        Value::Object(Default::default())
                    },
                    signature: part.thought_signature,
                });
                continue;
            }
            let text = part.text.unwrap_or_default();
            if part.thought || (text.is_empty() && part.thought_signature.is_some()) {
                if !text.is_empty() {
                    out.push(StreamEvent::ReasoningDelta(text.clone()));
                }
                self.push_thought(text, part.thought_signature);
            } else if !text.is_empty() {
                out.push(StreamEvent::TextDelta(text.clone()));
                self.parts.push(ContentPart::Text { text });
            }
        }
        out
    }

    //
    // Thought text streams in pieces; keep one Thinking part per run and
    // attach a signature when it arrives.
    //
    fn push_thought(&mut self, text: String, signature: Option<String>) {
        if let Some(ContentPart::Thinking {
            thinking,
            signature: sig,
        }) = self.parts.last_mut()
            && sig.is_none()
        {
            thinking.push_str(&text);
            *sig = signature;
            return;
        }
        self.parts.push(ContentPart::Thinking {
            thinking: text,
            signature,
        });
    }

    fn finish(self, model: String) -> ChatCompletionResponse {
        ChatCompletionResponse {
            //
            // Gemini doesn't return an ID.
            //
            id: "gemini-response".to_string(),
            model,
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: assistant_message(self.parts),
                finish_reason: self.finish_reason,
            }],
            usage: self.usage,
        }
    }
}

//
// Gemini API request/response types.
//

#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GeminiPart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    thought: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thought_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    function_call: Option<GeminiFunctionCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    function_response: Option<GeminiFunctionResponse>,
}

impl GeminiPart {
    fn text(text: String) -> Self {
        Self {
            text: Some(text),
            ..Default::default()
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct GeminiFunctionCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    name: String,
    #[serde(default)]
    args: Value,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct GeminiFunctionResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    name: String,
    response: Value,
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiThinkingConfig {
    include_thoughts: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_config: Option<GeminiThinkingConfig>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters_json_schema: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiTool {
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<GeminiTool>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiResponseContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: u32,
    #[serde(rename = "totalTokenCount", default)]
    total_token_count: u32,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorEnvelope {
    error: GeminiErrorBody,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorBody {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::types::{Message, ReasoningConfig};
    use serde_json::json;

    #[test]
    fn test_client_creation() {
        let client = GeminiClient::new("test-key".to_string());
        assert_eq!(client.api_key, "test-key");
    }

    #[test]
    fn request_maps_tools_and_function_turns() {
        let request = ChatCompletionRequest::new(
            "gemini-3-pro-preview",
            vec![
                Message::system("sys"),
                Message::user("list nodes"),
                Message::with_parts(
                    Role::Assistant,
                    vec![
                        ContentPart::Thinking {
                            thinking: "summary".into(),
                            signature: None,
                        },
                        ContentPart::ToolUse {
                            id: "gcall_1".into(),
                            name: "node_list".into(),
                            input: json!({}),
                            signature: Some("TS".into()),
                        },
                    ],
                ),
                Message::with_parts(
                    Role::User,
                    vec![
                        ContentPart::ToolResult {
                            tool_use_id: "gcall_1".into(),
                            name: "node_list".into(),
                            content: "[1,2]".into(),
                            is_error: false,
                        },
                        ContentPart::ToolResult {
                            tool_use_id: "fc-2".into(),
                            name: "op_info".into(),
                            content: "boom".into(),
                            is_error: true,
                        },
                    ],
                ),
            ],
        )
        .with_max_tokens(256)
        .with_tools(vec![Tool::new("node_list").with_description("List nodes")])
        .with_reasoning(ReasoningConfig::default());

        let body = serde_json::to_value(build_request(&request)).unwrap();
        assert_eq!(
            body,
            json!({
                "contents": [
                    {"role": "user", "parts": [{"text": "list nodes"}]},
                    {"role": "model", "parts": [{
                        "thoughtSignature": "TS",
                        "functionCall": {"name": "node_list", "args": {}}
                    }]},
                    {"role": "user", "parts": [
                        {"functionResponse": {"name": "node_list", "response": {"result": [1, 2]}}},
                        {"functionResponse": {"id": "fc-2", "name": "op_info", "response": {"error": "boom"}}}
                    ]}
                ],
                "systemInstruction": {"parts": [{"text": "sys"}]},
                "generationConfig": {
                    "maxOutputTokens": 256,
                    "thinkingConfig": {"includeThoughts": true}
                },
                "tools": [{"functionDeclarations": [{
                    "name": "node_list",
                    "description": "List nodes",
                    "parametersJsonSchema": {"type": "object", "properties": {}}
                }]}]
            })
        );
    }

    #[test]
    fn older_models_get_no_thinking_config() {
        let request = ChatCompletionRequest::new("gemini-2.0-flash", vec![Message::user("hi")])
            .with_reasoning(ReasoningConfig::default());
        let body = serde_json::to_value(build_request(&request)).unwrap();
        assert!(body["generationConfig"].get("thinkingConfig").is_none());
    }

    #[test]
    fn stream_chunks_assemble_thoughts_text_and_calls() {
        let chunks = [
            json!({"candidates": [{"content": {"role": "model", "parts": [
                {"text": "Thinking about", "thought": true}
            ]}}]}),
            json!({"candidates": [{"content": {"role": "model", "parts": [
                {"text": " nodes.", "thought": true}
            ]}}]}),
            json!({"candidates": [{"content": {"role": "model", "parts": [
                {"text": "Checking."}
            ]}}]}),
            json!({"candidates": [{"content": {"role": "model", "parts": [
                {"functionCall": {"name": "node_list", "args": {}}, "thoughtSignature": "SIG1"},
                {"functionCall": {"id": "abc", "name": "op_available", "args": {"x": 1}}}
            ]}, "finishReason": "STOP"}],
             "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 6, "totalTokenCount": 11}}),
        ];
        let mut acc = StreamAccumulator::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        for chunk in chunks {
            for ev in acc.handle(serde_json::from_value(chunk).unwrap()) {
                match ev {
                    StreamEvent::TextDelta(t) => text.push_str(&t),
                    StreamEvent::ReasoningDelta(r) => reasoning.push_str(&r),
                    StreamEvent::Completed(_) => unreachable!(),
                }
            }
        }
        assert_eq!(text, "Checking.");
        assert_eq!(reasoning, "Thinking about nodes.");

        let resp = acc.finish("gemini-3-pro-preview".into());
        assert_eq!(resp.usage.unwrap().total_tokens, 11);
        let parts = resp.choices[0].message.parts();
        assert_eq!(
            parts,
            vec![
                ContentPart::Thinking {
                    thinking: "Thinking about nodes.".into(),
                    signature: None
                },
                ContentPart::Text {
                    text: "Checking.".into()
                },
                ContentPart::ToolUse {
                    id: "gcall_1".into(),
                    name: "node_list".into(),
                    input: json!({}),
                    signature: Some("SIG1".into())
                },
                ContentPart::ToolUse {
                    id: "abc".into(),
                    name: "op_available".into(),
                    input: json!({"x": 1}),
                    signature: None
                },
            ]
        );

        //
        // Replay keeps the signature on the first call and drops the
        // synthetic id.
        //
        let replay: Vec<GeminiPart> = parts.iter().filter_map(to_part).collect();
        assert_eq!(
            serde_json::to_value(&replay).unwrap(),
            json!([
                {"text": "Checking."},
                {"thoughtSignature": "SIG1", "functionCall": {"name": "node_list", "args": {}}},
                {"functionCall": {"id": "abc", "name": "op_available", "args": {"x": 1}}}
            ])
        );
    }
}
