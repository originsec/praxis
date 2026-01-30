use std::sync::Arc;
use common::{
    Provider, SemanticParserRequest, SemanticParserResponse,
    create_ai_client, execute_chat_completion, build_message, Role,
};
use tokio::sync::RwLock;

use crate::config::ServiceConfig;

const MAX_RETRIES: usize = 3;

/// System prompt for the semantic parser
const SYSTEM_PROMPT: &str = r#"You are a semantic parser. Your task is to parse the provided text and extract structured data according to the JSON schema provided.

IMPORTANT RULES:
1. You MUST return ONLY valid JSON that matches the schema exactly
2. Do NOT include any explanatory text, markdown formatting, or code blocks
3. Do NOT include ```json or ``` markers
4. Return ONLY the raw JSON object
5. If you cannot extract the required data, return an empty object {} or appropriate default values

The output must be valid JSON that can be parsed by a JSON parser."#;

/// Handle a semantic parser request
pub async fn handle_semantic_parser_request(
    config: &Arc<RwLock<ServiceConfig>>,
    request: &SemanticParserRequest,
) -> SemanticParserResponse {
    //
    // Acquire read lock on config.
    //
    let config = config.read().await;

    //
    // Get credentials from model definition.
    //
    let model_def = match config.get_semantic_parser_model_def() {
        Some(def) => def,
        None => {
            return SemanticParserResponse {
                request_id: request.request_id.clone(),
                success: false,
                json: None,
                error: Some("No LLM configured for Semantic Parser. Configure in Settings > LLM Providers.".to_string()),
            };
        }
    };

    let provider = match Provider::from_str(&model_def.provider) {
        Some(p) => p,
        None => {
            return SemanticParserResponse {
                request_id: request.request_id.clone(),
                success: false,
                json: None,
                error: Some(format!("Invalid provider in model definition: {}", model_def.provider)),
            };
        }
    };

    let (api_key, model) = (model_def.api_key, model_def.model);

    //
    // Create AI client.
    //
    let client = match create_ai_client(provider, api_key) {
        Ok(c) => c,
        Err(e) => {
            return SemanticParserResponse {
                request_id: request.request_id.clone(),
                success: false,
                json: None,
                error: Some(format!("Failed to create AI client: {}", e)),
            };
        }
    };

    //
    // Build the user prompt with schema and data.
    //
    let user_prompt = format!(
        "Parse the provided TEXT according to the INSTRUCTIONS and yield a json output in the form of the provided SCHEMA only. (Don't output anything but valid JSON):\n\nSCHEMA:\n{}\n\nPARSING INSTRUCTIONS:\n{}",
        request.schema,
        request.prompt
    );

    //
    // Log the request being sent to the model.
    //
    common::log_info!("=== Semantic Parser Request ===");
    common::log_info!("Request ID: {}", request.request_id);
    common::log_info!("Provider: {:?}, Model: {}", provider, model);
    common::log_info!("--- User Prompt ---\n{}", user_prompt);
    common::log_info!("=== End Request ===");

    //
    // Try up to MAX_RETRIES times to get valid JSON.
    //
    let mut last_error = String::new();

    for attempt in 1..=MAX_RETRIES {
        common::log_info!(
            "Semantic parser attempt {}/{} for request {}",
            attempt, MAX_RETRIES, &request.request_id[..8.min(request.request_id.len())]
        );

        let messages = vec![
            build_message(Role::System, SYSTEM_PROMPT.to_string()),
            build_message(Role::User, user_prompt.clone()),
        ];

        match execute_chat_completion(&client, model.clone(), messages, Some(4096)).await {
            Ok(response) => {
                //
                // Log the raw response from the model.
                //
                common::log_info!("=== Semantic Parser Response (attempt {}) ===", attempt);
                common::log_info!("Request ID: {}", request.request_id);
                common::log_info!("--- Model Response ---\n{}", response);
                common::log_info!("=== End Response ===");

                //
                // Try to parse the response as JSON.
                //
                let trimmed = response.trim();

                //
                // Remove potential markdown code block markers.
                //
                let json_str = if trimmed.starts_with("```json") {
                    trimmed.strip_prefix("```json")
                        .and_then(|s| s.strip_suffix("```"))
                        .unwrap_or(trimmed)
                        .trim()
                } else if trimmed.starts_with("```") {
                    trimmed.strip_prefix("```")
                        .and_then(|s| s.strip_suffix("```"))
                        .unwrap_or(trimmed)
                        .trim()
                } else {
                    trimmed
                };

                match serde_json::from_str::<serde_json::Value>(json_str) {
                    Ok(_) => {
                        common::log_info!(
                            "Semantic parser succeeded on attempt {} for request {}",
                            attempt, &request.request_id[..8.min(request.request_id.len())]
                        );
                        return SemanticParserResponse {
                            request_id: request.request_id.clone(),
                            success: true,
                            json: Some(json_str.to_string()),
                            error: None,
                        };
                    }
                    Err(e) => {
                        last_error = format!("Invalid JSON on attempt {}: {}", attempt, e);
                        common::log_warn!("{}", last_error);
                    }
                }
            }
            Err(e) => {
                last_error = format!("AI request failed on attempt {}: {}", attempt, e);
                common::log_error!("{}", last_error);
            }
        }
    }

    //
    // All retries exhausted.
    //
    SemanticParserResponse {
        request_id: request.request_id.clone(),
        success: false,
        json: None,
        error: Some(format!("Failed after {} attempts: {}", MAX_RETRIES, last_error)),
    }
}
