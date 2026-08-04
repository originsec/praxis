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
    let body_limit_bytes = config.get_traffic_parser_body_limit_bytes();
    let traffic_content = format_traffic_for_llm(entry, body_limit_bytes);

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
fn format_traffic_for_llm(entry: &InterceptedTrafficEntry, body_limit_bytes: usize) -> String {
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
        parts.push(format!(
            "\nRequest Body:\n{}",
            format_body_for_llm(body, body_limit_bytes)
        ));
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
        parts.push(format!(
            "\nResponse Body:\n{}",
            format_body_for_llm(body, body_limit_bytes)
        ));
    }

    parts.join("\n")
}

fn format_body_for_llm(body: &[u8], limit_bytes: usize) -> String {
    let Ok(text) = std::str::from_utf8(body) else {
        return format!("[Binary data, {} bytes]", body.len());
    };
    if text.len() <= limit_bytes {
        return text.to_string();
    }

    //
    // Keep both ends because LLM conversation requests commonly put the
    // newest messages and tool results at the end of the JSON document.
    //
    let head_limit = limit_bytes / 2;
    let head_end = common::truncate_str(text, head_limit).len();
    let mut tail_start = text.len() - (limit_bytes - head_end);
    while !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }

    format!(
        "{}\n... [middle truncated: showing first {} and last {} of {} bytes] ...\n{}",
        &text[..head_end],
        head_end,
        text.len() - tail_start,
        text.len(),
        &text[tail_start..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::service_config::TRAFFIC_PARSER_BODY_LIMIT_KB_DEFAULT;

    const DEFAULT_LIMIT_BYTES: usize = TRAFFIC_PARSER_BODY_LIMIT_KB_DEFAULT * 1024;

    #[test]
    fn body_under_configured_limit_is_not_truncated() {
        let body = format!(
            "{}{}",
            "a".repeat(50_000),
            r#"{"type":"tool_use","name":"Bash"}"#
        );

        let formatted = format_body_for_llm(body.as_bytes(), DEFAULT_LIMIT_BYTES);

        assert_eq!(formatted, body);
        assert!(formatted.contains(r#""type":"tool_use""#));
        assert!(!formatted.contains("truncated"));
    }

    #[test]
    fn oversized_body_preserves_both_ends() {
        let body = format!("START{}END_TOOL_RESULT", "x".repeat(DEFAULT_LIMIT_BYTES));

        let formatted = format_body_for_llm(body.as_bytes(), DEFAULT_LIMIT_BYTES);

        assert!(formatted.starts_with("START"));
        assert!(formatted.ends_with("END_TOOL_RESULT"));
        assert!(formatted.contains("middle truncated"));
        assert!(formatted.contains(&format!("of {} bytes", body.len())));
    }

    #[test]
    fn truncation_respects_utf8_boundaries() {
        let body = format!("{}TAIL", "é".repeat((DEFAULT_LIMIT_BYTES / 2) + 10));

        let formatted = format_body_for_llm(body.as_bytes(), DEFAULT_LIMIT_BYTES);

        assert!(formatted.ends_with("TAIL"));
        assert!(formatted.contains("middle truncated"));
    }

    #[test]
    fn binary_body_reports_its_size() {
        assert_eq!(
            format_body_for_llm(&[0xff, 0xfe, 0xfd], DEFAULT_LIMIT_BYTES),
            "[Binary data, 3 bytes]"
        );
    }

    #[test]
    fn configured_limit_controls_truncation() {
        let body = b"0123456789";

        assert_eq!(format_body_for_llm(body, 10), "0123456789");
        assert!(format_body_for_llm(body, 9).contains("middle truncated"));
    }
}
