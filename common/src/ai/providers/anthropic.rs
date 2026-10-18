use anyhow::{Result, anyhow};
use async_stream::try_stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::stream::{
    StreamEvent, StreamEventStream, assistant_message, parse_tool_arguments, sse_data_lines,
};
use crate::ai::types::{
    ChatCompletionChoice, ChatCompletionRequest, ChatCompletionResponse, Content, ContentPart,
    Role, Tool, Usage,
};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic API client
pub struct AnthropicClient {
    api_key: String,
    http_client: reqwest::Client,
}

impl AnthropicClient {
    /// Create a new Anthropic client
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http_client: reqwest::Client::new(),
        }
    }

    async fn send(&self, body: &AnthropicRequest) -> Result<reqwest::Response> {
        //
        // Wire dump at debug — full body, no event-log fanout (payloads
        // can be multi-KB and would flood the application event log).
        //
        if let Ok(json) = serde_json::to_string(body) {
            tracing::debug!(
                target: "common::ai::provider",
                "Anthropic request to {}: {}",
                ANTHROPIC_API_URL,
                json
            );
        }

        let response = self
            .http_client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| anyhow!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::debug!(
                target: "common::ai::provider",
                "Anthropic response error {}: {}",
                status,
                body
            );
            return Err(anyhow!("Anthropic API error {}: {}", status, body));
        }
        Ok(response)
    }

    /// Send a chat completion request
    pub async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        let body = build_request(&request, false);
        let response = self.send(&body).await?;

        let response_body = response
            .text()
            .await
            .map_err(|e| anyhow!("Failed to read response body: {}", e))?;
        tracing::debug!(
            target: "common::ai::provider",
            "Anthropic response from {}: {}",
            ANTHROPIC_API_URL,
            response_body
        );

        let api_response: AnthropicResponse = serde_json::from_str(&response_body)
            .map_err(|e| anyhow!("Failed to parse response: {}", e))?;
        Ok(convert_response(api_response))
    }

    /// Stream a chat completion as structured events (text, thinking,
    /// tool_use) over Anthropic's SSE protocol.
    pub fn stream_events(&self, request: ChatCompletionRequest) -> StreamEventStream<'_> {
        let body = build_request(&request, true);
        Box::pin(try_stream! {
            let response = self.send(&body).await?;
            let mut acc = StreamAccumulator::default();
            let mut lines = sse_data_lines(response);
            while let Some(data) = lines.next().await {
                let data = data?;
                tracing::debug!(
                    target: "common::ai::provider",
                    "Anthropic stream SSE data: {}",
                    data
                );
                let event: AnthropicStreamEvent = match serde_json::from_str(&data) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let done = matches!(event, AnthropicStreamEvent::MessageStop);
                for out in acc.handle(event)? {
                    yield out;
                }
                if done {
                    break;
                }
            }
            yield StreamEvent::Completed(acc.finish());
        })
    }
}

//
// Request construction.
//

fn build_request(request: &ChatCompletionRequest, stream: bool) -> AnthropicRequest {
    let mut system: Option<String> = None;
    let mut messages: Vec<AnthropicMessage> = Vec::new();

    for msg in &request.messages {
        match msg.role {
            //
            // Anthropic: system goes in a separate field.
            //
            Role::System => system = Some(msg.joined_text()),
            Role::User | Role::Assistant => messages.push(AnthropicMessage {
                role: msg.role.as_str().to_string(),
                content: match &msg.content {
                    Content::Text(t) => AnthropicContent::Text(t.clone()),
                    Content::Parts(parts) => {
                        AnthropicContent::Blocks(parts.iter().filter_map(to_block).collect())
                    }
                },
            }),
        }
    }

    let max_tokens = request.max_tokens.unwrap_or(4096);
    let (thinking, output_config) = match &request.reasoning {
        Some(reasoning) => thinking_config(&request.model, max_tokens, reasoning.effort.as_deref()),
        None => (None, None),
    };

    AnthropicRequest {
        model: request.model.clone(),
        max_tokens,
        system,
        messages,
        //
        // Sampling parameters are rejected alongside thinking on current
        // models, so drop them whenever thinking is requested.
        //
        temperature: if thinking.is_some() {
            None
        } else {
            request.temperature
        },
        top_p: if thinking.is_some() {
            None
        } else {
            request.top_p
        },
        tools: request.tools.iter().map(to_tool).collect(),
        thinking,
        output_config,
        stream,
    }
}

