use std::pin::Pin;

use anyhow::{Result, anyhow};
use async_stream::try_stream;
use futures_core::Stream;
use futures_util::StreamExt;
use reqwest::Response;
use serde::{Deserialize, Serialize};

use crate::ai::types::{
    ChatCompletionChoice, ChatCompletionDelta, ChatCompletionRequest, ChatCompletionResponse,
    Content, Message, Role, Usage,
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
        //
        // Convert messages to OpenAI format.
        //
        let messages: Vec<OpenAIMessage> = request
            .messages
            .iter()
            .map(|msg| OpenAIMessage {
                role: msg.role.as_str().to_string(),
                content: msg.text().to_string(),
            })
            .collect();

        //
        // Apply provider-specific max_tokens cap if configured.
        //
        let max_tokens = request.max_tokens;

        let body = OpenAIRequest {
            model: request.model.clone(),
            messages,
            max_tokens,
            temperature: request.temperature,
            top_p: request.top_p,
        };

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

        //
        // Convert to our response format.
        //
        let choices = api_response
            .choices
            .into_iter()
            .map(|choice| ChatCompletionChoice {
                index: choice.index,
                message: Message::new(
                    match choice.message.role.as_str() {
                        "assistant" => Role::Assistant,
                        "user" => Role::User,
                        "system" => Role::System,
                        _ => Role::Assistant,
                    },
                    Content::Text(choice.message.content.unwrap_or_default()),
                ),
                finish_reason: choice.finish_reason,
            })
            .collect();

        Ok(ChatCompletionResponse {
            id: api_response.id,
            model: api_response.model,
            choices,
            usage: api_response.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
        })
    }

    /// Send a streaming chat completion request.
    ///
    /// Returns a stream of ChatCompletionDelta chunks. The final chunk
    /// will have finish_reason set and may include usage stats.
    pub fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<ChatCompletionDelta>> + Send + '_>> {
        let messages: Vec<OpenAIMessage> = request
            .messages
            .iter()
            .map(|msg| OpenAIMessage {
                role: msg.role.as_str().to_string(),
                content: msg.text().to_string(),
            })
            .collect();

        let max_tokens = request.max_tokens;

        let body = OpenAIStreamRequest {
            model: request.model.clone(),
            messages,
            max_tokens,
            temperature: request.temperature,
            top_p: request.top_p,
            stream: true,
        };

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
        use tokio::io::AsyncBufReadExt;
        use tokio_util::io::StreamReader;

        let byte_stream = response.bytes_stream().map(|r| {
            r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        });
        let reader = StreamReader::new(byte_stream);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await
            .map_err(|e| anyhow!("Stream read error: {}", e))? {

            let line = line.trim().to_string();

            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            let data = if let Some(d) = line.strip_prefix("data: ") {
                d.trim()
            } else {
                continue;
            };

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

            let chunk: OpenAIStreamChunk = match serde_json::from_str(data) {
                Ok(c) => c,
                Err(e) => {
                    crate::log_warn!(
                        "OpenAI stream: unparseable SSE line ({}): {}",
                        e,
                        crate::truncate_str(data, 200)
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

            let usage = chunk.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            });

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
    content: String,
}

#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
}

#[derive(Debug, Serialize)]
struct OpenAIStreamRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    stream: bool,
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
}

#[derive(Debug, Deserialize)]
struct OpenAIResponseMessage {
    role: String,
    content: Option<String>,
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
