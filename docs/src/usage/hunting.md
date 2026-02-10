# Hunting

The Hunting feature provides a KQL (Kusto Query Language) query interface for exploring and correlating data across Praxis virtual tables. Write KQL queries in a code editor, execute them with Ctrl+Enter, and browse paginated results.

## Available Tables

### TrafficLogs

Intercepted HTTP traffic stored in the database.

| Column | Description |
|--------|-------------|
| timestamp | When the traffic was captured |
| id | Traffic entry ID (correlation key for TrafficMatchLogs) |
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
| traffic_id | ID of the matched traffic entry (correlates to `id` in TrafficLogs) |
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

### NodeLogs

Currently connected nodes (in-memory).

| Column | Description |
|--------|-------------|
| timestamp | Last update time |
| node_id | Node identifier |
| machine_name | Machine hostname |
| os_details | Operating system details |
| intercept_active | Whether interception is active |

### AgentLogs

Discovered agents across all nodes (in-memory).

| Column | Description |
|--------|-------------|
| timestamp | Last update time |
| node_id | Node identifier |
| agent_short_name | Agent short name |
| agent_name | Agent display name |
| available | Whether the agent is available |
| version | Agent version (if known) |

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

### ReconMetadataLogs

User identities and API keys extracted from agent configurations.

| Column | Description |
|--------|-------------|
| timestamp | When recon was performed |
| node_id | Node identifier |
| agent_short_name | Agent short name |
| entry_type | "user_identity" or "api_key" |
| value | The identity or key value |

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

// List API keys found in recon
ReconMetadataLogs | where entry_type == "api_key"

// Correlate traffic matches with rules
TrafficMatchLogs | project timestamp, rule_name, url, summary | take 50

// Find traffic with large responses
TrafficLogs | where response_status == 200 | project timestamp, url, host | take 100
```

## KQL Parser

The hunting feature uses the [kqlparser](https://github.com/irtimmer/rust-kql) crate for parsing KQL syntax. Not all KQL features from the full Kusto specification are supported. Check the kqlparser GitHub repository for the current list of supported and unsupported features.

Results are capped at 10,000 rows from the service. The `total_count` field reflects the actual count before capping.
