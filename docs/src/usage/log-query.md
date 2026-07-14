# Log Query

The Log Query feature provides a KQL-like query interface for exploring and correlating data across Praxis virtual tables (captured traffic, events, recon results, nodes, agents, operation history, etc). The syntax is inspired by Kusto Query Language but only a subset of KQL is implemented — not all features or functions from the full Kusto specification will work. Write queries in the code editor (or press `Ctrl+E` to edit in `$VISUAL` / `$EDITOR`), execute them with **Ctrl+R** (Ctrl+Enter is kept as an alias), and browse paginated results.

## Available Tables

### AgentLogs

Discovered agents across all nodes (in-memory).

| Column | Description |
|--------|-------------|
| timestamp | Last update time |
| node_id | Node identifier |
| agent_short_name | Agent short name |
| agent_name | Agent display name |
| version | Agent version (if known) |

### EventLogs

Centralized application log entries from service and nodes. Requires `application_logs_enabled` to be set to `true` in settings.

| Column | Description |
|--------|-------------|
| timestamp | When the log entry was recorded |
| source | Origin category: "service" or "node" |
| source_id | Instance identifier (e.g. node UUID; empty for service) |
| level | Log level: error, warn, info, debug, trace |
| target | Log target/module (may be null) |
| message | Log message text |

### SemanticOperationChainLogs

Chain execution history, including per-element state and final outputs. The `elements` and `outputs` columns contain JSON — use `contains()` to search within them.

| Column | Description |
|--------|-------------|
| timestamp | When the chain execution was created |
| execution_id | Chain execution identifier |
| chain_id | Chain definition identifier |
| chain_name | Chain display name |
| node_id | Node that executed the chain |
| agent_short_name | Agent that executed the chain |
| status | Execution status: Queued, Running, Completed, Failed, Cancelled |
| elements | Per-element execution state (JSON) |
| outputs | Final outputs from termination elements (JSON) |
| started_at | When execution started |
| ended_at | When execution ended (null if still running) |

### NodeLogs

Currently connected nodes (in-memory).

| Column | Description |
|--------|-------------|
| timestamp | Last update time |
| node_id | Node identifier |
| machine_name | Machine hostname |
| os_details | Operating system details |
| intercept_active | Whether interception is active |

### SemanticOperationLogs

Semantic operation execution history, including results and summaries. The `operation_spec` column contains the full operation definition as JSON — use `contains()` to search within it.

| Column | Description |
|--------|-------------|
| timestamp | When the operation was created |
| operation_id | Operation identifier |
| node_id | Node that executed the operation |
| agent_short_name | Agent that executed the operation |
| status | Operation status: Queued, Running, Completed, Failed, Cancelled |
| operation_spec | Full operation specification (JSON) |
| start_time | When the operation started |
| end_time | When the operation ended (null if still running) |
| summary | Brief summary of actions taken |
| result | Actual findings/data/output |
| chain_execution_id | Parent chain execution ID (null if standalone) |

### ReconLogs

Summary of reconnaissance results per node+agent.

| Column | Description |
|--------|-------------|
| timestamp | When recon was performed |
| node_id | Node identifier |
| agent_short_name | Agent short name |
| is_semantic | Whether this was a semantic recon |
| mcp_server_count | Number of MCP servers discovered |
| skill_count | Number of skills discovered |
| internal_tool_count | Number of internal tools discovered |
| config_count | Number of config items discovered |
| session_count | Number of sessions discovered |
| project_path_count | Number of project paths discovered |

### ReconSessionLogs

Sessions discovered during reconnaissance.

| Column | Description |
|--------|-------------|
| timestamp | When recon was performed |
| node_id | Node identifier |
| agent_short_name | Agent short name |
| session_id | Session identifier |
| context_path | Project/context path |
| last_modified | When the session was last modified |
| message_count | Number of messages in the session |

### ReconToolLogs

Individual tools discovered during reconnaissance (MCP tools, skills, internal tools).