fn to_block(part: &ContentPart) -> Option<AnthropicBlock> {
    Some(match part {
        ContentPart::Text { text } if text.is_empty() => return None,
        ContentPart::Text { text } => AnthropicBlock::Text { text: text.clone() },
        ContentPart::Thinking {
            thinking,
            signature,
        } => AnthropicBlock::Thinking {
            thinking: thinking.clone(),
            signature: signature.clone().unwrap_or_default(),
        },
        ContentPart::RedactedThinking { data } => {
            AnthropicBlock::RedactedThinking { data: data.clone() }
        }
        ContentPart::ToolUse {
            id, name, input, ..
        } => AnthropicBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        ContentPart::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        } => AnthropicBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
            is_error: *is_error,
        },
    })
}

fn to_tool(tool: &Tool) -> AnthropicTool {
    AnthropicTool {
        name: tool.name.clone(),
        description: tool.description.clone(),
        input_schema: tool
            .parameters
            .clone()
            .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}})),
    }
}

//
// Pick the thinking configuration for a model. Claude 4.6 and later use
// adaptive thinking (a fixed budget is rejected on current models) with a
// summarized display so reasoning is visible. Earlier thinking-capable
// models (3.7, 4.0-4.5) take a token budget, which must be at least 1024 and
// below max_tokens. Models without thinking support get none.
//
fn thinking_config(
    model: &str,
    max_tokens: u32,
    effort: Option<&str>,
) -> (Option<AnthropicThinking>, Option<AnthropicOutputConfig>) {
    let m = model.to_ascii_lowercase();

    if m.starts_with("claude-3") && !m.starts_with("claude-3-7") || m.starts_with("claude-2") {
        return (None, None);
    }

    let budget_model = m.starts_with("claude-3-7")
        || ["-4-0", "-4-1", "-4-5", "opus-4-2025", "sonnet-4-2025"]
            .iter()
            .any(|p| m.contains(p));

    if budget_model {
        if max_tokens <= 2048 {
            return (None, None);
        }
        let budget = (max_tokens / 2).clamp(1024, 16_000);
        return (
            Some(AnthropicThinking::Enabled {
                budget_tokens: budget,
            }),
            None,
        );
    }

    let output_config = effort
        .filter(|e| !e.trim().is_empty())
        .map(|e| AnthropicOutputConfig {
            effort: e.trim().to_ascii_lowercase(),
        });
    (
        Some(AnthropicThinking::Adaptive {
            display: "summarized".to_string(),
        }),
        output_config,
    )
}

//
// Response conversion.
//

fn block_to_part(block: AnthropicBlock) -> Option<ContentPart> {
    Some(match block {
        AnthropicBlock::Text { text } => ContentPart::Text { text },
        AnthropicBlock::Thinking {
            thinking,
            signature,
        } => ContentPart::Thinking {
            thinking,
            signature: (!signature.is_empty()).then_some(signature),
        },
        AnthropicBlock::RedactedThinking { data } => ContentPart::RedactedThinking { data },
        AnthropicBlock::ToolUse { id, name, input } => ContentPart::ToolUse {
            id,
            name,
            input,
            signature: None,
        },
        AnthropicBlock::ToolResult { .. } | AnthropicBlock::Unknown => return None,
    })
}

fn convert_response(api: AnthropicResponse) -> ChatCompletionResponse {
    let parts: Vec<ContentPart> = api.content.into_iter().filter_map(block_to_part).collect();
    ChatCompletionResponse {
        id: api.id,
        model: api.model,
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: assistant_message(parts),
            finish_reason: api.stop_reason,
        }],
        usage: Some(Usage {
            prompt_tokens: api.usage.input_tokens,
            completion_tokens: api.usage.output_tokens,
            total_tokens: api.usage.input_tokens + api.usage.output_tokens,
        }),
    }
}

//
// Accumulates Anthropic SSE events into deltas for the caller and the final
// assembled message. Blocks are keyed by their stream index.
//

