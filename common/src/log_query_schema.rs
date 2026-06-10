//
// Canonical log-query table schema, shared by the service (column lists for
// the query engine), the CLI (schema sidebar + autocomplete), and drift
// tests covering the web frontend and documentation. This is the single
// source of truth: any table or column change happens here first.
//
// Tables are sorted alphabetically.
//

pub struct TableSchema {
    pub name: &'static str,
    pub description: &'static str,

    //
    // Where the table comes from: "DB" for SQL-backed tables,
    // "MEM" for in-memory tables materialised from the node registry.
    //
    pub source: &'static str,
    pub columns: &'static [ColumnSchema],
}

pub struct ColumnSchema {
    pub name: &'static str,
    pub description: &'static str,
}

pub const TABLES: &[TableSchema] = &[
    TableSchema {
        name: "AgentLogs",
        description: "Discovered agents across nodes",
        source: "MEM",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Last update",
            },
            ColumnSchema {
                name: "node_id",
                description: "Node identifier",
            },
            ColumnSchema {
                name: "agent_short_name",
                description: "Agent short name",
            },
            ColumnSchema {
                name: "agent_name",
                description: "Display name",
            },
            ColumnSchema {
                name: "version",
                description: "Agent version",
            },
        ],
    },
    TableSchema {
        name: "EventLogs",
        description: "System event log entries",
        source: "DB",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Event time",
            },
            ColumnSchema {
                name: "source",
                description: "Event source",
            },
            ColumnSchema {
                name: "source_id",
                description: "Instance identifier",
            },
            ColumnSchema {
                name: "level",
                description: "Log level",
            },
            ColumnSchema {
                name: "target",
                description: "Log target module",
            },
            ColumnSchema {
                name: "message",
                description: "Log message",
            },
        ],
    },
    TableSchema {
        name: "NodeLogs",
        description: "Connected nodes",
        source: "MEM",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Last update",
            },
            ColumnSchema {
                name: "node_id",
                description: "Node identifier",
            },
            ColumnSchema {
                name: "machine_name",
                description: "Hostname",
            },
            ColumnSchema {
                name: "os_details",
                description: "OS info",
            },
            ColumnSchema {
                name: "intercept_active",
                description: "Interception active",
            },
        ],
    },
    TableSchema {
        name: "ReconLogs",
        description: "Recon summary per node+agent",
        source: "DB",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Recon time",
            },
            ColumnSchema {
                name: "node_id",
                description: "Node identifier",
            },
            ColumnSchema {
                name: "agent_short_name",
                description: "Agent short name",
            },
            ColumnSchema {
                name: "is_semantic",
                description: "Semantic recon",
            },
            ColumnSchema {
                name: "mcp_server_count",
                description: "MCP servers found",
            },
            ColumnSchema {
                name: "skill_count",
                description: "Skills found",
            },
            ColumnSchema {
                name: "internal_tool_count",
                description: "Internal tools",
            },
            ColumnSchema {
                name: "config_count",
                description: "Config items found",
            },
            ColumnSchema {
                name: "session_count",
                description: "Sessions",
            },
            ColumnSchema {
                name: "project_path_count",
                description: "Project paths",
            },
        ],
    },
    TableSchema {
        name: "ReconSessionLogs",
        description: "Session data discovered in recon",
        source: "DB",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Recon time",
            },
            ColumnSchema {
                name: "node_id",
                description: "Node identifier",
            },
            ColumnSchema {
                name: "agent_short_name",
                description: "Agent short name",
            },
            ColumnSchema {
                name: "session_id",
                description: "Session ID",
            },
            ColumnSchema {
                name: "context_path",
                description: "Workspace path",
            },
            ColumnSchema {
                name: "last_modified",
                description: "Last modified",
            },
            ColumnSchema {
                name: "message_count",
                description: "Message count",
            },
        ],
    },
    TableSchema {
        name: "ReconToolLogs",
        description: "Individual tools from recon",
        source: "DB",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Recon time",
            },
            ColumnSchema {
                name: "node_id",
                description: "Node identifier",
            },
            ColumnSchema {
                name: "agent_short_name",
                description: "Agent short name",
            },
            ColumnSchema {
                name: "tool_type",
                description: "mcp, skill, or internal",
            },
            ColumnSchema {
                name: "server_name",
                description: "MCP server name",
            },
            ColumnSchema {
                name: "tool_name",
                description: "Tool name",
            },
            ColumnSchema {
                name: "tool_description",
                description: "Tool description",
            },
            ColumnSchema {
                name: "transport",
                description: "Transport (stdio/http)",
            },
        ],
    },
    TableSchema {
        name: "SemanticOperationChainLogs",
        description: "Chain execution history",
        source: "DB",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Creation time",
            },
            ColumnSchema {
                name: "execution_id",
                description: "Chain execution ID",
            },
            ColumnSchema {
                name: "chain_id",
                description: "Chain definition ID",
            },
            ColumnSchema {
                name: "chain_name",
                description: "Chain name",
            },
            ColumnSchema {
                name: "node_id",
                description: "Node identifier",
            },
            ColumnSchema {
                name: "agent_short_name",
                description: "Agent short name",
            },
            ColumnSchema {
                name: "status",
                description: "Execution status",
            },
            ColumnSchema {
                name: "elements",
                description: "Chain elements (JSON)",
            },
            ColumnSchema {
                name: "outputs",
                description: "Terminal outputs (JSON)",
            },
            ColumnSchema {
                name: "started_at",
                description: "Start time",
            },
            ColumnSchema {
                name: "ended_at",
                description: "End time",
            },
        ],
    },
    TableSchema {
        name: "SemanticOperationLogs",
        description: "Semantic operation history",
        source: "DB",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Creation time",
            },
            ColumnSchema {
                name: "operation_id",
                description: "Operation ID",
            },
            ColumnSchema {
                name: "node_id",
                description: "Node identifier",
            },
            ColumnSchema {
                name: "agent_short_name",
                description: "Agent short name",
            },
            ColumnSchema {
                name: "status",
                description: "Operation status",
            },
            ColumnSchema {
                name: "operation_spec",
                description: "Op spec identifier",
            },
            ColumnSchema {
                name: "start_time",
                description: "Start time",
            },
            ColumnSchema {
                name: "end_time",
                description: "End time",
            },
            ColumnSchema {
                name: "summary",
                description: "Summary of actions",
            },
            ColumnSchema {
                name: "result",
                description: "Output/findings",
            },
            ColumnSchema {
                name: "chain_execution_id",
                description: "Parent chain ID",
            },
        ],
    },
    TableSchema {
        name: "ToolkitActionsLog",
        description: "Toolkit tool execution history",
        source: "DB",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Execution time",
            },
            ColumnSchema {
                name: "id",
                description: "Action ID",
            },
            ColumnSchema {
                name: "execution_id",
                description: "Execution identifier",
            },
            ColumnSchema {
                name: "tool_name",
                description: "Tool name",
            },
            ColumnSchema {
                name: "action",
                description: "Action performed",
            },
            ColumnSchema {
                name: "status",
                description: "Action status",
            },
            ColumnSchema {
                name: "node_id",
                description: "Node identifier",
            },
            ColumnSchema {
                name: "agent_short_name",
                description: "Agent short name",
            },
            ColumnSchema {
                name: "session_id",
                description: "Session identifier",
            },
            ColumnSchema {
                name: "details_json",
                description: "Action details (JSON)",
            },
        ],
    },
    TableSchema {
        name: "TrafficLogs",
        description: "Intercepted HTTP traffic",
        source: "DB",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Capture time",
            },
            ColumnSchema {
                name: "traffic_id",
                description: "Traffic record ID",
            },
            ColumnSchema {
                name: "node_id",
                description: "Capturing node",
            },
            ColumnSchema {
                name: "agent_short_name",
                description: "Agent short name",
            },
            ColumnSchema {
                name: "intercept_method",
                description: "Capture method",
            },
            ColumnSchema {
                name: "direction",
                description: "Request/response",
            },
            ColumnSchema {
                name: "method",
                description: "HTTP method",
            },
            ColumnSchema {
                name: "url",
                description: "Full URL",
            },
            ColumnSchema {
                name: "host",
                description: "Host/domain",
            },
            ColumnSchema {
                name: "request_headers",
                description: "Request headers",
            },
            ColumnSchema {
                name: "request_body",
                description: "Request body",
            },
            ColumnSchema {
                name: "response_status",
                description: "HTTP status code",
            },
            ColumnSchema {
                name: "response_headers",
                description: "Response headers",
            },
            ColumnSchema {
                name: "response_body",
                description: "Response body",
            },
        ],
    },
    TableSchema {
        name: "TrafficMatchLogs",
        description: "Traffic matching intercept rules",
        source: "DB",
        columns: &[
            ColumnSchema {
                name: "timestamp",
                description: "Match time",
            },
            ColumnSchema {
                name: "traffic_id",
                description: "Traffic record ID",
            },
            ColumnSchema {
                name: "node_id",
                description: "Node identifier",
            },
            ColumnSchema {
                name: "agent_short_name",
                description: "Agent short name",
            },
            ColumnSchema {
                name: "rule_id",
                description: "Rule identifier",
            },
            ColumnSchema {
                name: "rule_name",
                description: "Rule name",
            },
            ColumnSchema {
                name: "summary",
                description: "LLM summary",
            },
            ColumnSchema {
                name: "method",
                description: "HTTP method",
            },
            ColumnSchema {
                name: "url",
                description: "Full URL",
            },
            ColumnSchema {
                name: "host",
                description: "Host/domain",
            },
            ColumnSchema {
                name: "direction",
                description: "Request/response",
            },
            ColumnSchema {
                name: "response_status",
                description: "HTTP status code",
            },
        ],
    },
];

pub fn find_table(name: &str) -> Option<&'static TableSchema> {
    TABLES.iter().find(|t| t.name.eq_ignore_ascii_case(name))
}
