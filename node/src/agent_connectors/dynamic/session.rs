//
// Dynamic Agent Session - Uses OpenAI-compatible API for chat completions.
//

use crate::agent_connectors::traits::{AgentMode, AgentSession};
use anyhow::Result;
use common::ai::providers::OpenAIClient;
use common::ai::types::{ChatCompletionRequest, ChatCompletionResponse, Message};

use std::sync::Mutex;
use uuid::Uuid;

/// Session for a dynamic agent that communicates with an OpenAI-compatible API.
pub struct DynamicAgentSession {
    /// Unique session identifier
    session_id: Uuid,
    /// OpenAI-compatible API client
    client: OpenAIClient,
    /// Model to use for completions
    model: String,
    /// Conversation history
    history: Mutex<Vec<Message>>,
    /// Whether YOLO mode is enabled
    #[allow(dead_code)]
    yolo_mode: bool,
    /// Tokio runtime handle for blocking calls
    runtime: tokio::runtime::Handle,
}

impl DynamicAgentSession {
    /// Create a new dynamic agent session
    pub fn new(api_key: String, base_url: String, model: String, yolo_mode: bool) -> Self {
        common::log_debug!(
            "Creating dynamic agent session with base_url={}, model={}",
            base_url, model
        );

        let client = OpenAIClient::with_base_url(api_key, base_url);

        Self {
            session_id: Uuid::new_v4(),
            client,
            model,
            history: Mutex::new(Vec::new()),
            yolo_mode,
            runtime: tokio::runtime::Handle::current(),
        }
    }

    /// Send a chat completion request and return the response
    async fn chat(&self, prompt: &str) -> Result<String> {
        //
        // Add user message to history.
        //
        let user_message = Message::user(prompt);
        {
            let mut history = self.history.lock().unwrap();
            history.push(user_message.clone());
        }

        //
        // Build request with full history.
        //
        let messages = {
            let history = self.history.lock().unwrap();
            history.clone()
        };

        let request = ChatCompletionRequest::new(&self.model, messages);

        //
        // Send request.
        //
        let response: ChatCompletionResponse = self.client.chat_completion(request).await?;

        //
        // Extract response text.
        //
        let response_text = response
            .text()
            .unwrap_or("(no response)")
            .to_string();

        //
        // Add assistant response to history.
        //
        {
            let mut history = self.history.lock().unwrap();
            history.push(Message::assistant(&response_text));
        }

        Ok(response_text)
    }
}

impl AgentSession for DynamicAgentSession {
    fn session_id(&self) -> &Uuid {
        &self.session_id
    }

    fn mode(&self) -> AgentMode {
        AgentMode::Cli
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        //
        // Block on the async chat method.
        //
        let prompt_owned = prompt.to_string();
        let result = self.runtime.block_on(async {
            self.chat(&prompt_owned).await
        });

        match result {
            Ok(response) => Ok(response),
            Err(e) => {
                common::log_error!("Dynamic agent chat error: {}", e);
                Err(e)
            }
        }
    }

    fn close(&self) {
        //
        // Clear conversation history.
        //
        let mut history = self.history.lock().unwrap();
        history.clear();
        common::log_debug!("Dynamic agent session {} closed", self.session_id);
    }
}