| Column | Description |
|--------|-------------|
| timestamp | When recon was performed |
| node_id | Node identifier |
| agent_short_name | Agent short name |
| tool_type | Type: "mcp", "skill", or "internal" |
| server_name | MCP server name (null for skills/internal) |
| tool_name | Tool name |
| tool_description | Tool description |
| transport | MCP transport type (null for skills/internal) |

### ToolkitActionsLog

Toolkit tool execution history.

| Column | Description |
|--------|-------------|
| timestamp | When the action was executed |
| id | Action ID |
| execution_id | Execution identifier |
| tool_name | Tool name |
| action | Action performed |
| status | Action status |
| node_id | Node identifier |
| agent_short_name | Agent short name |
| session_id | Session identifier |
| details_json | Action details as JSON |

### TrafficLogs

Intercepted HTTP traffic stored in the database.

| Column | Description |
|--------|-------------|
| timestamp | When the traffic was captured |
| traffic_id | Traffic entry ID (join key for TrafficMatchLogs) |
| node_id | Node that captured the traffic |
| agent_short_name | Agent associated with this traffic |
| intercept_method | Method used (proxy, vpn, hosts, tproxy) |
| direction | send or receive |
| method | HTTP method (GET, POST, etc.) |
| url | Full URL |
| host | Host/domain |
| request_headers | Request headers as JSON |
| request_body | Request body as text |
| response_status | HTTP response status code |
| response_headers | Response headers as JSON |
| response_body | Response body as text |

### TrafficMatchLogs

Traffic that matched intercept rules, joined with traffic details.

| Column | Description |
|--------|-------------|
| timestamp | When the match occurred |
| traffic_id | ID of the matched traffic entry (join key for TrafficLogs) |
| node_id | Node that captured the traffic |
| agent_short_name | Agent associated with this traffic |
| rule_id | ID of the matching rule |
| rule_name | Name of the matching rule |
| summary | LLM-generated summary (if rule has summarization prompt) |
| method | HTTP method |
| url | Full URL |
| host | Host/domain |
| direction | send or receive |
| response_status | HTTP response status code |

## Supported KQL Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `where` | Filter rows | `TrafficLogs \| where host contains "openai"` |
| `project` | Select columns | `TrafficLogs \| project timestamp, url, host` |
| `project-away` | Remove columns | `TrafficLogs \| project-away request_body, response_body` |
| `sort` / `order` | Sort rows | `TrafficLogs \| sort timestamp` |
| `take` / `limit` | Limit rows | `TrafficLogs \| take 50` |
| `top` | Top N by column | `TrafficLogs \| top 10 by timestamp` |
| `extend` | Add computed columns | `TrafficLogs \| extend url_length = strlen(url)` |
| `count` | Count rows | `TrafficLogs \| count` |
| `distinct` | Unique values | `TrafficLogs \| distinct host` |
| `summarize` | Aggregate | `TrafficLogs \| summarize count() by host` |
| `join` | Join two tables | `TrafficLogs \| join (TrafficMatchLogs) on traffic_id` |

Join supports qualified keys when column names differ between tables:
```
LeftTable | join (RightTable) on $left.col_a == $right.col_b
```

### Supported Expressions

- **Comparisons:** `==`, `!=`, `<`, `>`, `<=`, `>=`
- **Logical:** `and`, `or`, `not`
- **String functions:** `contains`, `startswith`, `endswith`, `has`, `strlen`, `tolower`, `toupper`
- **Null checks:** `isnotempty()`, `isnull()`, `isempty()`
- **Aggregations (in summarize):** `count()`, `sum()`, `avg()`, `min()`, `max()`, `dcount()`
- **Type conversion:** `tostring()`, `toint()`, `tolong()`

## Example Queries

