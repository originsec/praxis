use std::pin::Pin;

use anyhow::Result;
use futures_core::Stream;

use super::parsing::parse_manual_tool_call;
use super::provider::Provider;
use super::providers::{AnthropicClient, GeminiClient, OpenAIClient};
use super::types::{
    AiResponse, ChatCompletionDelta, ChatCompletionRequest, ChatCompletionResponse, Content,
    Message, Role,
};

/// Unified AI client that wraps provider-specific implementations
pub enum AiClient {
    Anthropic(AnthropicClient),
    OpenAI(OpenAIClient),
    Gemini(GeminiClient),
}

impl AiClient {
    /// Send a chat completion request
    pub async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        match self {
            AiClient::Anthropic(client) => client.chat_completion(request).await,
            AiClient::OpenAI(client) => client.chat_completion(request).await,
            AiClient::Gemini(client) => client.chat_completion(request).await,
        }
    }

    /// Send a streaming chat completion request.
    ///
    /// Returns a stream of delta chunks. Currently supported for
    /// OpenAI-compatible providers only. Falls back to a single-chunk
    /// non-streaming response for unsupported providers.
    pub fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<ChatCompletionDelta>> + Send + '_>> {
        match self {
            AiClient::OpenAI(client) => client.chat_completion_stream(request),
            _ => {
                //
                // Fallback: execute non-streaming and emit as single chunk.
                //
                Box::pin(async_stream::try_stream! {
                    let response = match self {
                        AiClient::Anthropic(client) => client.chat_completion(request).await,
                        AiClient::Gemini(client) => client.chat_completion(request).await,
                        AiClient::OpenAI(_) => unreachable!(),
                    }?;

                    let text = response.text().unwrap_or_default().to_string();
                    let finish_reason = response.choices.first()
                        .and_then(|c| c.finish_reason.clone());

                    yield ChatCompletionDelta {
                        content: text,
                        finish_reason,
                        usage: response.usage,
                    };
                })
            }
        }
    }
}