#[derive(Default)]
struct StreamAccumulator {
    id: String,
    model: String,
    blocks: Vec<(usize, PartialBlock)>,
    stop_reason: Option<String>,
    input_tokens: u32,
    output_tokens: u32,
}

enum PartialBlock {
    Text(String),
    Thinking {
        text: String,
        signature: String,
    },
    Redacted(String),
    ToolUse {
        id: String,
        name: String,
        json: String,
    },
    Ignored,
}

impl StreamAccumulator {
    fn block_mut(&mut self, index: usize) -> Option<&mut PartialBlock> {
        self.blocks
            .iter_mut()
            .find(|(i, _)| *i == index)
            .map(|(_, b)| b)
    }

    fn handle(&mut self, event: AnthropicStreamEvent) -> Result<Vec<StreamEvent>> {
        let mut out = Vec::new();
        match event {
            AnthropicStreamEvent::MessageStart { message } => {
                self.id = message.id;
                self.model = message.model;
                if let Some(u) = message.usage {
                    self.input_tokens = u.input_tokens;
                    self.output_tokens = u.output_tokens;
                }
            }
            AnthropicStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let block = match content_block {
                    AnthropicBlock::Text { text } => {
                        if !text.is_empty() {
                            out.push(StreamEvent::TextDelta(text.clone()));
                        }
                        PartialBlock::Text(text)
                    }
                    AnthropicBlock::Thinking {
                        thinking,
                        signature,
                    } => {
                        if !thinking.is_empty() {
                            out.push(StreamEvent::ReasoningDelta(thinking.clone()));
                        }
                        PartialBlock::Thinking {
                            text: thinking,
                            signature,
                        }
                    }
                    AnthropicBlock::RedactedThinking { data } => PartialBlock::Redacted(data),
                    AnthropicBlock::ToolUse { id, name, .. } => PartialBlock::ToolUse {
                        id,
                        name,
                        json: String::new(),
                    },
                    _ => PartialBlock::Ignored,
                };
                self.blocks.push((index, block));
            }
            AnthropicStreamEvent::ContentBlockDelta { index, delta } => {
                match (self.block_mut(index), delta) {
                    (Some(PartialBlock::Text(t)), AnthropicDelta::TextDelta { text }) => {
                        t.push_str(&text);
                        out.push(StreamEvent::TextDelta(text));
                    }
                    (
                        Some(PartialBlock::Thinking { text, .. }),
                        AnthropicDelta::ThinkingDelta { thinking },
                    ) => {
                        text.push_str(&thinking);
                        out.push(StreamEvent::ReasoningDelta(thinking));
                    }
                    (
                        Some(PartialBlock::Thinking { signature, .. }),
                        AnthropicDelta::SignatureDelta { signature: s },
                    ) => {
                        signature.push_str(&s);
                    }
                    (
                        Some(PartialBlock::ToolUse { json, .. }),
                        AnthropicDelta::InputJsonDelta { partial_json },
                    ) => {
                        json.push_str(&partial_json);
                    }
                    _ => {}
                }
            }
            AnthropicStreamEvent::MessageDelta { delta, usage } => {
                if delta.stop_reason.is_some() {
                    self.stop_reason = delta.stop_reason;
                }
                if let Some(u) = usage {
                    self.output_tokens = u.output_tokens;
                }
            }
            AnthropicStreamEvent::Error { error } => {
                return Err(anyhow!("Anthropic stream error: {}", error.message));
            }
            AnthropicStreamEvent::ContentBlockStop { .. }
            | AnthropicStreamEvent::MessageStop
            | AnthropicStreamEvent::Ping
            | AnthropicStreamEvent::Unknown => {}
        }
        Ok(out)
    }

    fn finish(mut self) -> ChatCompletionResponse {
        self.blocks.sort_by_key(|(i, _)| *i);
        let parts: Vec<ContentPart> = self
            .blocks
            .into_iter()
            .filter_map(|(_, b)| match b {
                PartialBlock::Text(text) => Some(ContentPart::Text { text }),
                PartialBlock::Thinking { text, signature } => Some(ContentPart::Thinking {
                    thinking: text,
                    signature: (!signature.is_empty()).then_some(signature),
                }),
                PartialBlock::Redacted(data) => Some(ContentPart::RedactedThinking { data }),
                PartialBlock::ToolUse { id, name, json } => Some(ContentPart::ToolUse {
                    id,
                    name,
                    input: parse_tool_arguments(&json),
                    signature: None,
                }),
                PartialBlock::Ignored => None,
            })
            .collect();

        ChatCompletionResponse {
            id: self.id,
            model: self.model,
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: assistant_message(parts),
                finish_reason: self.stop_reason,
            }],
            usage: Some(Usage {
                prompt_tokens: self.input_tokens,
                completion_tokens: self.output_tokens,
                total_tokens: self.input_tokens + self.output_tokens,
            }),
        }
    }
}

