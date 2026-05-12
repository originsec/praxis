use anyhow::{Result, anyhow};
use common::{
    AgentTool, McpServer, McpTransport, NODE_SIGNAL_QUEUE, NodeSignalMessage,
    SemanticParserRequest, SemanticParserResponse, publish_json,
};
use lapin::Channel;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::oneshot;
use uuid::Uuid;

//
// MCP Discovery Utilities.
//

/// JSON schema for combined MCP server and tools discovery via semantic parser.
/// This schema extracts both servers AND their tools in a single pass.
#[allow(dead_code)]
pub const MCP_SERVERS_AND_TOOLS_SCHEMA: &str = r#"{
    "type": "object",
    "properties": {
        "mcp_servers": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the MCP server" },
                    "transport": { "type": "string", "enum": ["stdio", "sse", "websocket"], "description": "Transport type" },
                    "address": { "type": "string", "description": "URL/address for network transports (optional)" },
                    "command": { "type": "string", "description": "Command for stdio transport (optional)" },
                    "tools": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Name of the tool" },
                                "description": { "type": "string", "description": "What the tool does" }
                            },
                            "required": ["name", "description"]
                        }
                    }
                },
                "required": ["name", "transport"]
            }
        }
    },
    "required": ["mcp_servers"]
}"#;

/// Discovery prompt for extracting both MCP servers and their tools.
#[allow(dead_code)]
pub const MCP_SERVERS_AND_TOOLS_PROMPT: &str = "Extract all MCP (Model Context Protocol) servers AND their tools from the following text. \
For each server, identify the name, transport type (stdio, sse, or websocket), \
address (for network transports), command (for stdio transport), and all tools provided by that server. \
For each tool, include the name and description. \
DO NOT LIST ANY SERVERS OR TOOLS THAT DO NOT EXIST IN THE TEXT. DO NOT MISS OUT ANY TOOLS.";

/// Build a prompt for combined MCP servers and tools discovery.
#[allow(dead_code)]
pub fn build_servers_and_tools_prompt(text: &str) -> String {
    format!("{}\n\n**TEXT**:\n{}", MCP_SERVERS_AND_TOOLS_PROMPT, text)
}

//
// Internal Tools Discovery Utilities.
//

/// JSON schema for internal/built-in tools discovery via semantic parser.
pub const INTERNAL_TOOLS_SCHEMA: &str = r#"{
    "type": "object",
    "properties": {
        "internal_tools": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the internal tool" },
                    "description": { "type": "string", "description": "What the tool does" }
                },
                "required": ["name", "description"]
            }
        }
    },
    "required": ["internal_tools"]
}"#;

/// Discovery prompt for extracting internal tools from unstructured text.
pub const INTERNAL_TOOLS_PROMPT: &str = "Extract all internal/built-in tools from the following text. \
These are tools that are part of the agent's core functionality, exclude MCP server tools. \
For each tool, extract the name and a brief description of what it does. \
DO NOT LIST ANY TOOLS THAT DO NOT EXIST IN THE TEXT. Only include tools that are explicitly mentioned. \
Tools could also appear in all sorts of formats - plain text, json, xml, etc.";

/// Parse JSON response from semantic parser into a Vec of AgentTool for internal tools.
pub fn parse_internal_tools_from_json(json: &str) -> Option<Vec<AgentTool>> {
    let parsed: serde_json::Value = serde_json::from_str(json).ok()?;
    let tools = parsed.get("internal_tools")?.as_array()?;

    let internal_tools: Vec<AgentTool> = tools
        .iter()
        .filter_map(|t| {
            Some(AgentTool {
                name: t.get("name")?.as_str()?.to_string(),
                description: t.get("description")?.as_str()?.to_string(),
                ..Default::default()
            })
        })
        .collect();

    Some(internal_tools)
}

/// JSON schema for MCP server info discovery (including connection status).
/// Used for parsing `claude mcp list` output.
#[allow(dead_code)]
pub const MCP_SERVER_INFO_SCHEMA: &str = r#"{
    "type": "object",
    "properties": {
        "mcp_servers": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name/identifier of the MCP server" },
                    "transport": { "type": "string", "enum": ["stdio", "sse", "websocket"], "description": "Transport type - stdio for command-line, sse for HTTP/HTTPS URLs, websocket for ws/wss URLs" },
                    "command": { "type": "string", "description": "Command to run for stdio transport (e.g., 'uvx arch-ops-server')" },
                    "url": { "type": "string", "description": "URL for HTTP/SSE or WebSocket transports" },
                    "status": { "type": "string", "enum": ["connected", "needs_auth", "failed", "unknown"], "description": "Connection status - connected if checkmark, needs_auth if warning, failed if X mark" }
                },
                "required": ["name", "transport", "status"]
            }
        }
    },
    "required": ["mcp_servers"]
}"#;

