use common::{
    InterceptedTrafficEntry, Provider, Role, build_message, create_ai_client,
    execute_chat_completion,
};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::ServiceConfig;

/// System prompt for traffic summarization
const SYSTEM_PROMPT: &str = r#"You are a traffic analyzer assistant. Your task is to analyze HTTP/WebSocket traffic and provide a concise summary based on the user's instructions.
Focus on extracting the most relevant information from the traffic data provided. Be concise but thorough."#;

/// Result of a summarization attempt
pub struct SummarizationResult {
    pub success: bool,
    pub summary: Option<String>,
    pub error: Option<String>,
}

/// Summarize intercepted traffic using the configured LLM
pub async fn summarize_traffic(
    config: &Arc<RwLock<ServiceConfig>>,
    entry: &InterceptedTrafficEntry,
    summarization_prompt: &str,
) -> SummarizationResult {
    //
    // Acquire read lock on config.
    //
    let config = config.read().await;

    //
    // Get model definition from traffic parser feature assignment.
    //
    let model_def = match config.get_traffic_parser_model_def() {
        Some(def) => def,
        None => {
            return SummarizationResult {
                success: false,
                summary: None,
                error: Some(
                    "No LLM configured for Traffic Parser. Configure in Settings > LLM Providers."
                        .to_string(),
                ),
            };
        }
    };

    common::log_info!(
        "Using traffic parser model: {} ({})",
        model_def.name,
        model_def.provider
    );
    let provider = match Provider::from_str(&model_def.provider) {
        Some(p) => p,
        None => {
            return SummarizationResult {
                success: false,
                summary: None,
                error: Some(format!(
                    "Invalid provider in model def: {}",
                    model_def.provider
                )),
            };
        }
    };

    let (api_key, model, base_url) = (model_def.api_key, model_def.model, model_def.base_url);

    //
    // Create AI client.
    //
    let client = match create_ai_client(provider, api_key, base_url.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            return SummarizationResult {
                success: false,
                summary: None,
                error: Some(format!("Failed to create AI client: {}", e)),
            };
        }
    };

    //
    // Build traffic content string.
    //
    let traffic_content = format_traffic_for_llm(entry);

    //
    // Build the user prompt.
    //
    let user_prompt = format!(
        "Analyze the following intercepted traffic and provide a summary based on these instructions:\n\n\
        INSTRUCTIONS:\n{}\n\n\
        TRAFFIC DATA:\n{}",
        summarization_prompt, traffic_content
    );

    common::log_info!("=== Traffic Summarization Request ===");
    common::log_info!("URL: {}", entry.url);
    common::log_info!("Prompt: {}", summarization_prompt);
    common::log_info!("=== End Request ===");

    let messages = vec![
        build_message(Role::System, SYSTEM_PROMPT.to_string()),
        build_message(Role::User, user_prompt),
    ];

    match execute_chat_completion(&client, model.clone(), messages, Some(2048)).await {
        Ok(response) => {
            let summary = response.trim().to_string();
            common::log_info!("=== Traffic Summarization Response ===");
            common::log_info!("Summary: {}", common::truncate_str(&summary, 200));
            common::log_info!("=== End Response ===");

            SummarizationResult {
                success: true,
                summary: Some(summary),
                error: None,
            }
        }
        Err(e) => {
            common::log_error!("Traffic summarization failed: {}", e);
            SummarizationResult {
                success: false,
                summary: None,
                error: Some(format!("Summarization failed: {}", e)),
            }
        }
    }
}

/// Format traffic entry for LLM consumption
fn format_traffic_for_llm(entry: &InterceptedTrafficEntry) -> String {
    let mut parts = Vec::new();

    //
    // Basic info.
    //
    parts.push(format!("URL: {}", entry.url));
    parts.push(format!(
        "Method: {}",
        entry.method.as_deref().unwrap_or("GET")
    ));
    parts.push(format!("Host: {}", entry.host));
    parts.push(format!("Direction: {:?}", entry.direction));
    parts.push(format!("Timestamp: {}", entry.timestamp));

    //
    // Request headers.
    //
    if let Some(ref headers) = entry.request_headers {
        parts.push("\nRequest Headers:".to_string());
        for (key, value) in headers {
            parts.push(format!("  {}: {}", key, value));
        }
    }

    //
    // Request body.
    //
    if let Some(ref body) = entry.request_body {
        if let Ok(body_str) = std::str::from_utf8(body) {
            let truncated = if body_str.len() > 4000 {
                format!(
                    "{}... [truncated, {} bytes total]",
                    &body_str[..4000],
                    body_str.len()
                )
            } else {
                body_str.to_string()
            };
            parts.push(format!("\nRequest Body:\n{}", truncated));
        } else {
            parts.push(format!(
                "\nRequest Body: [Binary data, {} bytes]",
                body.len()
            ));
        }
    }

    //
    // Response status.
    //
    if let Some(status) = entry.response_status {
        parts.push(format!("\nResponse Status: {}", status));
    }

    //
    // Response headers.
    //
    if let Some(ref headers) = entry.response_headers {
        parts.push("\nResponse Headers:".to_string());
        for (key, value) in headers {
            parts.push(format!("  {}: {}", key, value));
        }
    }

    //
    // Response body.
    //
    if let Some(ref body) = entry.response_body {
        if let Ok(body_str) = std::str::from_utf8(body) {
            let truncated = if body_str.len() > 4000 {
                format!(
                    "{}... [truncated, {} bytes total]",
                    &body_str[..4000],
                    body_str.len()
                )
            } else {
                body_str.to_string()
            };
            parts.push(format!("\nResponse Body:\n{}", truncated));
        } else {
            parts.push(format!(
                "\nResponse Body: [Binary data, {} bytes]",
                body.len()
            ));
        }
    }

    parts.join("\n")
}
