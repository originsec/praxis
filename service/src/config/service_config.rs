use std::collections::HashMap;
use std::sync::Arc;

use crate::database::Database;

//
// LLM model definitions config key (JSON array of model definitions).
//
pub const LLM_MODEL_DEFINITIONS: &str = "llm_model_definitions";

//
// LLM feature assignment config keys.
//
pub const LLM_FEATURE_SEMANTIC_PARSER: &str = "llm_feature_semantic_parser";
pub const LLM_FEATURE_TRAFFIC_PARSER: &str = "llm_feature_traffic_parser";
pub const LLM_FEATURE_SEMANTIC_OPS: &str = "llm_feature_semantic_ops";
#[allow(dead_code)]
pub const LLM_FEATURE_ORCHESTRATOR: &str = "llm_feature_orchestrator";

/// Centralized application/event logging toggle
pub const APPLICATION_LOGS_ENABLED: &str = "application_logs_enabled";

/// Max rows returned from database tables in hunting queries
pub const HUNTING_QUERY_ROW_LIMIT: &str = "hunting_query_row_limit";
pub const HUNTING_QUERY_ROW_LIMIT_DEFAULT: usize = 10_000_000;

/// MCP server configuration keys
pub const MCP_SERVER_ENABLED: &str = "mcp_server_enabled";
pub const MCP_SERVER_PORT: &str = "mcp_server_port";

/// Default MCP server port
pub const MCP_SERVER_DEFAULT_PORT: u16 = 8585;

/// A model definition stored in config
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelDefinition {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
}

/// Service configuration backed by database storage
pub struct ServiceConfig {
    db: Arc<Database>,
    cache: HashMap<String, String>,
}

impl ServiceConfig {
    /// Create a new ServiceConfig backed by the given database
    pub async fn new(db: Arc<Database>) -> anyhow::Result<Self> {
        let cache = db.get_all_config().await?;
        Ok(Self { db, cache })
    }

    /// Reload all config values from the database
    pub async fn reload(&mut self) -> anyhow::Result<()> {
        self.cache = self.db.get_all_config().await?;
        Ok(())
    }

    /// Get a configuration value by key (from cache)
    pub fn get(&self, key: &str) -> Option<&String> {
        self.cache.get(key)
    }

    /// Get a boolean configuration value by key (from cache)
    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        match self.get(key) {
            Some(value) => {
                let normalized = value.to_lowercase();
                !(normalized == "false" || normalized == "0" || normalized == "no")
            }
            None => default,
        }
    }

    /// Set a configuration value (writes to database and updates cache)
    pub async fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> anyhow::Result<()> {
        let key = key.into();
        let value = value.into();
        self.db.set_config(&key, &value).await?;
        self.cache.insert(key, value);
        Ok(())
    }

    /// Remove a configuration key
    #[allow(dead_code)]
    pub async fn remove(&mut self, key: &str) -> anyhow::Result<Option<String>> {
        self.db.delete_config(key).await?;
        Ok(self.cache.remove(key))
    }

    /// Get LLM model definitions from config
    pub fn get_model_definitions(&self) -> Vec<ModelDefinition> {
        if let Some(json_str) = self.get(LLM_MODEL_DEFINITIONS) {
            serde_json::from_str(json_str).unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Find a model definition by its name (provider::model format)
    pub fn find_model_definition(&self, model_ref: &str) -> Option<ModelDefinition> {
        self.get_model_definitions()
            .into_iter()
            .find(|m| m.name == model_ref)
    }

    /// Get the model definition assigned to the semantic parser feature
    pub fn get_semantic_parser_model_def(&self) -> Option<ModelDefinition> {
        self.get(LLM_FEATURE_SEMANTIC_PARSER)
            .and_then(|model_ref| self.find_model_definition(model_ref))
    }

    /// Get the model definition assigned to the traffic parser feature
    pub fn get_traffic_parser_model_def(&self) -> Option<ModelDefinition> {
        self.get(LLM_FEATURE_TRAFFIC_PARSER)
            .and_then(|model_ref| self.find_model_definition(model_ref))
    }

    /// Get the model definition assigned to semantic ops feature
    pub fn get_semantic_ops_model_def(&self) -> Option<ModelDefinition> {
        self.get(LLM_FEATURE_SEMANTIC_OPS)
            .and_then(|model_ref| self.find_model_definition(model_ref))
    }

    /// Get the model definition assigned to orchestrator feature
    #[allow(dead_code)]
    pub fn get_orchestrator_model_def(&self) -> Option<ModelDefinition> {
        self.get(LLM_FEATURE_ORCHESTRATOR)
            .and_then(|model_ref| self.find_model_definition(model_ref))
    }

    /// Convert to a HashMap (for backwards compatibility with existing code)
    #[allow(dead_code)]
    pub fn to_hashmap(&self) -> HashMap<String, String> {
        self.cache.clone()
    }

    /// Check if MCP server is enabled
    pub fn is_mcp_server_enabled(&self) -> bool {
        self.get_bool(MCP_SERVER_ENABLED, false)
    }

    /// Get the MCP server port
    pub fn get_mcp_server_port(&self) -> u16 {
        self.get(MCP_SERVER_PORT)
            .and_then(|s| s.parse().ok())
            .unwrap_or(MCP_SERVER_DEFAULT_PORT)
    }
}