/// Create an AI client with the given configuration.
///
/// The optional `base_url` overrides the provider's default endpoint. This is
/// used for local model servers (Ollama, vLLM, llama.cpp, LM Studio) and
/// custom OpenAI-compatible endpoints.
pub fn create_ai_client(
    provider: Provider,
    api_key: String,
    base_url: Option<&str>,
) -> Result<AiClient> {
    match provider {
        Provider::Anthropic => Ok(AiClient::Anthropic(AnthropicClient::new(api_key))),
        Provider::Gemini => Ok(AiClient::Gemini(GeminiClient::new(api_key))),

        //
        // Custom provider requires a base URL.
        //
        Provider::Custom => {
            let url = base_url
                .filter(|u| !u.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Custom provider requires a base URL"))?;
            Ok(AiClient::OpenAI(OpenAIClient::with_base_url(
                api_key,
                url.to_string(),
            )))
        }

        //
        // All other OpenAI-compatible providers (including Ollama).
        // Use custom base_url if provided, otherwise fall back to the
        // provider's default.
        //
        other => {
            let url = base_url
                .filter(|u| !u.is_empty())
                .map(|u| u.to_string())
                .unwrap_or_else(|| other.base_url().to_string());

            if other == Provider::OpenAI && base_url.is_none() {
                Ok(AiClient::OpenAI(OpenAIClient::new(api_key)))
            } else {
                Ok(AiClient::OpenAI(OpenAIClient::with_base_url(api_key, url)))
            }
        }
    }
}

/// Execute a chat completion request with the AI client
///
/// This is a simple wrapper that returns the text content of the first choice, or an error.
///
/// # Arguments
///
/// * `client` - The AI client to use
/// * `model` - Model name to use
/// * `conversation_history` - Vector of Message objects representing the conversation
/// * `max_tokens` - Optional maximum tokens to generate
///
/// # Returns
///
/// Result containing the response text or an error
pub async fn execute_chat_completion(
    client: &AiClient,
    model: String,
    conversation_history: Vec<Message>,
    max_tokens: Option<u32>,
) -> Result<String> {
    let mut request = ChatCompletionRequest::new(model, conversation_history);
    request.max_tokens = max_tokens;

    let response = client.chat_completion(request).await?;

    let choice = response
        .choices
        .first()
        .ok_or_else(|| anyhow::anyhow!("No response choices returned"))?;

    Ok(choice.message.text().to_string())
}

/// Build a Message for the conversation history
///
/// # Arguments
///
/// * `role` - The role (System, User, Assistant)
/// * `content` - The text content of the message
///
/// # Returns
///
/// A Message object ready to be added to conversation history
pub fn build_message(role: Role, content: String) -> Message {
    Message {
        role,
        content: Content::Text(content),
    }
}

/// Execute a chat completion and automatically parse for tool calls
///
/// This is a convenience function that combines `execute_chat_completion` with
/// automatic tool call parsing. It returns an AiResponse enum that indicates
/// whether the AI wants to call a tool or provided a final response.
///
/// # Arguments
///
/// * `client` - The AI client to use
/// * `model` - Model name to use
/// * `conversation_history` - Vector of Message objects representing the conversation
/// * `max_tokens` - Optional maximum tokens to generate
///
/// # Returns
///
/// Result containing an AiResponse (either ToolCall or FinalResponse)
///
/// # Examples
///
/// ```no_run
/// use common::ai::{Provider, create_ai_client, execute_with_tool_parsing, build_message, AiResponse, Role};
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = create_ai_client(Provider::Anthropic, "sk-...".to_string(), None)?;
/// let messages = vec![
///     build_message(Role::User, "Hello!".to_string())
/// ];
///
/// match execute_with_tool_parsing(&client, "claude-haiku-4-5".to_string(), messages, None).await? {
///     AiResponse::ToolCall { tool_name, tool_args, response_text } => {
///         println!("AI wants to call tool: {}", tool_name);
///     }
///     AiResponse::FinalResponse { text } => {
///         println!("AI responded: {}", text);
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub async fn execute_with_tool_parsing(
    client: &AiClient,
    model: String,
    conversation_history: Vec<Message>,
    max_tokens: Option<u32>,
) -> Result<AiResponse> {
    let text_content =
        execute_chat_completion(client, model, conversation_history, max_tokens).await?;

    //
    // Try to parse a tool call from the response.
    //
    if let Some((tool_name, tool_args, remaining_text)) = parse_manual_tool_call(&text_content) {
        Ok(AiResponse::ToolCall {
            tool_name,
            tool_args,
            response_text: remaining_text,
        })
    } else {
        Ok(AiResponse::FinalResponse { text: text_content })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_client_anthropic() {
        let client = create_ai_client(Provider::Anthropic, "test-key".to_string(), None);
        assert!(client.is_ok());
        assert!(matches!(client.unwrap(), AiClient::Anthropic(_)));
    }

    #[test]
    fn test_create_client_openai() {
        let client = create_ai_client(Provider::OpenAI, "test-key".to_string(), None);
        assert!(client.is_ok());
        assert!(matches!(client.unwrap(), AiClient::OpenAI(_)));
    }

    #[test]
    fn test_create_client_groq() {
        let client = create_ai_client(Provider::Groq, "test-key".to_string(), None);
        assert!(client.is_ok());
        assert!(matches!(client.unwrap(), AiClient::OpenAI(_)));
    }

    #[test]
    fn test_create_client_gemini() {
        let client = create_ai_client(Provider::Gemini, "test-key".to_string(), None);
        assert!(client.is_ok());
        assert!(matches!(client.unwrap(), AiClient::Gemini(_)));
    }

    #[test]
    fn test_create_client_ollama() {
        let client = create_ai_client(Provider::Ollama, String::new(), None);
        assert!(client.is_ok());
        assert!(matches!(client.unwrap(), AiClient::OpenAI(_)));
    }

    #[test]
    fn test_create_client_ollama_custom_url() {
        let client = create_ai_client(
            Provider::Ollama,
            String::new(),
            Some("http://myserver:11434/v1"),
        );
        assert!(client.is_ok());
        assert!(matches!(client.unwrap(), AiClient::OpenAI(_)));
    }

    #[test]
    fn test_create_client_custom_requires_url() {
        let client = create_ai_client(Provider::Custom, String::new(), None);
        assert!(client.is_err());
    }

    #[test]
    fn test_create_client_custom_with_url() {
        let client = create_ai_client(
            Provider::Custom,
            String::new(),
            Some("http://localhost:8000/v1"),
        );
        assert!(client.is_ok());
        assert!(matches!(client.unwrap(), AiClient::OpenAI(_)));
    }

    #[test]
    fn test_build_message() {
        let msg = build_message(Role::User, "Hello".to_string());
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text(), "Hello");
    }
}