/// Discovery prompt for extracting MCP server info from `claude mcp list` output.
#[allow(dead_code)]
pub const MCP_SERVER_INFO_PROMPT: &str = "Extract all MCP servers from the following `claude mcp list` output. \
For each server line, identify: the name (before the colon), the transport type (stdio if it's a command, sse if HTTP/HTTPS URL, websocket if ws/wss URL), \
the command (for stdio) or url (for HTTP/WebSocket), and the status (connected if ✓ or 'Connected', needs_auth if ⚠ or 'Needs authentication', failed if ✗ or 'Failed'). \
DO NOT LIST ANY SERVERS THAT DO NOT EXIST IN THE TEXT.";

/// Build a prompt for MCP server info discovery.
#[allow(dead_code)]
pub fn build_mcp_server_info_prompt(text: &str) -> String {
    format!("{}\n\n**TEXT**:\n{}", MCP_SERVER_INFO_PROMPT, text)
}

/// Parse JSON response from semantic parser into a Vec of McpServer with tools.
/// This parses the combined servers+tools schema.
/// Returns None if parsing fails.
#[allow(dead_code)]
pub fn parse_servers_and_tools_from_json(json: &str) -> Option<Vec<McpServer>> {
    let parsed: serde_json::Value = serde_json::from_str(json).ok()?;
    let servers = parsed.get("mcp_servers")?.as_array()?;

    let mcp_servers: Vec<McpServer> = servers
        .iter()
        .filter_map(|s| {
            let name = s.get("name")?.as_str()?.to_string();
            let transport_str = s.get("transport")?.as_str()?;
            let transport = match transport_str {
                "stdio" => McpTransport::Stdio,
                "sse" => McpTransport::Sse,
                "websocket" => McpTransport::WebSocket,
                _ => return None,
            };
            let address = s
                .get("address")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let command = s
                .get("command")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            //
            // Parse tools for this server.
            //
            let tools: Vec<AgentTool> = s
                .get("tools")
                .and_then(|v| v.as_array())
                .map(|tools_arr| {
                    tools_arr
                        .iter()
                        .filter_map(|t| {
                            Some(AgentTool {
                                name: t.get("name")?.as_str()?.to_string(),
                                description: t.get("description")?.as_str()?.to_string(),
                                ..Default::default()
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(McpServer {
                name,
                transport,
                address,
                command,
                tools,
                ..Default::default()
            })
        })
        .collect();

    Some(mcp_servers)
}

//
// Semantic Parser Client.
//

/// Global semantic parser client (can be updated on reconnection)
static SEMANTIC_PARSER_CLIENT: RwLock<Option<Arc<SemanticParserClient>>> = RwLock::new(None);

/// Initialize or update the global semantic parser client.
/// Called on initial connection and on reconnection to update the channel.
pub fn init_global_client(client: SemanticParserClient) {
    let mut guard = SEMANTIC_PARSER_CLIENT.write().unwrap();
    let is_update = guard.is_some();
    *guard = Some(Arc::new(client));
    if is_update {
        common::log_info!("Semantic parser client updated with new channel (reconnection)");
    } else {
        common::log_info!("Semantic parser client initialized");
    }
}

/// Get the global semantic parser client
pub fn get_client() -> Option<Arc<SemanticParserClient>> {
    SEMANTIC_PARSER_CLIENT.read().unwrap().clone()
}

/// Manages pending semantic parser requests
pub struct SemanticParserTracker {
    pending: Mutex<HashMap<String, oneshot::Sender<SemanticParserResponse>>>,
}

impl SemanticParserTracker {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Register a new request and return a receiver for the response
    pub fn register(&self, request_id: String) -> oneshot::Receiver<SemanticParserResponse> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(request_id, tx);
        rx
    }

    /// Complete a request with its response
    pub fn complete(&self, response: SemanticParserResponse) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&response.request_id) {
            let _ = tx.send(response);
        }
    }
}

/// Client for sending semantic parser requests
pub struct SemanticParserClient {
    channel: Arc<Channel>,
    node_id: String,
    tracker: Arc<SemanticParserTracker>,
}

impl SemanticParserClient {
    pub fn new(
        channel: Arc<Channel>,
        node_id: String,
        tracker: Arc<SemanticParserTracker>,
    ) -> Self {
        Self {
            channel,
            node_id,
            tracker,
        }
    }

    /// Send a semantic parser request and wait for the response
    pub async fn parse(
        &self,
        instruction: String,
        text: String,
        schema: String,
    ) -> Result<SemanticParserResponse> {
        let request_id = Uuid::new_v4().to_string();

        //
        // Register the request before sending.
        //
        let rx = self.tracker.register(request_id.clone());

        //
        // Build the request.
        //
        let request = SemanticParserRequest {
            request_id: request_id.clone(),
            instruction,
            text,
            schema,
        };

        //
        // Send the request to the service.
        //
        let message = NodeSignalMessage::SemanticParserRequest {
            node_id: self.node_id.clone(),
            request,
        };

        publish_json(&self.channel, NODE_SIGNAL_QUEUE, &message)
            .await
            .map_err(|e| anyhow!("Failed to send semantic parser request: {}", e))?;

        common::log_info!("Sent semantic parser request {}", &request_id[..8]);

        //
        // Wait for the response with a timeout.
        //
        match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(anyhow!("Semantic parser request was cancelled")),
            Err(_) => Err(anyhow!("Semantic parser request timed out")),
        }
    }
}
