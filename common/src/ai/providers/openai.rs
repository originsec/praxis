use std::pin::Pin;

use anyhow::{Result, anyhow};
use async_stream::try_stream;
use futures_core::Stream;
use futures_util::StreamExt;
use reqwest::Response;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::ai::stream::{
    StreamEvent, StreamEventStream, assistant_message, parse_tool_arguments, sse_data_lines,
};
use crate::ai::types::{
    ChatCompletionChoice, ChatCompletionDelta, ChatCompletionRequest, ChatCompletionResponse,
    Content, ContentPart, Message, Role, Tool, Usage,
};

const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";

/// OpenAI-compatible API client
///
/// This client works with OpenAI and any OpenAI-compatible API by configuring
/// a custom base URL.
pub struct OpenAIClient {
    api_key: String,
    base_url: String,
    http_client: reqwest::Client,
}

impl OpenAIClient {
    /// Create a new OpenAI client
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: OPENAI_API_URL.to_string(),
            http_client: reqwest::Client::new(),
        }
    }

    /// Create a client with a custom base URL (for OpenAI-compatible APIs)
    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        //
        // Ensure the URL ends with /chat/completions.
        //
        let url = if base_url.ends_with("/chat/completions") {
            base_url
        } else if base_url.ends_with('/') {
            format!("{}chat/completions", base_url)
        } else {
            format!("{}/chat/completions", base_url)
        };

        Self {
            api_key,
            base_url: url,
            http_client: reqwest::Client::new(),
        }
    }

    /// Send a chat completion request
    pub async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        let body = self.build_request(&request, false);

        log_provider_request("OpenAI", &self.base_url, &body);

        //
        // Send request.
        //
        let response = self
            .http_client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::debug!(
                target: "common::ai::provider",
                "OpenAI response error {} from {}: {}",
                status,
                self.base_url,
                body
            );
            return Err(anyhow!("OpenAI API error {}: {}", status, body));
        }

        let response_body = response
            .text()
            .await
            .map_err(|e| anyhow!("Failed to read response body: {}", e))?;
        log_provider_response("OpenAI", &self.base_url, &response_body);

        let api_response: OpenAIResponse = serde_json::from_str(&response_body)
            .map_err(|e| anyhow!("Failed to parse response: {}", e))?;

        Ok(convert_response(api_response))
    }

    /// Send a streaming chat completion request.
    ///
    /// Returns a stream of ChatCompletionDelta chunks. The final chunk
    /// will have finish_reason set and may include usage stats.
    pub fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<ChatCompletionDelta>> + Send + '_>> {
        let body = self.build_request(&request, true);

        let base_url = self.base_url.clone();
        log_provider_request("OpenAI", &base_url, &body);

        Box::pin(try_stream! {
            let response = self
                .http_client
                .post(&self.base_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| anyhow!("HTTP request failed: {}", e))?;

            let response = check_response_status(response).await?;

            tracing::debug!(
                target: "common::ai::provider",
                "OpenAI stream response start from {} (status {})",
                base_url,
                response.status()
            );

            for await delta in parse_sse_stream(response) {
                yield delta?;
            }

            tracing::debug!(
                target: "common::ai::provider",
                "OpenAI stream response end from {}",
                base_url
            );
        })
    }

    /// Stream a chat completion as structured events, including native
    /// tool calls and reasoning content where the endpoint returns them.
    pub fn stream_events(&self, request: ChatCompletionRequest) -> StreamEventStream<'_> {
        let body = self.build_request(&request, true);
        log_provider_request("OpenAI", &self.base_url, &body);

        Box::pin(try_stream! {
            let response = self
                .http_client
                .post(&self.base_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| anyhow!("HTTP request failed: {}", e))?;
            let response = check_response_status(response).await?;

            let mut acc = StreamAccumulator::default();
            let mut lines = sse_data_lines(response);
            while let Some(data) = lines.next().await {
                let data = data?;
                tracing::debug!(
                    target: "common::ai::provider",
                    "OpenAI stream SSE data: {}",
                    data
                );
                if data == "[DONE]" {
                    break;
                }
                let chunk: OpenAIStreamChunk = match serde_json::from_str(&data) {
                    Ok(c) => c,
                    Err(e) => {
                        crate::log_warn!(
                            "OpenAI stream: unparseable SSE line ({}): {}",
                            e,
                            crate::truncate_str(&data, 200)
                        );
                        continue;
                    }
                };
                for out in acc.handle(chunk)? {
                    yield out;
                }
            }
            yield StreamEvent::Completed(acc.finish());
        })
    }

    //
    // Build the wire request. OpenAI's own endpoint wants
    // `max_completion_tokens` (reasoning models reject `max_tokens`); other
    // compatible servers still expect `max_tokens`.
    //
    fn build_request(&self, request: &ChatCompletionRequest, stream: bool) -> OpenAIRequest {
        let official = self.base_url.contains("api.openai.com");
        OpenAIRequest {
            model: request.model.clone(),
            messages: convert_messages(&request.messages),
            max_tokens: if official { None } else { request.max_tokens },
            max_completion_tokens: if official { request.max_tokens } else { None },
            temperature: request.temperature,
            top_p: request.top_p,
            tools: request.tools.iter().map(to_tool).collect(),
            reasoning_effort: request
                .reasoning
                .as_ref()
                .and_then(|r| r.effort.clone())
                .filter(|e| !e.trim().is_empty())
                .map(|e| e.trim().to_ascii_lowercase()),
            stream,
            stream_options: (stream && official).then_some(OpenAIStreamOptions {
                include_usage: true,
            }),
        }
    }
}

