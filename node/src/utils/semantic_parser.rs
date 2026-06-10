use super::LockExt;
use anyhow::{Result, anyhow};
use common::{
    AgentTool, NODE_SIGNAL_QUEUE, NodeSignalMessage, SemanticParserRequest, SemanticParserResponse,
    publish_json,
};
use lapin::Channel;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::oneshot;
use uuid::Uuid;

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
        self.pending.lock_safe().insert(request_id, tx);
        rx
    }

    /// Complete a request with its response
    pub fn complete(&self, response: SemanticParserResponse) {
        if let Some(tx) = self.pending.lock_safe().remove(&response.request_id) {
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
