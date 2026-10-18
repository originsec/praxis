use std::pin::Pin;

use anyhow::{Result, anyhow};
use async_stream::try_stream;
use futures_core::Stream;
use futures_util::StreamExt;
use serde_json::Value;

use super::client::AiClient;
use super::types::{ChatCompletionRequest, ChatCompletionResponse, ContentPart, Message, Role};

/// An incremental event from a structured (tool/reasoning aware) stream.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Visible assistant text.
    TextDelta(String),
    /// Model reasoning text (thinking summary / reasoning content).
    ReasoningDelta(String),
    /// The fully assembled response. Always the last event of a stream.
    /// The assistant message carries `ContentPart`s when the turn includes
    /// tool calls or thinking blocks.
    Completed(ChatCompletionResponse),
}

pub type StreamEventStream<'a> = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'a>>;

//
// A tool call whose streamed arguments were not valid JSON (usually cut off
// by the output-token limit) is kept replayable as an object holding the raw
// text under this key, so the caller can return an error tool result instead
// of executing it.
//
pub const INVALID_TOOL_ARGUMENTS_KEY: &str = "_invalid_arguments";

impl AiClient {
    /// Stream a chat completion as structured events. Unlike
    /// `chat_completion_stream`, this honours `request.tools` (native
    /// function calling) and `request.reasoning`, and ends with a
    /// `StreamEvent::Completed` carrying the assembled message.
    pub fn chat_completion_events(&self, request: ChatCompletionRequest) -> StreamEventStream<'_> {
        match self {
            AiClient::Anthropic(client) => client.stream_events(request),
            AiClient::OpenAI(client) => client.stream_events(request),
            AiClient::Gemini(client) => client.stream_events(request),
        }
    }
}

/// Parse a tool-call argument string into a JSON object. Empty input is an
/// empty object; anything that is not a JSON object is wrapped under
/// `INVALID_TOOL_ARGUMENTS_KEY`.
pub fn parse_tool_arguments(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return Value::Object(Default::default());
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(v @ Value::Object(_)) => v,
        _ => serde_json::json!({ INVALID_TOOL_ARGUMENTS_KEY: raw }),
    }
}

/// Build an assistant message from parts, collapsing a lone text part (or
/// nothing) to plain text so simple turns stay simple.
pub fn assistant_message(parts: Vec<ContentPart>) -> Message {
    let simple = parts.iter().all(|p| matches!(p, ContentPart::Text { .. }));
    if simple {
        let text: String = parts
            .into_iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text),
                _ => None,
            })
            .collect();
        Message::assistant(text)
    } else {
        Message::with_parts(Role::Assistant, merge_adjacent_text(parts))
    }
}

//
// Streaming providers emit text in many small pieces; keep the stored
// message compact by merging consecutive text parts.
//
fn merge_adjacent_text(parts: Vec<ContentPart>) -> Vec<ContentPart> {
    let mut out: Vec<ContentPart> = Vec::with_capacity(parts.len());
    for part in parts {
        match (out.last_mut(), part) {
            (Some(ContentPart::Text { text: prev }), ContentPart::Text { text }) => {
                prev.push_str(&text);
            }
            (_, part) => out.push(part),
        }
    }
    out
}

/// Read an SSE response body and yield the payload of each `data:` line.
/// Comment lines, blank lines and other fields are skipped.
pub(crate) fn sse_data_lines(
    response: reqwest::Response,
) -> Pin<Box<dyn Stream<Item = Result<String>> + Send>> {
    Box::pin(try_stream! {
        use tokio::io::AsyncBufReadExt;
        use tokio_util::io::StreamReader;

        let byte_stream = response
            .bytes_stream()
            .map(|r| r.map_err(std::io::Error::other));
        let reader = StreamReader::new(byte_stream);
        let mut lines = reader.lines();

        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| anyhow!("Stream read error: {}", e))?
        {
            let line = line.trim();
            if let Some(data) = line.strip_prefix("data:") {
                yield data.trim().to_string();
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::types::Content;

    #[test]
    fn parse_tool_arguments_handles_empty_valid_and_invalid() {
        assert_eq!(parse_tool_arguments(""), serde_json::json!({}));
        assert_eq!(
            parse_tool_arguments(r#"{"a": 1}"#),
            serde_json::json!({"a": 1})
        );
        let invalid = parse_tool_arguments(r#"{"a": "#);
        assert_eq!(invalid[INVALID_TOOL_ARGUMENTS_KEY], r#"{"a": "#);
        let not_object = parse_tool_arguments("[1,2]");
        assert!(not_object.get(INVALID_TOOL_ARGUMENTS_KEY).is_some());
    }

    #[test]
    fn assistant_message_collapses_plain_text() {
        let msg = assistant_message(vec![
            ContentPart::Text { text: "Hel".into() },
            ContentPart::Text { text: "lo".into() },
        ]);
        assert!(matches!(msg.content, Content::Text(ref t) if t == "Hello"));
    }

    #[test]
    fn assistant_message_keeps_structure_and_merges_text() {
        let msg = assistant_message(vec![
            ContentPart::Thinking {
                thinking: "hmm".into(),
                signature: Some("sig".into()),
            },
            ContentPart::Text { text: "a".into() },
            ContentPart::Text { text: "b".into() },
            ContentPart::ToolUse {
                id: "t1".into(),
                name: "node_list".into(),
                input: serde_json::json!({}),
                signature: None,
            },
        ]);
        let parts = msg.parts();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[1], ContentPart::Text { text: "ab".into() });
    }
}