//
// Translate our messages to the chat-completions shape. Tool results become
// one `tool` message each; thinking blocks are not replayed (compatible
// servers either ignore or reject reasoning in the input).
//

fn convert_messages(messages: &[Message]) -> Vec<OpenAIMessage> {
    let mut out = Vec::with_capacity(messages.len());
    for msg in messages {
        let parts = match &msg.content {
            Content::Text(t) => {
                out.push(OpenAIMessage::text(msg.role.as_str(), t.clone()));
                continue;
            }
            Content::Parts(parts) => parts,
        };

        let text: String = parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        match msg.role {
            Role::Assistant => {
                let tool_calls: Vec<OpenAIToolCall> = parts
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::ToolUse {
                            id, name, input, ..
                        } => Some(OpenAIToolCall {
                            id: id.clone(),
                            call_type: "function".to_string(),
                            function: OpenAIFunctionCall {
                                name: name.clone(),
                                arguments: input.to_string(),
                            },
                        }),
                        _ => None,
                    })
                    .collect();
                out.push(OpenAIMessage {
                    role: "assistant".to_string(),
                    content: if text.is_empty() && !tool_calls.is_empty() {
                        None
                    } else {
                        Some(text)
                    },
                    tool_calls,
                    tool_call_id: None,
                });
            }
            Role::User | Role::System => {
                for part in parts {
                    if let ContentPart::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } = part
                    {
                        out.push(OpenAIMessage {
                            role: "tool".to_string(),
                            content: Some(content.clone()),
                            tool_calls: Vec::new(),
                            tool_call_id: Some(tool_use_id.clone()),
                        });
                    }
                }
                if !text.is_empty() {
                    out.push(OpenAIMessage::text(msg.role.as_str(), text));
                }
            }
        }
    }
    out
}

fn to_tool(tool: &Tool) -> OpenAITool {
    OpenAITool {
        tool_type: "function".to_string(),
        function: OpenAIFunctionDef {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool
                .parameters
                .clone()
                .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}})),
        },
    }
}

//
// Reasoning text arrives as `reasoning_content` (DeepSeek, vLLM, Moonshot)
// or `reasoning` (OpenRouter, Ollama, Groq) depending on the server.
//
fn reasoning_text(reasoning_content: Option<String>, reasoning: Option<String>) -> Option<String> {
    reasoning_content.or(reasoning).filter(|r| !r.is_empty())
}

