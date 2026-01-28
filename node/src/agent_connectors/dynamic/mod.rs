//
// Dynamic Agent - An agent created from a discovered OpenAI-compatible endpoint.
//

mod intercept;
mod recon;
mod session;

pub use session::DynamicAgentSession;

use crate::agent_connectors::traits::{Agent, AgentIntercept, AgentRecon, AgentSession};
use async_trait::async_trait;
use common::{DiscoveredLlmEndpoint, SessionContext};
use std::sync::{Arc, RwLock};

/// A dynamic agent created from a discovered OpenAI-compatible LLM endpoint.
pub struct DynamicAgent {
    /// Display name for this agent
    name: String,
    /// Short identifier (used for selection)
    short_name: String,
    /// The discovered endpoint information
    endpoint: DiscoveredLlmEndpoint,
    /// Current session
    session: RwLock<Option<Arc<dyn AgentSession>>>,
}

impl DynamicAgent {
    /// Create a new dynamic agent from a discovered endpoint
    pub fn new(name: String, short_name: String, endpoint: DiscoveredLlmEndpoint) -> Self {
        common::log_info!(
            "Creating dynamic agent '{}' ({}) from endpoint {}",
            name, short_name, endpoint.base_url
        );
        Self {
            name,
            short_name,
            endpoint,
            session: RwLock::new(None),
        }
    }
}

#[async_trait]
impl Agent for DynamicAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn short_name(&self) -> &str {
        &self.short_name
    }

    fn as_intercept(&self) -> Option<&dyn AgentIntercept> {
        //
        // Dynamic agents support interception for their endpoint domain.
        //
        if self.endpoint.domain.is_some() {
            Some(self)
        } else {
            None
        }
    }

    fn as_recon(&self) -> Option<&dyn AgentRecon> {
        Some(self)
    }

    async fn do_fingerprint(&self) -> bool {
        //
        // Dynamic agents are always "available" since they were created from a
        // discovered endpoint.
        //
        true
    }

    fn create_session(&self, context: &SessionContext) -> Option<Arc<dyn AgentSession>> {
        //
        // Check if we have an API key for this endpoint.
        //

        let api_key = match &self.endpoint.api_key {
            Some(key) => key.clone(),
            None => {
                common::log_info!(
                    "Cannot create session for dynamic agent '{}': no API key available",
                    self.name
                );
                return None;
            }
        };

        //
        // Get the first available model, or use a default.
        //

        let model = self
            .endpoint
            .models
            .first()
            .cloned()
            .unwrap_or_else(|| "gpt-3.5-turbo".to_string());

        let session_arc = Arc::new(DynamicAgentSession::new(
            api_key,
            self.endpoint.base_url.clone(),
            model,
            context.yolo_mode,
        )) as Arc<dyn AgentSession>;

        *self.session.write().unwrap() = Some(Arc::clone(&session_arc));
        Some(session_arc)
    }

    fn get_session(&self) -> Option<Arc<dyn AgentSession>> {
        self.session.read().unwrap().clone()
    }

    fn close_session(&self) {
        let mut guard = self.session.write().unwrap();
        if let Some(session) = guard.as_ref() {
            session.close();
        }
        *guard = None;
    }
}
