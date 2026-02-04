use anyhow::{anyhow, Result};
use common::ai::{create_ai_client, AiClient, ChatCompletionRequest, Message};
use tracing::{error, info, warn};

use crate::config::ParserConfig;

/// System prompt for the semantic parser
const SYSTEM_PROMPT: &str = r#"You are a semantic parser. Your task is to parse the provided text and extract structured data according to the JSON schema provided.

The input TEXT can be in any format - plain text, xml, json (yes still parse this according to the provided schema), etc.

IMPORTANT RULES:
1. You MUST return ONLY valid JSON that matches the schema exactly
2. Do NOT include any explanatory text, markdown formatting, or code blocks
3. Do NOT include ```json or ``` markers
4. Return ONLY the raw JSON object
5. If you cannot extract the required data, return an empty object {} or appropriate default values

The output must be valid JSON that can be parsed by a JSON parser."#;

/// Result of a parse operation
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// Whether parsing succeeded
    pub success: bool,
    /// Parsed JSON string (if successful)
    pub json: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
}

/// Semantic parser for extracting structured data from text
pub struct SemanticParser {
    client: AiClient,
    model: String,
    max_retries: usize,
    max_tokens: u32,
}

impl SemanticParser {
    /// Create a new semantic parser with the given configuration
    pub fn new(config: ParserConfig) -> Result<Self> {
        let client = create_ai_client(config.provider, config.api_key)?;

        Ok(Self {
            client,
            model: config.model,
            max_retries: config.max_retries,
            max_tokens: config.max_tokens.unwrap_or(4096),
        })
    }

    /// Parse text according to a schema and prompt
    ///
    /// # Arguments
    /// * `text` - The text to parse
    /// * `instructions` - Instructions for what to extract
    /// * `schema` - JSON schema defining the expected output structure
    ///
    /// # Returns
    /// The parsed JSON string if successful
    pub async fn parse(&self, text: &str, instructions: &str, schema: &str) -> Result<String> {
        let full_prompt = format!(
            "Parse the provided TEXT according to the INSTRUCTIONS and yield a json output in the form of the provided SCHEMA only. (Don't output anything but valid JSON):\n\nSCHEMA:\n{}\n\nINSTRUCTIONS:\n{}\n\nTEXT:\n{}",
            schema, instructions, text
        );

        self.execute_parse(&full_prompt).await
    }

    /// Execute the parse operation with retries
    async fn execute_parse(&self, user_prompt: &str) -> Result<String> {
        let mut last_error = String::new();

        for attempt in 1..=self.max_retries {
            info!("Semantic parser attempt {}/{}", attempt, self.max_retries);

            let messages = vec![
                Message::system(SYSTEM_PROMPT),
                Message::user(user_prompt),
            ];

            let request = ChatCompletionRequest::new(self.model.clone(), messages)
                .with_max_tokens(self.max_tokens);

            match self.client.chat_completion(request).await {
                Ok(response) => {
                    //
                    // Extract text content from the response.
                    //

                    let text_content = response.text().unwrap_or_default().to_string();
                    info!(
                        "Raw model response (attempt {}, {} chars): {}",
                        attempt,
                        text_content.len(),
                        text_content
                    );

                    //
                    // Try to parse the response as JSON.
                    //
                    let trimmed = text_content.trim();

                    //
                    // Remove potential markdown code block markers.
                    //
                    let json_str = Self::strip_markdown(trimmed);

                    match serde_json::from_str::<serde_json::Value>(json_str) {
                        Ok(_) => {
                            info!("Semantic parser succeeded on attempt {}", attempt);
                            return Ok(json_str.to_string());
                        }
                        Err(e) => {
                            last_error = format!("Invalid JSON on attempt {}: {}", attempt, e);
                            warn!("{}", last_error);
                        }
                    }
                }
                Err(e) => {
                    last_error = format!("AI request failed on attempt {}: {}", attempt, e);
                    error!("{}", last_error);
                }
            }
        }

        Err(anyhow!(
            "Failed after {} attempts: {}",
            self.max_retries,
            last_error
        ))
    }

    /// Parse and return a structured result (doesn't error, returns ParseResult)
    pub async fn try_parse(&self, text: &str, prompt: &str, schema: &str) -> ParseResult {
        match self.parse(text, prompt, schema).await {
            Ok(json) => ParseResult {
                success: true,
                json: Some(json),
                error: None,
            },
            Err(e) => ParseResult {
                success: false,
                json: None,
                error: Some(e.to_string()),
            },
        }
    }

    /// Strip markdown code block markers from response
    fn strip_markdown(text: &str) -> &str {
        if text.starts_with("```json") {
            text.strip_prefix("```json")
                .and_then(|s| s.strip_suffix("```"))
                .unwrap_or(text)
                .trim()
        } else if text.starts_with("```") {
            text.strip_prefix("```")
                .and_then(|s| s.strip_suffix("```"))
                .unwrap_or(text)
                .trim()
        } else {
            text
        }
    }
}