fn convert_response(api_response: OpenAIResponse) -> ChatCompletionResponse {
    let choices = api_response
        .choices
        .into_iter()
        .map(|choice| {
            let message = if choice.message.tool_calls.is_empty()
                && choice.message.reasoning_content.is_none()
                && choice.message.reasoning.is_none()
            {
                Message::new(
                    match choice.message.role.as_str() {
                        "assistant" => Role::Assistant,
                        "user" => Role::User,
                        "system" => Role::System,
                        _ => Role::Assistant,
                    },
                    Content::Text(choice.message.content.unwrap_or_default()),
                )
            } else {
                let mut parts = Vec::new();
                if let Some(r) =
                    reasoning_text(choice.message.reasoning_content, choice.message.reasoning)
                {
                    parts.push(ContentPart::Thinking {
                        thinking: r,
                        signature: None,
                    });
                }
                if let Some(text) = choice.message.content.filter(|t| !t.is_empty()) {
                    parts.push(ContentPart::Text { text });
                }
                for call in choice.message.tool_calls {
                    parts.push(ContentPart::ToolUse {
                        id: call.id,
                        name: call.function.name,
                        input: parse_tool_arguments(&call.function.arguments),
                        signature: None,
                    });
                }
                assistant_message(parts)
            };
            ChatCompletionChoice {
                index: choice.index,
                message,
                finish_reason: choice.finish_reason,
            }
        })
        .collect();

    ChatCompletionResponse {
        id: api_response.id,
        model: api_response.model,
        choices,
        usage: api_response.usage.map(Usage::from),
    }
}

impl From<OpenAIUsage> for Usage {
    fn from(u: OpenAIUsage) -> Self {
        Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        }
    }
}

//
// Accumulates streamed chunks: text and reasoning deltas are forwarded as
// they arrive; tool calls are assembled per `index` from fragments.
//

#[derive(Default)]
struct StreamAccumulator {
    reasoning: String,
    text: String,
    tool_calls: Vec<(usize, String, String, String)>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
}

impl StreamAccumulator {
    fn handle(&mut self, chunk: OpenAIStreamChunk) -> Result<Vec<StreamEvent>> {
        //
        // Some providers signal a mid-stream failure as an `{"error": ...}`
        // envelope with an HTTP 200 status.
        //
        if let Some(err) = chunk.error {
            return Err(anyhow!("OpenAI stream error: {}", err.message));
        }
        if let Some(u) = chunk.usage {
            self.usage = Some(u.into());
        }

        let mut out = Vec::new();
        for choice in chunk.choices {
            let delta = choice.delta;
            if let Some(r) = reasoning_text(delta.reasoning_content, delta.reasoning) {
                self.reasoning.push_str(&r);
                out.push(StreamEvent::ReasoningDelta(r));
            }
            if let Some(t) = delta.content.filter(|t| !t.is_empty()) {
                self.text.push_str(&t);
                out.push(StreamEvent::TextDelta(t));
            }
            for (pos, call) in delta.tool_calls.into_iter().enumerate() {
                let index = call.index.unwrap_or(pos);
                let slot = match self.tool_calls.iter_mut().find(|c| c.0 == index) {
                    Some(slot) => slot,
                    None => {
                        self.tool_calls
                            .push((index, String::new(), String::new(), String::new()));
                        self.tool_calls.last_mut().unwrap()
                    }
                };
                if let Some(id) = call.id.filter(|i| !i.is_empty()) {
                    slot.1 = id;
                }
                if let Some(f) = call.function {
                    if let Some(name) = f.name {
                        slot.2.push_str(&name);
                    }
                    if let Some(args) = f.arguments {
                        slot.3.push_str(&args);
                    }
                }
            }
            if choice.finish_reason.is_some() {
                self.finish_reason = choice.finish_reason;
            }
        }
        Ok(out)
    }

    fn finish(mut self) -> ChatCompletionResponse {
        let mut parts = Vec::new();
        if !self.reasoning.is_empty() {
            parts.push(ContentPart::Thinking {
                thinking: self.reasoning,
                signature: None,
            });
        }
        if !self.text.is_empty() {
            parts.push(ContentPart::Text { text: self.text });
        }
        self.tool_calls.sort_by_key(|c| c.0);
        for (index, id, name, args) in self.tool_calls {
            //
            // A few servers omit call ids; synthesize stable ones so results
            // can still be paired.
            //
            let id = if id.is_empty() {
                format!("call_{}", index)
            } else {
                id
            };
            parts.push(ContentPart::ToolUse {
                id,
                name,
                input: parse_tool_arguments(&args),
                signature: None,
            });
        }

        ChatCompletionResponse {
            id: String::new(),
            model: String::new(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: assistant_message(parts),
                finish_reason: self.finish_reason,
            }],
            usage: self.usage,
        }
    }
}

