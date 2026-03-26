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

/// Create an AI client with the given configuration
///
/// # Arguments
///
/// * `provider` - The AI provider to use
/// * `api_key` - API key for the provider
///
/// # Returns
///
/// Result containing the configured AiClient or an error
///
/// # Examples
///
/// ```no_run
/// use common::ai::{Provider, create_ai_client};
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = create_ai_client(Provider::Anthropic, "sk-...".to_string())?;
/// # Ok(())
/// # }
/// ```
pub fn create_ai_client(provider: Provider, api_key: String) -> Result<AiClient> {
    match provider {
        Provider::Anthropic => Ok(AiClient::Anthropic(AnthropicClient::new(api_key))),
        Provider::OpenAI => Ok(AiClient::OpenAI(OpenAIClient::new(api_key))),
        Provider::Gemini => Ok(AiClient::Gemini(GeminiClient::new(api_key))),
        //
        // OpenAI-compatible providers.
        //
        Provider::Groq => Ok(AiClient::OpenAI(OpenAIClient::with_base_url(
            api_key,
            "https://api.groq.com/openai/v1".to_string(),
        ))),
        Provider::Mistral => Ok(AiClient::OpenAI(OpenAIClient::with_base_url(
            api_key,
            "https://api.mistral.ai/v1".to_string(),
        ))),
        Provider::XAI => Ok(AiClient::OpenAI(OpenAIClient::with_base_url(
            api_key,
            "https://api.x.ai/v1".to_string(),
        ))),
        Provider::Cerebras => Ok(AiClient::OpenAI(OpenAIClient::with_base_url(
            api_key,
            "https://api.cerebras.ai/v1".to_string(),
        ))),
        Provider::Nvidia => Ok(AiClient::OpenAI(OpenAIClient::with_base_url(
            api_key,
            "https://integrate.api.nvidia.com/v1".to_string(),
        ))),
        Provider::MiniMax => Ok(AiClient::OpenAI(OpenAIClient::with_base_url(
            api_key,
            "https://api.minimax.io/v1".to_string(),
        ))),
        Provider::Moonshot => Ok(AiClient::OpenAI(OpenAIClient::with_base_url(
            api_key,
            "https://api.moonshot.ai/v1".to_string(),
        ))),
        Provider::FireworksAI => Ok(AiClient::OpenAI(OpenAIClient::with_base_url(
            api_key,
            "https://api.fireworks.ai/inference/v1".to_string(),
        ))),
        Provider::OpenRouter => Ok(AiClient::OpenAI(OpenAIClient::with_base_url(
            api_key,
            "https://openrouter.ai/api/v1".to_string(),
        ))),
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
/// let client = create_ai_client(Provider::Anthropic, "sk-...".to_string())?;
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
        let client = create_ai_client(Provider::Anthropic, "test-key".to_string());
        assert!(client.is_ok());
        assert!(matches!(client.unwrap(), AiClient::Anthropic(_)));
    }

    #[test]
    fn test_create_client_openai() {
        let client = create_ai_client(Provider::OpenAI, "test-key".to_string());
        assert!(client.is_ok());
        assert!(matches!(client.unwrap(), AiClient::OpenAI(_)));
    }

    #[test]
    fn test_create_client_groq() {
        let client = create_ai_client(Provider::Groq, "test-key".to_string());
        assert!(client.is_ok());
        //
        // Groq uses OpenAI-compatible.
        //
        assert!(matches!(client.unwrap(), AiClient::OpenAI(_)));
    }

    #[test]
    fn test_create_client_gemini() {
        let client = create_ai_client(Provider::Gemini, "test-key".to_string());
        assert!(client.is_ok());
        assert!(matches!(client.unwrap(), AiClient::Gemini(_)));
    }

    #[test]
    fn test_build_message() {
        let msg = build_message(Role::User, "Hello".to_string());
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text(), "Hello");
    }
}
