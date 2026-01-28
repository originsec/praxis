use super::DynamicAgent;
use crate::agent_connectors::traits::AgentIntercept;

//
// Implement the AgentIntercept trait for Dynamic Agent.
//

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
