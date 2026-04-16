use serde::{Deserialize, Serialize};

/// Supported AI providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Groq,
    OpenAI,
    Anthropic,
    Mistral,
    XAI,
    Gemini,
    Cerebras,
    Nvidia,
    MiniMax,
    Moonshot,
    FireworksAI,
    OpenRouter,
    Ollama,
    Custom,
}

impl Provider {
    /// Get the string representation of the provider (lowercase)
    pub fn as_str(&self) -> &str {
        match self {
            Provider::Groq => "groq",
            Provider::OpenAI => "openai",
            Provider::Anthropic => "anthropic",
            Provider::Mistral => "mistral",
            Provider::XAI => "xai",
            Provider::Gemini => "gemini",
            Provider::Cerebras => "cerebras",
            Provider::Nvidia => "nvidia",
            Provider::MiniMax => "minimax",
            Provider::Moonshot => "moonshot",
            Provider::FireworksAI => "fireworksai",
            Provider::OpenRouter => "openrouter",
            Provider::Ollama => "ollama",
            Provider::Custom => "custom",
        }
    }

    /// Parse a provider from a string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "groq" => Some(Provider::Groq),
            "openai" => Some(Provider::OpenAI),
            "anthropic" => Some(Provider::Anthropic),
            "mistral" => Some(Provider::Mistral),
            "xai" => Some(Provider::XAI),
            "gemini" => Some(Provider::Gemini),
            "cerebras" => Some(Provider::Cerebras),
            "nvidia" => Some(Provider::Nvidia),
            "minimax" => Some(Provider::MiniMax),
            "moonshot" => Some(Provider::Moonshot),
            "fireworksai" | "fireworks" => Some(Provider::FireworksAI),
            "openrouter" => Some(Provider::OpenRouter),
            "ollama" => Some(Provider::Ollama),
            "custom" => Some(Provider::Custom),
            _ => None,
        }
    }

    /// Get all available providers
    pub fn all() -> Vec<Provider> {
        vec![
            Provider::Groq,
            Provider::OpenAI,
            Provider::Anthropic,
            Provider::Mistral,
            Provider::XAI,
            Provider::Gemini,
            Provider::Cerebras,
            Provider::Nvidia,
            Provider::MiniMax,
            Provider::Moonshot,
            Provider::FireworksAI,
            Provider::OpenRouter,
            Provider::Ollama,
            Provider::Custom,
        ]
    }

    /// Get the display name for the provider (user-friendly)
    pub fn display_name(&self) -> &str {
        match self {
            Provider::Groq => "Groq",
            Provider::OpenAI => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::Mistral => "Mistral",
            Provider::XAI => "xAI",
            Provider::Gemini => "Google Gemini",
            Provider::Cerebras => "Cerebras",
            Provider::Nvidia => "NVIDIA",
            Provider::MiniMax => "MiniMax",
            Provider::Moonshot => "Moonshot AI",
            Provider::FireworksAI => "Fireworks AI",
            Provider::OpenRouter => "OpenRouter",
            Provider::Ollama => "Ollama (Local)",
            Provider::Custom => "Custom (OpenAI-Compatible)",
        }
    }

    /// Get the base URL for the provider's API
    pub fn base_url(&self) -> &'static str {
        match self {
            Provider::Groq => "https://api.groq.com/openai/v1",
            Provider::OpenAI => "https://api.openai.com/v1",
            Provider::Anthropic => "https://api.anthropic.com/v1",
            Provider::Mistral => "https://api.mistral.ai/v1",
            Provider::XAI => "https://api.x.ai/v1",
            Provider::Gemini => "https://generativelanguage.googleapis.com/v1beta",
            Provider::Cerebras => "https://api.cerebras.ai/v1",
            Provider::Nvidia => "https://integrate.api.nvidia.com/v1",
            Provider::MiniMax => "https://api.minimax.io/v1",
            Provider::Moonshot => "https://api.moonshot.ai/v1",
            Provider::FireworksAI => "https://api.fireworks.ai/inference/v1",
            Provider::OpenRouter => "https://openrouter.ai/api/v1",
            Provider::Ollama => "http://localhost:11434/v1",
            Provider::Custom => "",
        }
    }

    /// Check if this provider uses an OpenAI-compatible API
    pub fn is_openai_compatible(&self) -> bool {
        matches!(
            self,
            Provider::OpenAI
                | Provider::Groq
                | Provider::Mistral
                | Provider::XAI
                | Provider::Cerebras
                | Provider::Nvidia
                | Provider::MiniMax
                | Provider::Moonshot
                | Provider::FireworksAI
                | Provider::OpenRouter
                | Provider::Ollama
                | Provider::Custom
        )
    }

    /// Whether this provider requires a base URL to be specified by the user
    pub fn requires_base_url(&self) -> bool {
        matches!(self, Provider::Custom)
    }

    /// Whether the API key is optional for this provider
    pub fn api_key_optional(&self) -> bool {
        matches!(self, Provider::Ollama | Provider::Custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_roundtrip() {
        for provider in Provider::all() {
            let s = provider.as_str();
            let parsed = Provider::from_str(s).unwrap();
            assert_eq!(provider, parsed);
        }
    }

    #[test]
    fn test_provider_case_insensitive() {
        assert_eq!(Provider::from_str("ANTHROPIC"), Some(Provider::Anthropic));
        assert_eq!(Provider::from_str("OpenAI"), Some(Provider::OpenAI));
        assert_eq!(Provider::from_str("GRoq"), Some(Provider::Groq));
    }

    #[test]
    fn test_provider_invalid() {
        assert_eq!(Provider::from_str("invalid"), None);
        assert_eq!(Provider::from_str(""), None);
    }

    #[test]
    fn test_openai_compatible() {
        assert!(Provider::OpenAI.is_openai_compatible());
        assert!(Provider::Groq.is_openai_compatible());
        assert!(Provider::Mistral.is_openai_compatible());
        assert!(!Provider::Anthropic.is_openai_compatible());
        assert!(!Provider::Gemini.is_openai_compatible());
    }
}
