use std::sync::Arc;
use common::{SemanticParserRequest, SemanticParserResponse};
use semantic_parser::{ParserConfig, Provider, SemanticParser};
use tokio::sync::RwLock;

use crate::config::ServiceConfig;

/// Handle a semantic parser request using the semantic_parser crate.
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

    //
    // Build parser config and create the semantic parser.
    //

    let parser_config = ParserConfig {
        provider,
        api_key: model_def.api_key,
        model: model_def.model.clone(),
        max_retries: 3,
        max_tokens: Some(4096),
    };

    let parser = match SemanticParser::new(parser_config) {
        Ok(p) => p,
        Err(e) => {
            return SemanticParserResponse {
                request_id: request.request_id.clone(),
                success: false,
                json: None,
                error: Some(format!("Failed to create semantic parser: {}", e)),
            };
        }
    };

    common::log_info!(
        "Semantic parser request {} using {:?}/{}",
        &request.request_id[..8.min(request.request_id.len())],
        provider,
        model_def.model
    );

    //
    // Execute the parse operation.
    //

    match parser.parse(&request.text, &request.instruction, &request.schema).await {
        Ok(json) => SemanticParserResponse {
            request_id: request.request_id.clone(),
            success: true,
            json: Some(json),
            error: None,
        },
        Err(e) => SemanticParserResponse {
            request_id: request.request_id.clone(),
            success: false,
            json: None,
            error: Some(e.to_string()),
        },
    }
}
