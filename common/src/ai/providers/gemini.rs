use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::ai::types::{
    ChatCompletionChoice, ChatCompletionRequest, ChatCompletionResponse, Message, Role, Usage,
};

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

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

    /// Send a chat completion request
    pub async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        //
        // Build the API URL with model and API key.
        //
        let url = format!(
            "{}:generateContent?key={}",
            format!("{}/{}", GEMINI_API_BASE, request.model),
            self.api_key
        );

        //
        // Separate system instruction from conversation.
        //
        let mut system_instruction: Option<String> = None;
        let mut contents: Vec<GeminiContent> = Vec::new();

        for msg in &request.messages {
            match msg.role {
                Role::System => {
                    //
                    // Gemini uses system_instruction for system messages.
                    //
                    system_instruction = Some(msg.text().to_string());
                }
                Role::User => {
                    contents.push(GeminiContent {
                        role: "user".to_string(),
                        parts: vec![GeminiPart {
                            text: msg.text().to_string(),
                        }],
                    });
                }
                Role::Assistant => {
                    contents.push(GeminiContent {
                        role: "model".to_string(),
                        parts: vec![GeminiPart {
                            text: msg.text().to_string(),
                        }],
                    });
                }
            }
        }

        //
        // Build generation config.
        //
        let generation_config = GeminiGenerationConfig {
            max_output_tokens: request.max_tokens,
            temperature: request.temperature,
            top_p: request.top_p,
        };

        //
        // Build request body.
        //
        let body = GeminiRequest {
            contents,
            system_instruction: system_instruction.map(|text| GeminiSystemInstruction {
                parts: vec![GeminiPart { text }],
            }),
            generation_config: Some(generation_config),
        };

        //
        // Send request.
        //
        let response = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Gemini API error {}: {}", status, body));
        }

        let api_response: GeminiResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse response: {}", e))?;

        //
        // Extract text from response.
        //
        let text = api_response
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .map(|p| p.text.clone())
            .unwrap_or_default();

        let finish_reason = api_response
            .candidates
            .first()
            .and_then(|c| c.finish_reason.clone());

        //
        // Build usage if available.
        //
        let usage = api_response.usage_metadata.map(|u| Usage {
            prompt_tokens: u.prompt_token_count,
            completion_tokens: u.candidates_token_count,
            total_tokens: u.total_token_count,
        });

        Ok(ChatCompletionResponse {
            //
            // Gemini doesn't return an ID.
            //
            id: "gemini-response".to_string(),
            model: request.model,
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: Message::assistant(text),
                finish_reason,
            }],
            usage,
        })
    }
}

//
// Gemini API request/response types.
//

#[derive(Debug, Serialize)]
struct GeminiPart {
    text: String,
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
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
}

#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponsePart {
    text: String,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiResponsePart>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiResponseContent,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: u32,
    #[serde(rename = "totalTokenCount")]
    total_token_count: u32,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = GeminiClient::new("test-key".to_string());
        assert_eq!(client.api_key, "test-key");
    }
}
