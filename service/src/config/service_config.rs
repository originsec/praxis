use std::collections::HashMap;
use std::sync::Arc;

use crate::database::Database;
use common::{PraxisAgentConfig, Provider};

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
pub const LLM_FEATURE_ORCHESTRATOR: &str = "llm_feature_orchestrator";

/// Praxis agent configuration keys.
pub const PRAXIS_AGENT_SETTINGS: &str = "praxis_agent_settings";
pub const PRAXIS_AGENT_SYSTEM_PROMPT: &str = "praxis_agent_system_prompt";

/// Centralized application/event logging toggle
pub const APPLICATION_LOGS_ENABLED: &str = "application_logs_enabled";

/// Max rows returned from database tables in log-query searches
pub const LOG_QUERY_ROW_LIMIT: &str = "log_query_row_limit";
pub const LOG_QUERY_ROW_LIMIT_DEFAULT: usize = 10_000_000;

/// MCP server configuration keys
pub const MCP_SERVER_ENABLED: &str = "mcp_server_enabled";
pub const MCP_SERVER_PORT: &str = "mcp_server_port";

/// Default MCP server port
pub const MCP_SERVER_DEFAULT_PORT: u16 = 8585;

/// Prompt timeout in seconds (how long a single agent prompt can run)
pub const PROMPT_TIMEOUT_SECS: &str = "prompt_timeout_secs";
pub const PROMPT_TIMEOUT_SECS_DEFAULT: u64 = 600;

/// Claude bridge configuration keys
pub const CLAUDE_CCRV1_ENABLED: &str = "claude_ccrv1_enabled";
pub const CLAUDE_CCRV1_PORT: &str = "claude_ccrv1_port";
pub const CLAUDE_CCRV2_ENABLED: &str = "claude_ccrv2_enabled";
pub const CLAUDE_CCRV2_PORT: &str = "claude_ccrv2_port";

/// Default Claude bridge ports
pub const CLAUDE_CCRV1_DEFAULT_PORT: u16 = 8586;
pub const CLAUDE_CCRV2_DEFAULT_PORT: u16 = 8587;

/// All recognized config keys for validation purposes.
pub const KNOWN_CONFIG_KEYS: &[&str] = &[
    LLM_MODEL_DEFINITIONS,
    LLM_FEATURE_SEMANTIC_PARSER,
    LLM_FEATURE_TRAFFIC_PARSER,
    LLM_FEATURE_SEMANTIC_OPS,
    LLM_FEATURE_ORCHESTRATOR,
    PRAXIS_AGENT_SETTINGS,
    PRAXIS_AGENT_SYSTEM_PROMPT,
    APPLICATION_LOGS_ENABLED,
    LOG_QUERY_ROW_LIMIT,
    MCP_SERVER_ENABLED,
    MCP_SERVER_PORT,
    PROMPT_TIMEOUT_SECS,
    CLAUDE_CCRV1_ENABLED,
    CLAUDE_CCRV1_PORT,
    CLAUDE_CCRV2_ENABLED,
    CLAUDE_CCRV2_PORT,
];

/// A model definition stored in config
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelDefinition {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PraxisAgentSettings {
    #[serde(alias = "model_ref")]
    pub model_ref: String,
    #[serde(default, alias = "thinking_effort")]
    pub thinking_effort: String,
    #[serde(default)]
    pub enabled: bool,
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
    pub async fn set(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> anyhow::Result<()> {
        let key = key.into();
        let value = value.into();
        self.db.set_config(&key, &value).await?;
        self.cache.insert(key, value);
        Ok(())
    }

    /// Remove a configuration key
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
    pub fn get_orchestrator_model_def(&self) -> Option<ModelDefinition> {
        self.get(LLM_FEATURE_ORCHESTRATOR)
            .and_then(|model_ref| self.find_model_definition(model_ref))
    }

    /// Get Praxis agent settings from config.
    pub fn get_praxis_agent_settings(&self) -> Option<PraxisAgentSettings> {
        self.get(PRAXIS_AGENT_SETTINGS)
            .and_then(|json_str| serde_json::from_str(json_str).ok())
    }

    /// Get the configured Praxis agent system prompt.
    pub fn get_praxis_agent_system_prompt(&self) -> Option<String> {
        self.get(PRAXIS_AGENT_SYSTEM_PROMPT)
            .filter(|prompt| !prompt.trim().is_empty())
            .cloned()
    }

    /// Resolve Praxis agent settings to concrete endpoint/model credentials.
    pub fn resolve_praxis_agent_config(&self) -> Option<PraxisAgentConfig> {
        let settings = self.get_praxis_agent_settings()?;
        let model_def = self.find_model_definition(&settings.model_ref)?;
        let endpoint_url = model_def
            .base_url
            .clone()
            .filter(|url| !url.trim().is_empty())
            .or_else(|| {
                Provider::from_str(&model_def.provider)
                    .map(|provider| provider.base_url().to_string())
            })?
            .trim_end_matches('/')
            .to_string();

        if endpoint_url.is_empty() {
            return None;
        }

        Some(PraxisAgentConfig {
            provider: model_def.provider,
            api_key: model_def.api_key,
            endpoint_url,
            model_name: model_def.model,
            thinking_effort: if settings.thinking_effort.trim().is_empty() {
                None
            } else {
                Some(settings.thinking_effort)
            },
            system_prompt: self.get_praxis_agent_system_prompt(),
            max_tool_iterations: None,
            command_timeout_secs: None,
        })
    }

    /// Convert to a HashMap (for backwards compatibility with existing code)
    pub fn to_hashmap(&self) -> HashMap<String, String> {
        self.cache.clone()
    }

    /// Check if MCP server is enabled
    pub fn is_mcp_server_enabled(&self) -> bool {
        self.get_bool(MCP_SERVER_ENABLED, true)
    }

    /// Get the MCP server port
    pub fn get_mcp_server_port(&self) -> u16 {
        self.get(MCP_SERVER_PORT)
            .and_then(|s| s.parse().ok())
            .unwrap_or(MCP_SERVER_DEFAULT_PORT)
    }

    /// Get the prompt timeout in seconds
    pub fn get_prompt_timeout_secs(&self) -> u64 {
        self.get(PROMPT_TIMEOUT_SECS)
            .and_then(|s| s.parse().ok())
            .unwrap_or(PROMPT_TIMEOUT_SECS_DEFAULT)
    }

    pub fn is_claude_ccrv1_enabled(&self) -> bool {
        self.get_bool(CLAUDE_CCRV1_ENABLED, false)
    }

    pub fn get_claude_ccrv1_port(&self) -> u16 {
        self.get(CLAUDE_CCRV1_PORT)
            .and_then(|s| s.parse().ok())
            .unwrap_or(CLAUDE_CCRV1_DEFAULT_PORT)
    }

    pub fn is_claude_ccrv2_enabled(&self) -> bool {
        self.get_bool(CLAUDE_CCRV2_ENABLED, false)
    }

    pub fn get_claude_ccrv2_port(&self) -> u16 {
        self.get(CLAUDE_CCRV2_PORT)
            .and_then(|s| s.parse().ok())
            .unwrap_or(CLAUDE_CCRV2_DEFAULT_PORT)
    }
}