//
// Wire-level debug dumps for provider traffic. Uses tracing::debug (not the
// event-log macros) so multi-KB request/response bodies do not flood the
// centralized application event log when RUST_LOG includes this target.
//

fn log_provider_request<T: Serialize>(provider: &str, url: &str, body: &T) {
    match serde_json::to_string(body) {
        Ok(json) => {
            tracing::debug!(
                target: "common::ai::provider",
                "{} request to {}: {}",
                provider,
                url,
                json
            );
        }
        Err(e) => {
            tracing::debug!(
                target: "common::ai::provider",
                "{} request to {}: <failed to serialize body: {}>",
                provider,
                url,
                e
            );
        }
    }
}

fn log_provider_response(provider: &str, url: &str, body: &str) {
    tracing::debug!(
        target: "common::ai::provider",
        "{} response from {}: {}",
        provider,
        url,
        body
    );
}

async fn check_response_status(response: Response) -> Result<Response> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::debug!(
            target: "common::ai::provider",
            "OpenAI stream response error {}: {}",
            status,
            body
        );
        return Err(anyhow!("OpenAI API error {}: {}", status, body));
    }
    Ok(response)
}

//
// Parse an SSE response stream into ChatCompletionDelta chunks.
//

fn parse_sse_stream(
    response: Response,
) -> Pin<Box<dyn Stream<Item = Result<ChatCompletionDelta>> + Send>> {
    Box::pin(try_stream! {
        let mut lines = sse_data_lines(response);
        while let Some(data) = lines.next().await {
            let data = data?;

            //
            // Raw SSE payload as received from the provider — includes
            // every delta chunk and the terminal [DONE] marker.
            //
            tracing::debug!(
                target: "common::ai::provider",
                "OpenAI stream SSE data: {}",
                data
            );

            if data == "[DONE]" {
                break;
            }

            let chunk: OpenAIStreamChunk = match serde_json::from_str(&data) {
                Ok(c) => c,
                Err(e) => {
                    crate::log_warn!(
                        "OpenAI stream: unparseable SSE line ({}): {}",
                        e,
                        crate::truncate_str(&data, 200)
                    );
                    continue;
                }
            };

            //
            // Some providers signal a mid-stream failure (context length,
            // quota, an upstream 5xx) as an `{"error": ...}` envelope with
            // an HTTP 200 status, rather than a failing status code. Surface
            // it as a stream error instead of falling through silently.
            //
            if let Some(err) = chunk.error {
                Err(anyhow!("OpenAI stream error: {}", err.message))?;
            }

            let usage = chunk.usage.map(Usage::from);

            match chunk.choices.first() {
                Some(choice) => {
                    yield ChatCompletionDelta {
                        content: choice.delta.content.clone().unwrap_or_default(),
                        finish_reason: choice.finish_reason.clone(),
                        usage,
                    };
                }
                //
                // Some providers send a final, choice-less chunk carrying
                // only usage stats. Yield it so callers reading usage from
                // the delta stream are not silently starved of it.
                //
                None if usage.is_some() => {
                    yield ChatCompletionDelta {
                        content: String::new(),
                        finish_reason: None,
                        usage,
                    };
                }
                None => {}
            }
        }
    })
}

