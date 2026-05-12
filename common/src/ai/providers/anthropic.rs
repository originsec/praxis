use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::ai::types::{
    ChatCompletionChoice, ChatCompletionRequest, ChatCompletionResponse, Message, Role, Usage,
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

    /// Send a chat completion request
    pub async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        //
        // Separate system message from conversation messages.
        //
        let (system_content, messages): (Option<String>, Vec<_>) = {
            let mut system = None;
            let mut msgs = Vec::new();

            for msg in request.messages {
                match msg.role {
                    Role::System => {
                        //
                        // Anthropic: system goes in a separate field.
                        //
                        system = Some(msg.text().to_string());
                    }
                    Role::User | Role::Assistant => {
                        msgs.push(AnthropicMessage {
                            role: msg.role.as_str().to_string(),
                            content: msg.text().to_string(),
                        });
                    }
                }
            }

            (system, msgs)
        };

        //
        // Build request body.
        //
        let body = AnthropicRequest {
            model: request.model.clone(),
            max_tokens: request.max_tokens.unwrap_or(4096),
            system: system_content,
            messages,
            temperature: request.temperature,
            top_p: request.top_p,
        };

        //
        // Send request.
        //
        let response = self
            .http_client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Anthropic API error {}: {}", status, body));
        }

        let api_response: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse response: {}", e))?;

        //
        // Convert to our response format.
        //
        let text = api_response
            .content
            .iter()
            .filter_map(|block| {
                if block.content_type == "text" {
                    block.text.clone()
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");

        Ok(ChatCompletionResponse {
            id: api_response.id,
            model: api_response.model,
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: Message::assistant(text),
                finish_reason: api_response.stop_reason,
            }],
            usage: Some(Usage {
                prompt_tokens: api_response.usage.input_tokens,
                completion_tokens: api_response.usage.output_tokens,
                total_tokens: api_response.usage.input_tokens + api_response.usage.output_tokens,
            }),
        })
    }
}

//
// Anthropic API request/response types.
//

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
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
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = AnthropicClient::new("test-key".to_string());
        assert_eq!(client.api_key, "test-key");
    }
}
