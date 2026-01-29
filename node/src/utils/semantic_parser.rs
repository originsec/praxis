use anyhow::{anyhow, Result};
use common::{
    publish_json, AgentTool, McpServer, McpTransport, NodeSignalMessage, SemanticParserRequest,
    SemanticParserResponse, NODE_SIGNAL_QUEUE,
};
use lapin::Channel;
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
/// This schema extracts agent internal tools (like Bash, Read, Write, Grep, etc).
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
These are tools that are part of the agent's core functionality, NOT MCP server tools. \
Examples include: Bash (command execution), Read (file reading), Write (file writing), \
Edit (file editing), Grep (search), Glob (file pattern matching), Task (agent spawning), etc. \
For each tool, extract the name and a brief description of what it does. \
DO NOT LIST ANY TOOLS THAT DO NOT EXIST IN THE TEXT. Only include tools that are explicitly mentioned.";

/// Build a prompt for internal tools discovery from unstructured text.
pub fn build_internal_tools_prompt(text: &str) -> String {
    format!("{}\n\n**TEXT**:\n{}", INTERNAL_TOOLS_PROMPT, text)
}

/// Parse JSON response from semantic parser into a Vec of AgentTool for internal tools.
/// Returns None if parsing fails.
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
// Metadata Extraction Utilities.
//

/// JSON schema for extracting metadata (user identities and API keys) from config files.
pub const METADATA_EXTRACTION_SCHEMA: &str = r#"{
    "type": "object",
    "properties": {
        "user_identities": {
            "type": "array",
            "items": { "type": "string" },
            "description": "User identities found (emails, usernames, account IDs, organization names)"
        },
        "api_keys": {
            "type": "array",
            "items": { "type": "string" },
            "description": "API keys found (partial or full keys, tokens, secrets)"
        }
    }
}"#;

/// Discovery prompt for extracting metadata from configuration files.
pub const METADATA_EXTRACTION_PROMPT: &str = "Analyze the following configuration file contents and extract:\n\
1. User identities: Any emails (look for email structure), usernames (identify via field names)\n\
2. API keys: Any API keys, tokens, secrets, or credentials - identify by field names\n\n\
Only extract values that actually exist in the text. Do not guess or fabricate any information.";

/// Build a prompt for metadata extraction from config file contents.
pub fn build_metadata_extraction_prompt(config_contents: &str) -> String {
    format!("{}\n\n**CONFIG FILES**:\n{}", METADATA_EXTRACTION_PROMPT, config_contents)
}

/// Parsed metadata from semantic parser response
#[derive(Debug, Default)]
pub struct ExtractedMetadata {
    pub user_identities: Vec<String>,
    pub api_keys: Vec<String>,
}

/// Parse JSON response from semantic parser into ExtractedMetadata.
/// Returns None if parsing fails.
pub fn parse_metadata_from_json(json: &str) -> Option<ExtractedMetadata> {
    let parsed: serde_json::Value = serde_json::from_str(json).ok()?;

    let user_identities = parsed
        .get("user_identities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let api_keys = parsed
        .get("api_keys")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Some(ExtractedMetadata {
        user_identities,
        api_keys,
    })
}

//
// Semantic Parser Client.
//

/// Global semantic parser client (initialized once in main)
static SEMANTIC_PARSER_CLIENT: OnceCell<Arc<SemanticParserClient>> = OnceCell::new();

/// Initialize the global semantic parser client
pub fn init_global_client(client: SemanticParserClient) {
    let _ = SEMANTIC_PARSER_CLIENT.set(Arc::new(client));
}

/// Get the global semantic parser client
pub fn get_client() -> Option<Arc<SemanticParserClient>> {
    SEMANTIC_PARSER_CLIENT.get().cloned()
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
    pub fn new(channel: Arc<Channel>, node_id: String, tracker: Arc<SemanticParserTracker>) -> Self {
        Self {
            channel,
            node_id,
            tracker,
        }
    }

    /// Send a semantic parser request and wait for the response
    pub async fn parse(&self, prompt: String, schema: String) -> Result<SemanticParserResponse> {
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
            prompt,
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
