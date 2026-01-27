use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::ai::types::{
    ChatCompletionChoice, ChatCompletionRequest, ChatCompletionResponse, Content, Message, Role,
    Usage,
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
        // Build request body.
        //
        let body = OpenAIRequest {
            model: request.model.clone(),
            messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            top_p: request.top_p,
        };

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
            return Err(anyhow!("OpenAI API error {}: {}", status, body));
        }

        let api_response: OpenAIResponse = response
            .json()
            .await
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
    fn test_client_creation() {
        let client = OpenAIClient::new("test-key".to_string());
        assert_eq!(client.api_key, "test-key");
        assert!(client.base_url.contains("openai.com"));
    }

    #[test]
    fn test_custom_base_url() {
        let client =
            OpenAIClient::with_base_url("test-key".to_string(), "https://api.groq.com/v1".to_string());
        assert!(client.base_url.contains("groq.com"));
        assert!(client.base_url.ends_with("/chat/completions"));
    }

    #[test]
    fn test_base_url_normalization() {
        //
        // Without trailing slash.
        //
        let client1 = OpenAIClient::with_base_url("key".to_string(), "https://api.example.com/v1".to_string());
        assert_eq!(client1.base_url, "https://api.example.com/v1/chat/completions");

        //
        // With trailing slash.
        //
        let client2 = OpenAIClient::with_base_url("key".to_string(), "https://api.example.com/v1/".to_string());
        assert_eq!(client2.base_url, "https://api.example.com/v1/chat/completions");

        //
        // Already has endpoint.
        //
        let client3 = OpenAIClient::with_base_url(
            "key".to_string(),
            "https://api.example.com/v1/chat/completions".to_string(),
        );
        assert_eq!(client3.base_url, "https://api.example.com/v1/chat/completions");
    }
}
