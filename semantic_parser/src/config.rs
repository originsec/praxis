pub use common::ai::Provider;

/// Configuration for the semantic parser
#[derive(Debug, Clone)]
pub struct ParserConfig {
    /// AI provider to use
    pub provider: Provider,
    /// API key for the provider
    pub api_key: String,
    /// Model to use for parsing
    pub model: String,
    /// Maximum retry attempts for invalid JSON (default: 3)
    pub max_retries: usize,
    /// Maximum tokens in response (default: 4096)
    pub max_tokens: Option<u32>,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            provider: Provider::Anthropic,
            api_key: String::new(),
            model: "claude-haiku-4-5-20241022".to_string(),
            max_retries: 3,
            max_tokens: Some(4096),
        }
    }
}
