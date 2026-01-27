use anyhow::Result;
use common::FileConfig;

/// Service configuration wrapper around FileConfig
pub struct ServiceConfig {
    inner: FileConfig,
}

//
// Semantic parser config keys.
//
pub const SEMANTIC_PARSER_API_KEY: &str = "semantic_parser_api_key";
pub const SEMANTIC_PARSER_PROVIDER: &str = "semantic_parser_provider";
pub const SEMANTIC_PARSER_MODEL: &str = "semantic_parser_model";

//
// Semantic ops config keys (for running semantic operations).
//
pub const SEMANTIC_OP_API_KEY: &str = "semantic_op_api_key";
pub const SEMANTIC_OP_PROVIDER: &str = "semantic_op_provider";
pub const SEMANTIC_OP_MODEL: &str = "semantic_op_model";
pub const SEMANTIC_OP_SYSTEM_PROMPT: &str = "semantic_op_system_prompt";

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
pub const LLM_FEATURE_NEXUS: &str = "llm_feature_nexus";

/// A model definition stored in config
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDefinition {
    //
    // provider::model format.
    //
    pub name: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
}

#[allow(dead_code)]
impl ServiceConfig {
    /// Load service configuration from ~/.praxis_srv_cfg
    pub fn load() -> Result<Self> {
        let config_path = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?
            .join(".praxis_srv_cfg");

        let mut inner = FileConfig::new(config_path);
        inner.load()?;
        Ok(Self { inner })
    }

    /// Get semantic parser API key
    pub fn semantic_parser_api_key(&self) -> Option<&String> {
        self.inner.get(SEMANTIC_PARSER_API_KEY)
    }

    /// Get semantic parser provider (defaults to "anthropic")
    pub fn semantic_parser_provider(&self) -> String {
        self.inner.get(SEMANTIC_PARSER_PROVIDER)
            .cloned()
            .unwrap_or_else(|| "anthropic".to_string())
    }

    /// Get semantic parser model (defaults to "claude-haiku-4-5-20241022")
    pub fn semantic_parser_model(&self) -> String {
        self.inner.get(SEMANTIC_PARSER_MODEL)
            .cloned()
            .unwrap_or_else(|| "claude-haiku-4-5-20241022".to_string())
    }

    /// Get semantic ops API key
    pub fn semantic_op_api_key(&self) -> Option<&String> {
        self.inner.get(SEMANTIC_OP_API_KEY)
    }

    /// Get semantic ops provider (defaults to "anthropic")
    pub fn semantic_op_provider(&self) -> String {
        self.inner.get(SEMANTIC_OP_PROVIDER)
            .cloned()
            .unwrap_or_else(|| "anthropic".to_string())
    }

    /// Get semantic ops model (defaults to "claude-haiku-4-5")
    pub fn semantic_op_model(&self) -> String {
        self.inner.get(SEMANTIC_OP_MODEL)
            .cloned()
            .unwrap_or_else(|| "claude-haiku-4-5".to_string())
    }

    /// Get semantic ops system prompt (optional)
    pub fn semantic_op_system_prompt(&self) -> Option<&String> {
        self.inner.get(SEMANTIC_OP_SYSTEM_PROMPT)
    }

    /// Get LLM model definitions from config
    pub fn get_model_definitions(&self) -> Vec<ModelDefinition> {
        if let Some(json_str) = self.inner.get(LLM_MODEL_DEFINITIONS) {
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
        self.inner.get(LLM_FEATURE_SEMANTIC_PARSER)
            .and_then(|model_ref| self.find_model_definition(model_ref))
    }

    /// Get the model definition assigned to the traffic parser feature
    pub fn get_traffic_parser_model_def(&self) -> Option<ModelDefinition> {
        self.inner.get(LLM_FEATURE_TRAFFIC_PARSER)
            .and_then(|model_ref| self.find_model_definition(model_ref))
    }

    /// Get the model definition assigned to semantic ops feature
    pub fn get_semantic_ops_model_def(&self) -> Option<ModelDefinition> {
        self.inner.get(LLM_FEATURE_SEMANTIC_OPS)
            .and_then(|model_ref| self.find_model_definition(model_ref))
    }

    /// Get the model definition assigned to nexus feature
    pub fn get_nexus_model_def(&self) -> Option<ModelDefinition> {
        self.inner.get(LLM_FEATURE_NEXUS)
            .and_then(|model_ref| self.find_model_definition(model_ref))
    }

    /// Save the configuration
    pub fn save(&self) -> Result<()> {
        self.inner.save()?;
        Ok(())
    }

    /// Get a configuration value by key
    pub fn get(&self, key: &str) -> Option<&String> {
        self.inner.get(key)
    }

    /// Set a configuration value
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.inner.set(key, value);
    }

    /// Remove a configuration key
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.inner.remove(key)
    }

    /// Convert to a HashMap (for backwards compatibility with existing code)
    pub fn to_hashmap(&self) -> std::collections::HashMap<String, String> {
        self.inner.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}

/// Helper function for backwards compatibility - set a config value and save
#[allow(dead_code)]
pub fn set_config_value(key: &str, value: &str) -> Result<()> {
    let mut config = ServiceConfig::load()?;
    config.set(key, value);
    config.save()?;
    Ok(())
}

/// Helper function for backwards compatibility - load config as HashMap
#[allow(dead_code)]
pub fn load_config() -> Result<std::collections::HashMap<String, String>> {
    let config = ServiceConfig::load()?;
    Ok(config.to_hashmap())
}
