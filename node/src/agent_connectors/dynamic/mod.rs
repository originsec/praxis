//
// Dynamic Agent - An agent created from a discovered OpenAI-compatible endpoint.
//

mod session;

pub use session::DynamicAgentSession;

use crate::agent_connectors::traits::{Agent, AgentIntercept, AgentSession};
use anyhow::Result;
use async_trait::async_trait;
use common::{DiscoveredLlmEndpoint, SessionContext};
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// YOLO mode flag
    yolo_mode: AtomicBool,
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
            yolo_mode: AtomicBool::new(false),
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

    fn supports_intercept(&self) -> bool {
        //
        // Dynamic agents support interception for their endpoint domain.
        //
        self.endpoint.domain.is_some()
    }

    fn as_intercept(&self) -> Option<&dyn AgentIntercept> {
        if self.supports_intercept() {
            Some(self)
        } else {
            None
        }
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

        let session: Arc<dyn AgentSession> = Arc::new(DynamicAgentSession::new(
            api_key,
            self.endpoint.base_url.clone(),
            model,
            context.yolo_mode,
        ));

        let mut guard = self.session.write().unwrap();
        *guard = Some(session.clone());
        Some(session)
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

    fn set_yolo_mode(&self, enabled: bool) -> Result<()> {
        self.yolo_mode.store(enabled, Ordering::SeqCst);
        Ok(())
    }

    fn is_yolo_mode(&self) -> bool {
        self.yolo_mode.load(Ordering::SeqCst)
    }
}

impl AgentIntercept for DynamicAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        //
        // Intercept traffic to the endpoint domain if available.
        //
        match &self.endpoint.domain {
            Some(domain) => vec![domain.as_str()],
            None => vec![],
        }
    }

    fn intercept_url_pattern(&self) -> Option<&str> {
        //
        // Intercept all traffic to /v1/ endpoints.
        //
        Some("/v1/")
    }
}