//
// Anthropic API request/response types.
//

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicBlock>),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicBlock {
    Text {
        #[serde(default)]
        text: String,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
        #[serde(default)]
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
    //
    // Block types we neither produce nor need (server tools, fallback
    // markers, ...). Dropped from the assembled message.
    //
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_schema: Value,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicThinking {
    Adaptive { display: String },
    Enabled { budget_tokens: u32 },
}

#[derive(Debug, Serialize, PartialEq)]
struct AnthropicOutputConfig {
    effort: String,
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<AnthropicTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<AnthropicThinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<AnthropicOutputConfig>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Debug, Deserialize, Default)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<AnthropicBlock>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamMessage {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageDelta {
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamError {
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicDelta {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    SignatureDelta {
        signature: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicStreamEvent {
    MessageStart {
        message: AnthropicStreamMessage,
    },
    ContentBlockStart {
        index: usize,
        content_block: AnthropicBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: AnthropicDelta,
    },
    ContentBlockStop {
        #[allow(dead_code)]
        index: usize,
    },
    MessageDelta {
        delta: AnthropicMessageDelta,
        usage: Option<AnthropicUsage>,
    },
    MessageStop,
    Ping,
    Error {
        error: AnthropicStreamError,
    },
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::types::{Message, ReasoningConfig};
    use serde_json::json;

    #[test]
    fn test_client_creation() {
        let client = AnthropicClient::new("test-key".to_string());
        assert_eq!(client.api_key, "test-key");
    }

    fn tool_turn_request() -> ChatCompletionRequest {
        ChatCompletionRequest::new(
            "claude-opus-5-5",
            vec![
                Message::system("sys"),
                Message::user("list nodes"),
                Message::with_parts(
                    Role::Assistant,
                    vec![
                        ContentPart::Thinking {
                            thinking: "need nodes".into(),
                            signature: Some("SIG".into()),
                        },
                        ContentPart::Text {
                            text: "Checking.".into(),
                        },
                        ContentPart::ToolUse {
                            id: "toolu_1".into(),
                            name: "node_list".into(),
                            input: json!({}),
                            signature: None,
                        },
                    ],
                ),
                Message::with_parts(
                    Role::User,
                    vec![ContentPart::ToolResult {
                        tool_use_id: "toolu_1".into(),
                        name: "node_list".into(),
                        content: "[]".into(),
                        is_error: false,
                    }],
                ),
            ],
        )
        .with_max_tokens(8000)
        .with_tools(vec![
            Tool::new("node_list")
                .with_description("List nodes")
                .with_parameters(json!({"type": "object", "properties": {}})),
        ])
        .with_reasoning(ReasoningConfig {
            effort: Some("High".into()),
        })
    }

    #[test]
    fn request_maps_tools_thinking_and_tool_turns() {
        let body = serde_json::to_value(build_request(&tool_turn_request(), true)).unwrap();
        let expected = json!({
            "model": "claude-opus-5-5",
            "max_tokens": 8000,
            "system": "sys",
            "messages": [
                {"role": "user", "content": "list nodes"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "need nodes", "signature": "SIG"},
                    {"type": "text", "text": "Checking."},
                    {"type": "tool_use", "id": "toolu_1", "name": "node_list", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": "[]"}
                ]}
            ],
            "tools": [{
                "name": "node_list",
                "description": "List nodes",
                "input_schema": {"type": "object", "properties": {}}
            }],
            "thinking": {"type": "adaptive", "display": "summarized"},
            "output_config": {"effort": "high"},
            "stream": true
        });
        assert_eq!(body, expected);
    }

    #[test]
    fn legacy_models_use_budget_thinking() {
        let (thinking, cfg) = thinking_config("claude-sonnet-4-5", 8000, Some("high"));
        assert_eq!(
            thinking,
            Some(AnthropicThinking::Enabled {
                budget_tokens: 4000
            })
        );
        assert!(cfg.is_none());
        assert_eq!(
            thinking_config("claude-3-5-haiku-latest", 8000, None).0,
            None
        );
        assert!(matches!(
            thinking_config("claude-3-7-sonnet-latest", 8000, None).0,
            Some(AnthropicThinking::Enabled { .. })
        ));
        assert!(matches!(
            thinking_config("claude-sonnet-5-5", 8000, None).0,
            Some(AnthropicThinking::Adaptive { .. })
        ));
    }

    #[test]
    fn response_with_thinking_and_tool_use_round_trips() {
        let fixture = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5-5",
            "content": [
                {"type": "thinking", "thinking": "plan", "signature": "abc"},
                {"type": "text", "text": "Listing nodes."},
                {"type": "tool_use", "id": "toolu_9", "name": "node_list", "input": {"x": 1}},
                {"type": "fallback", "from": {}, "to": {}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let api: AnthropicResponse = serde_json::from_value(fixture).unwrap();
        let resp = convert_response(api);
        let msg = &resp.choices[0].message;
        let parts = msg.parts();
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts[2],
            ContentPart::ToolUse {
                id: "toolu_9".into(),
                name: "node_list".into(),
                input: json!({"x": 1}),
                signature: None
            }
        );
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_use"));

        //
        // Replaying the assembled message must reproduce the wire blocks.
        //
        let replay: Vec<AnthropicBlock> = parts.iter().filter_map(to_block).collect();
        assert_eq!(
            serde_json::to_value(&replay).unwrap(),
            json!([
                {"type": "thinking", "thinking": "plan", "signature": "abc"},
                {"type": "text", "text": "Listing nodes."},
                {"type": "tool_use", "id": "toolu_9", "name": "node_list", "input": {"x": 1}}
            ])
        );
    }

    #[test]
    fn stream_accumulator_assembles_blocks() {
        let events = [
            r#"{"type":"message_start","message":{"id":"msg_s","model":"claude-opus-5-5","usage":{"input_tokens":42,"output_tokens":1}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me "}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"check."}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"SIGX"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"On it."}}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_a","name":"agent_list","input":{}}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"node_id\":"}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"abc\"}"}}"#,
            r#"{"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"toolu_b","name":"node_list","input":{}}}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":17}}"#,
            r#"{"type":"ping"}"#,
            r#"{"type":"message_stop"}"#,
        ];

        let mut acc = StreamAccumulator::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        for raw in events {
            let event: AnthropicStreamEvent = serde_json::from_str(raw).unwrap();
            for out in acc.handle(event).unwrap() {
                match out {
                    StreamEvent::TextDelta(t) => text.push_str(&t),
                    StreamEvent::ReasoningDelta(r) => reasoning.push_str(&r),
                    StreamEvent::Completed(_) => unreachable!(),
                }
            }
        }
        assert_eq!(text, "On it.");
        assert_eq!(reasoning, "Let me check.");

        let resp = acc.finish();
        assert_eq!(resp.id, "msg_s");
        let usage = resp.usage.unwrap();
        assert_eq!((usage.prompt_tokens, usage.completion_tokens), (42, 17));
        let parts = resp.choices[0].message.parts();
        assert_eq!(
            parts,
            vec![
                ContentPart::Thinking {
                    thinking: "Let me check.".into(),
                    signature: Some("SIGX".into())
                },
                ContentPart::Text {
                    text: "On it.".into()
                },
                ContentPart::ToolUse {
                    id: "toolu_a".into(),
                    name: "agent_list".into(),
                    input: json!({"node_id": "abc"}),
                    signature: None
                },
                ContentPart::ToolUse {
                    id: "toolu_b".into(),
                    name: "node_list".into(),
                    input: json!({}),
                    signature: None
                },
            ]
        );
    }

    #[test]
    fn stream_error_event_is_surfaced() {
        let mut acc = StreamAccumulator::default();
        let event: AnthropicStreamEvent = serde_json::from_str(
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        )
        .unwrap();
        assert!(acc.handle(event).is_err());
    }
}