```kql
// List recent traffic
TrafficLogs | take 20

// Find traffic to a specific host
TrafficLogs | where host contains "api.openai.com" | project timestamp, method, url, response_status

// Count traffic by host
TrafficLogs | summarize count() by host

// List all connected nodes
NodeLogs

// Find available agents
AgentLogs | where available == true

// Find all MCP tools across agents
ReconToolLogs | where tool_type == "mcp" | project agent_short_name, server_name, tool_name

// Correlate traffic matches with rules
TrafficMatchLogs | project timestamp, rule_name, url, summary | take 50

// Join traffic with matches to see matched URLs with rule names
TrafficLogs | join (TrafficMatchLogs) on traffic_id | project timestamp, url, rule_name, summary

// Find traffic with large responses
TrafficLogs | where response_status == 200 | project timestamp, url, host | take 100

// View recent error logs
EventLogs | where level == "error" | take 50

// Count log entries by source
EventLogs | summarize count() by source

// List completed operations with results
SemanticOperationLogs | where status == "Completed" | project timestamp, agent_short_name, summary, result | take 50

// Find failed operations
SemanticOperationLogs | where status == "Failed" | project timestamp, operation_id, agent_short_name, result

// Count operations by status
SemanticOperationLogs | summarize count() by status

// Find operations that are part of a chain
SemanticOperationLogs | where isnotempty(chain_execution_id) | project timestamp, operation_id, chain_execution_id, summary

// List chain executions
SemanticOperationChainLogs | project timestamp, chain_name, status, outputs | take 20

// Find completed chains with their outputs
SemanticOperationChainLogs | where status == "Completed" | project timestamp, chain_name, outputs
```

## Query Execution

### SQL Pushdown

Tables backed by the database (EventLogs, TrafficLogs, TrafficMatchLogs, SemanticOperationLogs, SemanticOperationChainLogs) benefit from automatic SQL pushdown. When the executor encounters leading `where` and `take`/`limit` operators in a query pipeline, it translates KQL expressions directly into SQL WHERE clauses with parameterized queries. This means the database handles filtering before rows are loaded into memory, enabling efficient queries over large datasets.

The following KQL constructs are translated to SQL:

- **Comparisons:** `==`, `!=`, `<`, `>`, `<=`, `>=` become SQL comparison operators
- **Logical:** `and`, `or` become SQL AND/OR
- **String functions:** `contains`/`has` become `LOWER(col) LIKE '%value%'`, `startswith` becomes `LIKE 'value%'`, `endswith` becomes `LIKE '%value'`
- **Null checks:** `isnull()`/`isempty()` become `IS NULL OR = ''`, `isnotnull()`/`isnotempty()` become `IS NOT NULL AND != ''`
- **Case functions:** `tolower()`, `toupper()` become SQL `LOWER()`, `UPPER()`
- **Utility:** `strlen()` becomes `LENGTH()`, `tostring()` becomes `CAST(... AS TEXT)`, `toint()`/`tolong()` become `CAST(... AS INTEGER)`, `now()` binds the current UTC timestamp

User-provided string values in LIKE patterns are escaped to prevent SQL wildcard injection (`%` and `_` are matched literally).

If any expression in the leading where clauses cannot be translated to SQL (e.g. an unsupported function), the executor falls back to fetching all rows with just a LIMIT and applies all filtering in memory. Operators that appear after a non-pushable operator (like `project`, `extend`, `summarize`) always run in memory.

In-memory tables (NodeLogs, AgentLogs) and JSON-expanded tables (ReconLogs, ReconToolLogs, etc.) are always materialized fully and filtered in memory.

### Result Limits

Results are capped by the `log_query_row_limit` setting, which defaults to 10,000,000 rows. This limit can be configured in **Settings > Service > Event Logging**. The `total_count` field reflects the actual count before capping. Use `take` or `limit` to reduce result size for large tables.

## KQL Parser

The Log Query feature uses a vendored fork of the [kqlparser](https://github.com/irtimmer/rust-kql) crate (v0.0.4, Apache-2.0) for parsing KQL syntax. The vendored copy lives in `service/src/log_query/parser/` and includes fixes for multiline join expressions and native `$left`/`$right` join key syntax. Only the subset of KQL operators and functions listed above are supported; unsupported constructs will return an error.