//
// OpenAI API request/response types.
//

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<OpenAIToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl OpenAIMessage {
    fn text(role: &str, content: String) -> Self {
        Self {
            role: role.to_string(),
            content: Some(content),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type", default = "function_type")]
    call_type: String,
    function: OpenAIFunctionCall,
}

//
// Several compatible servers send `"tool_calls": null` instead of omitting
// the field.
//
fn null_as_empty<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

fn function_type() -> String {
    "function".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIFunctionDef,
}

#[derive(Debug, Serialize)]
struct OpenAIFunctionDef {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parameters: Value,
}

#[derive(Debug, Serialize)]
struct OpenAIStreamOptions {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenAITool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<OpenAIStreamOptions>,
}

//
// Streaming response types.
//

#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    #[serde(default)]
    choices: Vec<OpenAIStreamChoice>,
    usage: Option<OpenAIUsage>,
    error: Option<OpenAIStreamErrorEnvelope>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamErrorEnvelope {
    message: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    delta: OpenAIStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default, deserialize_with = "null_as_empty")]
    tool_calls: Vec<OpenAIStreamToolCall>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamToolCall {
    index: Option<usize>,
    id: Option<String>,
    function: Option<OpenAIStreamFunction>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponseMessage {
    role: String,
    content: Option<String>,
    #[serde(default, deserialize_with = "null_as_empty")]
    tool_calls: Vec<OpenAIToolCall>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    index: usize,
    message: OpenAIResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    id: String,
    model: String,
    choices: Vec<OpenAIChoice>,
    usage: Option<OpenAIUsage>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_chunk_deserializes_error_envelope() {
        let data = r#"{"error": {"message": "context_length_exceeded"}}"#;
        let chunk: OpenAIStreamChunk = serde_json::from_str(data).unwrap();
        assert_eq!(chunk.error.unwrap().message, "context_length_exceeded");
    }

    #[test]
    fn test_stream_chunk_usage_only_chunk_has_no_choices() {
        let data = r#"{"choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}}"#;
        let chunk: OpenAIStreamChunk = serde_json::from_str(data).unwrap();
        assert!(chunk.choices.is_empty());
        assert_eq!(chunk.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn test_stream_chunk_choices_defaults_when_absent() {
        let data = r#"{"usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}}"#;
        let chunk: OpenAIStreamChunk = serde_json::from_str(data).unwrap();
        assert!(chunk.choices.is_empty());
    }

    fn tool_turn_messages() -> Vec<Message> {
        vec![
            Message::system("sys"),
            Message::user("list nodes"),
            Message::with_parts(
                Role::Assistant,
                vec![
                    ContentPart::Thinking {
                        thinking: "hidden".into(),
                        signature: None,
                    },
                    ContentPart::ToolUse {
                        id: "call_1".into(),
                        name: "node_list".into(),
                        input: serde_json::json!({"verbose": true}),
                        signature: None,
                    },
                ],
            ),
            Message::with_parts(
                Role::User,
                vec![ContentPart::ToolResult {
                    tool_use_id: "call_1".into(),
                    name: "node_list".into(),
                    content: "[]".into(),
                    is_error: false,
                }],
            ),
        ]
    }

    #[test]
    fn request_maps_tools_and_tool_turns() {
        let client =
            OpenAIClient::with_base_url("k".into(), "https://api.groq.com/openai/v1".into());
        let request = ChatCompletionRequest::new("m", tool_turn_messages())
            .with_max_tokens(100)
            .with_tools(vec![Tool::new("node_list").with_description("List nodes")])
            .with_reasoning(crate::ai::types::ReasoningConfig {
                effort: Some("Low".into()),
            });
        let body = serde_json::to_value(client.build_request(&request, true)).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "model": "m",
                "messages": [
                    {"role": "system", "content": "sys"},
                    {"role": "user", "content": "list nodes"},
                    {"role": "assistant", "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "node_list", "arguments": "{\"verbose\":true}"}
                    }]},
                    {"role": "tool", "content": "[]", "tool_call_id": "call_1"}
                ],
                "max_tokens": 100,
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "node_list",
                        "description": "List nodes",
                        "parameters": {"type": "object", "properties": {}}
                    }
                }],
                "reasoning_effort": "low",
                "stream": true
            })
        );
    }

    #[test]
    fn official_endpoint_uses_max_completion_tokens() {
        let client = OpenAIClient::new("k".into());
        let request =
            ChatCompletionRequest::new("gpt", vec![Message::user("hi")]).with_max_tokens(50);
        let body = serde_json::to_value(client.build_request(&request, true)).unwrap();
        assert_eq!(body["max_completion_tokens"], 50);
        assert!(body.get("max_tokens").is_none());
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn response_with_tool_calls_round_trips() {
        let fixture = serde_json::json!({
            "id": "chatcmpl-1",
            "model": "m",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": "think",
                    "tool_calls": [{
                        "id": "call_9",
                        "type": "function",
                        "function": {"name": "agent_list", "arguments": "{\"node_id\":\"n1\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7}
        });
        let api: OpenAIResponse = serde_json::from_value(fixture).unwrap();
        let resp = convert_response(api);
        let parts = resp.choices[0].message.parts();
        assert_eq!(
            parts,
            vec![
                ContentPart::Thinking {
                    thinking: "think".into(),
                    signature: None
                },
                ContentPart::ToolUse {
                    id: "call_9".into(),
                    name: "agent_list".into(),
                    input: serde_json::json!({"node_id": "n1"}),
                    signature: None
                }
            ]
        );

        //
        // Replaying the assembled message reproduces the tool call.
        //
        let replay = convert_messages(&[resp.choices[0].message.clone()]);
        assert_eq!(
            serde_json::to_value(&replay).unwrap(),
            serde_json::json!([{
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_9",
                    "type": "function",
                    "function": {"name": "agent_list", "arguments": "{\"node_id\":\"n1\"}"}
                }]
            }])
        );
    }

    #[test]
    fn null_tool_calls_deserialize_as_empty() {
        let msg: OpenAIResponseMessage =
            serde_json::from_str(r#"{"role":"assistant","content":"hi","tool_calls":null}"#)
                .unwrap();
        assert!(msg.tool_calls.is_empty());
    }

    #[test]
    fn stream_accumulator_assembles_fragmented_tool_calls() {
        let chunks = [
            r#"{"choices":[{"delta":{"reasoning":"Plan."},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"content":"Checking"},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"node_","arguments":""}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"list","arguments":"{}"}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_b","function":{"name":"op_available","arguments":"{\"a\""}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":":1}"}}]},"finish_reason":"tool_calls"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#,
        ];
        let mut acc = StreamAccumulator::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        for raw in chunks {
            for ev in acc.handle(serde_json::from_str(raw).unwrap()).unwrap() {
                match ev {
                    StreamEvent::TextDelta(t) => text.push_str(&t),
                    StreamEvent::ReasoningDelta(r) => reasoning.push_str(&r),
                    StreamEvent::Completed(_) => unreachable!(),
                }
            }
        }
        assert_eq!(text, "Checking");
        assert_eq!(reasoning, "Plan.");
        let resp = acc.finish();
        assert_eq!(resp.usage.unwrap().total_tokens, 3);
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let parts = resp.choices[0].message.parts();
        assert_eq!(parts.len(), 4);
        assert_eq!(
            parts[2],
            ContentPart::ToolUse {
                id: "call_a".into(),
                name: "node_list".into(),
                input: serde_json::json!({}),
                signature: None
            }
        );
        assert_eq!(
            parts[3],
            ContentPart::ToolUse {
                id: "call_b".into(),
                name: "op_available".into(),
                input: serde_json::json!({"a": 1}),
                signature: None
            }
        );
    }

    #[test]
    fn test_client_creation() {
        let client = OpenAIClient::new("test-key".to_string());
        assert_eq!(client.api_key, "test-key");
        assert!(client.base_url.contains("openai.com"));
    }

    #[test]
    fn test_custom_base_url() {
        let client = OpenAIClient::with_base_url(
            "test-key".to_string(),
            "https://api.groq.com/v1".to_string(),
        );
        assert!(client.base_url.contains("groq.com"));
        assert!(client.base_url.ends_with("/chat/completions"));
    }

    #[test]
    fn test_base_url_normalization() {
        //
        // Without trailing slash.
        //
        let client1 = OpenAIClient::with_base_url(
            "key".to_string(),
            "https://api.example.com/v1".to_string(),
        );
        assert_eq!(
            client1.base_url,
            "https://api.example.com/v1/chat/completions"
        );

        //
        // With trailing slash.
        //
        let client2 = OpenAIClient::with_base_url(
            "key".to_string(),
            "https://api.example.com/v1/".to_string(),
        );
        assert_eq!(
            client2.base_url,
            "https://api.example.com/v1/chat/completions"
        );

        //
        // Already has endpoint.
        //
        let client3 = OpenAIClient::with_base_url(
            "key".to_string(),
            "https://api.example.com/v1/chat/completions".to_string(),
        );
        assert_eq!(
            client3.base_url,
            "https://api.example.com/v1/chat/completions"
        );
    }
}
