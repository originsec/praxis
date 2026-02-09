use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Message role in a conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// Message content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
}

impl Content {
    /// Get the text content if this is a Text variant
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Content::Text(t) => Some(t),
        }
    }
}

impl From<String> for Content {
    fn from(s: String) -> Self {
        Content::Text(s)
    }
}

impl From<&str> for Content {
    fn from(s: &str) -> Self {
        Content::Text(s.to_string())
    }
}

/// A message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Content,
}

impl Message {
    /// Create a new message with text content
    pub fn new(role: Role, content: impl Into<Content>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }

    /// Create a system message
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, Content::Text(content.into()))
    }

    /// Create a user message
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, Content::Text(content.into()))
    }

    /// Create an assistant message
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, Content::Text(content.into()))
    }

    /// Get the text content of this message
    pub fn text(&self) -> &str {
        match &self.content {
            Content::Text(t) => t,
        }
    }
}

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// The name of the tool
    pub name: String,
    /// Description of what the tool does
    pub description: Option<String>,
    /// JSON schema for the tool parameters
    pub parameters: Option<Value>,
}

impl Tool {
    /// Create a new tool definition
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            parameters: None,
        }
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the parameters schema
    pub fn with_parameters(mut self, parameters: Value) -> Self {
        self.parameters = Some(parameters);
        self
    }
}

/// Chat completion request
#[derive(Debug, Clone)]
pub struct ChatCompletionRequest {
    /// Model name to use
    pub model: String,
    /// Conversation messages
    pub messages: Vec<Message>,
    /// Maximum tokens to generate
    pub max_tokens: Option<u32>,
    /// Temperature (0.0 to 2.0)
    pub temperature: Option<f32>,
    /// Top-p sampling
    pub top_p: Option<f32>,
}

impl ChatCompletionRequest {
    /// Create a new chat completion request
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            max_tokens: None,
            temperature: None,
            top_p: None,
        }
    }

    /// Set max tokens
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set temperature
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }
}

/// A choice in a chat completion response
#[derive(Debug, Clone)]
pub struct ChatCompletionChoice {
    /// Index of this choice
    pub index: usize,
    /// The message content
    pub message: Message,
    /// Finish reason (stop, length, etc.)
    pub finish_reason: Option<String>,
}

/// Chat completion response
#[derive(Debug, Clone)]
pub struct ChatCompletionResponse {
    /// Unique response ID
    pub id: String,
    /// Model used
    pub model: String,
    /// Response choices
    pub choices: Vec<ChatCompletionChoice>,
    /// Token usage statistics
    pub usage: Option<Usage>,
}

impl ChatCompletionResponse {
    /// Get the text content of the first choice
    pub fn text(&self) -> Option<&str> {
        self.choices.first().map(|c| c.message.text())
    }
}

/// Token usage statistics
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Response from AI that may contain a tool call or be a final response
#[derive(Debug)]
pub enum AiResponse {
    /// AI wants to call a tool
    ToolCall {
        /// Name of the tool to call
        tool_name: String,
        /// Arguments for the tool (as JSON)
        tool_args: Value,
        /// Explanatory text from the response (tool call block removed)
        response_text: String,
    },
    /// AI provided a final response with no tool calls
    FinalResponse {
        /// The response text
        text: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_constructors() {
        let sys = Message::system("You are helpful");
        assert_eq!(sys.role, Role::System);
        assert_eq!(sys.text(), "You are helpful");

        let user = Message::user("Hello");
        assert_eq!(user.role, Role::User);
        assert_eq!(user.text(), "Hello");

        let asst = Message::assistant("Hi there!");
        assert_eq!(asst.role, Role::Assistant);
        assert_eq!(asst.text(), "Hi there!");
    }

    #[test]
    fn test_tool_builder() {
        let tool = Tool::new("test_tool")
            .with_description("A test tool")
            .with_parameters(serde_json::json!({"type": "object"}));

        assert_eq!(tool.name, "test_tool");
        assert_eq!(tool.description, Some("A test tool".to_string()));
        assert!(tool.parameters.is_some());
    }

    #[test]
    fn test_request_builder() {
        let request = ChatCompletionRequest::new("gpt-4", vec![])
            .with_max_tokens(1000)
            .with_temperature(0.7);

        assert_eq!(request.model, "gpt-4");
        assert_eq!(request.max_tokens, Some(1000));
        assert_eq!(request.temperature, Some(0.7));
    }
}
