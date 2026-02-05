use super::CursorAgent;
use crate::agent_connectors::traits::AgentIntercept;

impl AgentIntercept for CursorAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        vec!["api.cursor.sh","agent.api5.cursor.sh","api2.cursor.sh","cursor.sh"]
    }
}
