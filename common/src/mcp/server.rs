use anyhow::Result;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::client::McpClient;
use super::params::*;
use crate::{AgentCommandResult, AgentFileType, NodeCommand, NodeCommandResult, SessionCommandResult};

const SERVER_NAME: &str = "praxis";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

//
// Generic MCP server that works with any McpClient implementation.
//

#[derive(Clone)]
pub struct PraxisServer<C: McpClient + Clone + 'static> {
    client: Arc<Mutex<Option<C>>>,
    client_factory: Arc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<C>> + Send>> + Send + Sync>,
    tool_router: ToolRouter<Self>,
}

impl<C: McpClient + Clone + 'static> PraxisServer<C> {
    pub fn new<F, Fut>(client_factory: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<C>> + Send + 'static,
    {
        let factory = Arc::new(move || {
            let fut = client_factory();
            Box::pin(fut) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<C>> + Send>>
        });

        Self {
            client: Arc::new(Mutex::new(None)),
            client_factory: factory,
            tool_router: Self::tool_router(),
        }
    }

    //
    // Create server with an already-connected client.
    //

    pub fn with_client(client: C) -> Self {
        Self {
            client: Arc::new(Mutex::new(Some(client))),
            client_factory: Arc::new(|| {
                Box::pin(async { Err(anyhow::anyhow!("No factory configured")) })
            }),
            tool_router: Self::tool_router(),
        }
    }

    async fn get_client(&self) -> Result<(), String> {
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            let client = (self.client_factory)()
                .await
                .map_err(|e| e.to_string())?;
            *guard = Some(client);
        }
        Ok(())
    }
}

include!("server/tool_router_impl.rs");

#[tool_handler]
impl<C: McpClient + Clone + 'static> ServerHandler for PraxisServer<C> {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation {
                name: SERVER_NAME.into(),
                version: SERVER_VERSION.into(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Praxis C2 framework for orchestrating AI coding agents. \
                Use node_list to see connected nodes, then agent_list to see agents on a node. \
                IMPORTANT: Always call session_close when you are done with a session to free \
                resources and allow other clients to use the agent."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

//
// Helper function to run the MCP server with stdio transport.
//

pub async fn run_stdio_server<C: McpClient + Clone + 'static>(server: PraxisServer<C>) -> Result<()> {
    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
