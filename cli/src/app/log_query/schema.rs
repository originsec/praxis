//
// Schema reference for the Log Query window. Mirrors the `TABLES` constant
// in `web/frontend/src/components/command/LogQueryModal.tsx` and the
// `TABLE_SCHEMAS` in `web/frontend/src/components/log-query/KqlCodeEditor.tsx`.
// Any update to the backend (`service/src/log_query/tables.rs`) must be
// reflected here so autocomplete and the sidebar stay in sync.
//
// Tables are sorted alphabetically.
//

pub struct TableInfo {
    pub name: &'static str,
    pub description: &'static str,

    //
    // Where the table comes from: "DB" for SQL-backed tables,
    // "MEM" for in-memory tables materialised from `NodeRegistry`.
    //
    pub source: &'static str,
    pub columns: &'static [ColumnInfo],
}

pub struct ColumnInfo {
    pub name: &'static str,
    pub description: &'static str,
}

pub const TABLES: &[TableInfo] = &[
    TableInfo {
        name: "AgentLogs",
        description: "Discovered agents across nodes",
        source: "MEM",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Last update" },
            ColumnInfo { name: "node_id", description: "Node identifier" },
            ColumnInfo { name: "agent_short_name", description: "Agent short name" },
            ColumnInfo { name: "agent_name", description: "Display name" },
            ColumnInfo { name: "version", description: "Agent version" },
        ],
    },
    TableInfo {
        name: "EventLogs",
        description: "System event log entries",
        source: "DB",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Event time" },
            ColumnInfo { name: "source", description: "Event source" },
            ColumnInfo { name: "source_id", description: "Instance identifier" },
            ColumnInfo { name: "level", description: "Log level" },
            ColumnInfo { name: "target", description: "Log target module" },
            ColumnInfo { name: "message", description: "Log message" },
        ],
    },
    TableInfo {
        name: "NodeLogs",
        description: "Connected nodes",
        source: "MEM",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Last update" },
            ColumnInfo { name: "node_id", description: "Node identifier" },
            ColumnInfo { name: "machine_name", description: "Hostname" },
            ColumnInfo { name: "os_details", description: "OS info" },
            ColumnInfo { name: "intercept_active", description: "Interception active" },
        ],
    },
    TableInfo {
        name: "ReconLogs",
        description: "Recon summary per node+agent",
        source: "DB",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Recon time" },
            ColumnInfo { name: "node_id", description: "Node identifier" },
            ColumnInfo { name: "agent_short_name", description: "Agent short name" },
            ColumnInfo { name: "is_semantic", description: "Semantic recon" },
            ColumnInfo { name: "mcp_server_count", description: "MCP servers found" },
            ColumnInfo { name: "skill_count", description: "Skills found" },
            ColumnInfo { name: "internal_tool_count", description: "Internal tools" },
            ColumnInfo { name: "config_count", description: "Config items found" },
            ColumnInfo { name: "session_count", description: "Sessions" },
            ColumnInfo { name: "project_path_count", description: "Project paths" },
        ],
    },
    TableInfo {
        name: "ReconMetadataLogs",
        description: "User identities and API keys from recon",
        source: "DB",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Recon time" },
            ColumnInfo { name: "node_id", description: "Node identifier" },
            ColumnInfo { name: "agent_short_name", description: "Agent short name" },
            ColumnInfo { name: "entry_type", description: "Entry kind" },
            ColumnInfo { name: "value", description: "Entry value" },
        ],
    },
    TableInfo {
        name: "ReconSessionLogs",
        description: "Session data discovered in recon",
        source: "DB",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Recon time" },
            ColumnInfo { name: "node_id", description: "Node identifier" },
            ColumnInfo { name: "agent_short_name", description: "Agent short name" },
            ColumnInfo { name: "session_id", description: "Session ID" },
            ColumnInfo { name: "context_path", description: "Workspace path" },
            ColumnInfo { name: "last_modified", description: "Last modified" },
            ColumnInfo { name: "message_count", description: "Message count" },
        ],
    },
    TableInfo {
        name: "ReconToolLogs",
        description: "Individual tools from recon",
        source: "DB",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Recon time" },
            ColumnInfo { name: "node_id", description: "Node identifier" },
            ColumnInfo { name: "agent_short_name", description: "Agent short name" },
            ColumnInfo { name: "tool_type", description: "mcp, skill, or internal" },
            ColumnInfo { name: "server_name", description: "MCP server name" },
            ColumnInfo { name: "tool_name", description: "Tool name" },
            ColumnInfo { name: "tool_description", description: "Tool description" },
            ColumnInfo { name: "transport", description: "Transport (stdio/http)" },
        ],
    },
    TableInfo {
        name: "SemanticOperationChainLogs",
        description: "Chain execution history",
        source: "DB",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Creation time" },
            ColumnInfo { name: "execution_id", description: "Chain execution ID" },
            ColumnInfo { name: "chain_id", description: "Chain definition ID" },
            ColumnInfo { name: "chain_name", description: "Chain name" },
            ColumnInfo { name: "node_id", description: "Node identifier" },
            ColumnInfo { name: "agent_short_name", description: "Agent short name" },
            ColumnInfo { name: "status", description: "Execution status" },
            ColumnInfo { name: "elements", description: "Chain elements (JSON)" },
            ColumnInfo { name: "outputs", description: "Terminal outputs (JSON)" },
            ColumnInfo { name: "started_at", description: "Start time" },
            ColumnInfo { name: "ended_at", description: "End time" },
        ],
    },
    TableInfo {
        name: "SemanticOperationLogs",
        description: "Semantic operation history",
        source: "DB",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Creation time" },
            ColumnInfo { name: "operation_id", description: "Operation ID" },
            ColumnInfo { name: "node_id", description: "Node identifier" },
            ColumnInfo { name: "agent_short_name", description: "Agent short name" },
            ColumnInfo { name: "status", description: "Operation status" },
            ColumnInfo { name: "operation_spec", description: "Op spec identifier" },
            ColumnInfo { name: "start_time", description: "Start time" },
            ColumnInfo { name: "end_time", description: "End time" },
            ColumnInfo { name: "summary", description: "Summary of actions" },
            ColumnInfo { name: "result", description: "Output/findings" },
            ColumnInfo { name: "chain_execution_id", description: "Parent chain ID" },
        ],
    },
    TableInfo {
        name: "ToolkitActionsLog",
        description: "Toolkit tool execution history",
        source: "DB",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Execution time" },
            ColumnInfo { name: "id", description: "Action ID" },
            ColumnInfo { name: "execution_id", description: "Execution identifier" },
            ColumnInfo { name: "tool_name", description: "Tool name" },
            ColumnInfo { name: "action", description: "Action performed" },
            ColumnInfo { name: "status", description: "Action status" },
            ColumnInfo { name: "node_id", description: "Node identifier" },
            ColumnInfo { name: "agent_short_name", description: "Agent short name" },
            ColumnInfo { name: "session_id", description: "Session identifier" },
            ColumnInfo { name: "details_json", description: "Action details (JSON)" },
        ],
    },
    TableInfo {
        name: "TrafficLogs",
        description: "Intercepted HTTP traffic",
        source: "DB",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Capture time" },
            ColumnInfo { name: "traffic_id", description: "Traffic record ID" },
            ColumnInfo { name: "node_id", description: "Capturing node" },
            ColumnInfo { name: "agent_short_name", description: "Agent short name" },
            ColumnInfo { name: "intercept_method", description: "Capture method" },
            ColumnInfo { name: "direction", description: "Request/response" },
            ColumnInfo { name: "method", description: "HTTP method" },
            ColumnInfo { name: "url", description: "Full URL" },
            ColumnInfo { name: "host", description: "Host/domain" },
            ColumnInfo { name: "request_headers", description: "Request headers" },
            ColumnInfo { name: "request_body", description: "Request body" },
            ColumnInfo { name: "response_status", description: "HTTP status code" },
            ColumnInfo { name: "response_headers", description: "Response headers" },
            ColumnInfo { name: "response_body", description: "Response body" },
        ],
    },
    TableInfo {
        name: "TrafficMatchLogs",
        description: "Traffic matching intercept rules",
        source: "DB",
        columns: &[
            ColumnInfo { name: "timestamp", description: "Match time" },
            ColumnInfo { name: "traffic_id", description: "Traffic record ID" },
            ColumnInfo { name: "node_id", description: "Node identifier" },
            ColumnInfo { name: "agent_short_name", description: "Agent short name" },
            ColumnInfo { name: "rule_id", description: "Rule identifier" },
            ColumnInfo { name: "rule_name", description: "Rule name" },
            ColumnInfo { name: "summary", description: "LLM summary" },
            ColumnInfo { name: "method", description: "HTTP method" },
            ColumnInfo { name: "url", description: "Full URL" },
            ColumnInfo { name: "host", description: "Host/domain" },
            ColumnInfo { name: "direction", description: "Request/response" },
            ColumnInfo { name: "response_status", description: "HTTP status code" },
        ],
    },
];

pub fn find_table(name: &str) -> Option<&'static TableInfo> {
    TABLES
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
}
