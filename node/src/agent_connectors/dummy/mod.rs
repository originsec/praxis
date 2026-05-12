mod fingerprint;
mod session;

pub use session::DummySession;

use crate::agent_connectors::traits::{Agent, AgentRecon, AgentSession};
use async_trait::async_trait;
use common::{
    AgentTool, ConfigItem, McpServer, McpTransport, ReconConfig, ReconResult, ReconSessions,
    ReconTools,
};
use std::sync::Arc;
use uuid::Uuid;

const AGENT_NAME: &str = "Dummy Agent";
const AGENT_SHORTNAME: &str = "dummy";

/// A dummy agent that doesn't require any external processes.
/// Useful for testing and validating agent abstractions.
#[allow(dead_code)]
pub struct DummyAgent;

#[allow(dead_code)]
impl DummyAgent {
    pub fn new() -> Self {
        Self
    }

    /// Generate demo MCP servers with tools
    fn get_demo_mcp_servers(&self) -> Vec<McpServer> {
        vec![
            McpServer {
                name: "FileSystem Server".to_string(),
                transport: McpTransport::Stdio,
                address: None,
                command: Some("npx @modelcontextprotocol/server-filesystem".to_string()),
                tools: vec![
                    AgentTool {
                        name: "fs_read".to_string(),
                        description: "Read file contents from disk".to_string(),
                        ..Default::default()
                    },
                    AgentTool {
                        name: "fs_write".to_string(),
                        description: "Write content to files".to_string(),
                        ..Default::default()
                    },
                    AgentTool {
                        name: "fs_list".to_string(),
                        description: "List directory contents".to_string(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            McpServer {
                name: "GitHub Server".to_string(),
                transport: McpTransport::Sse,
                address: Some("https://mcp.github.io/api".to_string()),
                command: None,
                tools: vec![
                    AgentTool {
                        name: "github_repos".to_string(),
                        description: "List and access repositories".to_string(),
                        ..Default::default()
                    },
                    AgentTool {
                        name: "github_issues".to_string(),
                        description: "Manage issues and pull requests".to_string(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            McpServer {
                name: "Database Server".to_string(),
                transport: McpTransport::Stdio,
                address: None,
                command: Some("npx @modelcontextprotocol/server-postgres".to_string()),
                tools: vec![AgentTool {
                    name: "db_query".to_string(),
                    description: "Execute SQL queries".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ]
    }

    /// Generate demo skills
    fn get_demo_skills(&self) -> Vec<AgentTool> {
        vec![
            AgentTool {
                name: "/commit".to_string(),
                description: "Create a git commit with staged changes".to_string(),
                ..Default::default()
            },
            AgentTool {
                name: "/review-pr".to_string(),
                description: "Review a pull request for issues".to_string(),
                ..Default::default()
            },
            AgentTool {
                name: "/test".to_string(),
                description: "Run tests and report results".to_string(),
                ..Default::default()
            },
            AgentTool {
                name: "/refactor".to_string(),
                description: "Refactor code to improve quality".to_string(),
                ..Default::default()
            },
        ]
    }

    /// Generate demo config items
    fn get_demo_config(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem {
                path: "~/.dummy/settings.json".to_string(),
                contents: None, // Contents fetched on-demand
                config_type: "settings".to_string(),
            },
            ConfigItem {
                path: "~/.dummy/CLAUDE.md".to_string(),
                contents: None, // Contents fetched on-demand
                config_type: "instructions".to_string(),
            },
            ConfigItem {
                path: "~/project/.dummy/local.json".to_string(),
                contents: None, // Contents fetched on-demand
                config_type: "project".to_string(),
            },
        ]
    }
}

impl Default for DummyAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for DummyAgent {
    fn name(&self) -> &str {
        AGENT_NAME
    }

    fn short_name(&self) -> &str {
        AGENT_SHORTNAME
    }

    fn as_recon(&self) -> Option<&dyn AgentRecon> {
        Some(self)
    }

    async fn do_fingerprint(&self) -> bool {
        self.do_fingerprint_impl().await
    }

    fn create_session_with_id(
        &self,
        _context: &common::SessionContext,
        _session_id: Uuid,
    ) -> Option<Arc<dyn AgentSession>> {
        Some(Arc::new(DummySession::new()) as Arc<dyn AgentSession>)
    }
}

#[async_trait]
impl AgentRecon for DummyAgent {
    async fn perform_recon(&self) -> Option<ReconResult> {
        let tools = ReconTools {
            mcp_servers: self.get_demo_mcp_servers(),
            skills: self.get_demo_skills(),
        };

        let config = ReconConfig {
            items: self.get_demo_config(),
            project_paths: Vec::new(),
        };

        common::log_info!(
            "Recon complete - {} MCP servers, {} skills, {} config items",
            tools.mcp_servers.len(),
            tools.skills.len(),
            config.items.len()
        );

        Some(ReconResult {
            tools,
            config,
            sessions: ReconSessions::default(),
        })
    }
}
