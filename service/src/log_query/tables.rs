use serde_json::Value;
use std::sync::Arc;

use crate::database::Database;
use crate::state::NodeRegistry;

#[derive(Debug, Clone, Copy)]
pub enum VirtualTable {
    TrafficLogs,
    TrafficMatchLogs,
    NodeLogs,
    AgentLogs,
    ReconLogs,
    ReconToolLogs,
    ReconSessionLogs,
    EventLogs,
    ToolkitActionsLog,
    SemanticOperationLogs,
    SemanticOperationChainLogs,
}

impl VirtualTable {
    /// Canonical table name, matching `common::log_query_schema::TABLES`.
    pub fn name(&self) -> &'static str {
        match self {
            VirtualTable::TrafficLogs => "TrafficLogs",
            VirtualTable::TrafficMatchLogs => "TrafficMatchLogs",
            VirtualTable::NodeLogs => "NodeLogs",
            VirtualTable::AgentLogs => "AgentLogs",
            VirtualTable::ReconLogs => "ReconLogs",
            VirtualTable::ReconToolLogs => "ReconToolLogs",
            VirtualTable::ReconSessionLogs => "ReconSessionLogs",
            VirtualTable::EventLogs => "EventLogs",
            VirtualTable::ToolkitActionsLog => "ToolkitActionsLog",
            VirtualTable::SemanticOperationLogs => "SemanticOperationLogs",
            VirtualTable::SemanticOperationChainLogs => "SemanticOperationChainLogs",
        }
    }
}

pub const ALL_TABLES: &[VirtualTable] = &[
    VirtualTable::TrafficLogs,
    VirtualTable::TrafficMatchLogs,
    VirtualTable::NodeLogs,
    VirtualTable::AgentLogs,
    VirtualTable::ReconLogs,
    VirtualTable::ReconToolLogs,
    VirtualTable::ReconSessionLogs,
    VirtualTable::EventLogs,
    VirtualTable::ToolkitActionsLog,
    VirtualTable::SemanticOperationLogs,
    VirtualTable::SemanticOperationChainLogs,
];

pub fn resolve_table(name: &str) -> Option<VirtualTable> {
    ALL_TABLES
        .iter()
        .find(|t| t.name().eq_ignore_ascii_case(name))
        .copied()
}

//
// Column lists come from the canonical schema in common::log_query_schema;
// the order there defines the order the materializers below emit values in.
//

pub fn table_columns(table: VirtualTable) -> Vec<&'static str> {
    common::log_query_schema::find_table(table.name())
        .map(|t| t.columns.iter().map(|c| c.name).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    //
    // Every canonical schema table must resolve to a VirtualTable and vice
    // versa, so the shared schema and the query engine cannot drift apart.
    //

    #[test]
    fn schema_and_virtual_tables_match() {
        for schema in common::log_query_schema::TABLES {
            let table = resolve_table(schema.name);
            assert!(
                table.is_some(),
                "schema table {} has no VirtualTable",
                schema.name
            );
        }
        for table in ALL_TABLES {
            let schema = common::log_query_schema::find_table(table.name());
            assert!(
                schema.is_some(),
                "VirtualTable {} missing from common::log_query_schema",
                table.name()
            );
            assert!(
                !table_columns(*table).is_empty(),
                "VirtualTable {} resolved no columns",
                table.name()
            );
        }
        assert_eq!(common::log_query_schema::TABLES.len(), ALL_TABLES.len());
    }

    //
    // SQL-backed tables expose exactly the canonical column set, in order.
    //

    #[test]
    fn sql_configs_match_schema_columns() {
        for table in ALL_TABLES {
            if let Some(config) = table.sql_config() {
                let kql: Vec<&str> = config.columns.iter().map(|c| c.kql_name).collect();
                assert_eq!(
                    kql,
                    table_columns(*table),
                    "sql_config columns for {} drifted from the canonical schema",
                    table.name()
                );
            }
        }
    }
}

//
// Materialize in-memory tables from node registry.
//

pub async fn materialize_node_logs(registry: &Arc<NodeRegistry>) -> (Vec<String>, Vec<Vec<Value>>) {
    let columns: Vec<String> = table_columns(VirtualTable::NodeLogs)
        .into_iter()
        .map(String::from)
        .collect();

    let nodes = registry.list().await;
    let rows: Vec<Vec<Value>> = nodes
        .into_iter()
        .map(|node| {
            vec![
                Value::String(node.last_update_received.to_rfc3339()),
                Value::String(node.id.clone()),
                Value::String(node.machine_name.clone()),
                Value::String(node.os_details.clone()),
                Value::Bool(node.intercept_active),
            ]
        })
        .collect();

    (columns, rows)
}

pub async fn materialize_agent_logs(
    registry: &Arc<NodeRegistry>,
) -> (Vec<String>, Vec<Vec<Value>>) {
    let columns: Vec<String> = table_columns(VirtualTable::AgentLogs)
        .into_iter()
        .map(String::from)
        .collect();

    let nodes = registry.list().await;
    let mut rows = Vec::new();

    for node in nodes {
        let agents = node
            .last_update
            .as_ref()
            .map(|u| u.discovered_agents.as_slice())
            .unwrap_or(&[]);

        for agent in agents {
            rows.push(vec![
                Value::String(node.last_update_received.to_rfc3339()),
                Value::String(node.id.clone()),
                Value::String(agent.short_name.clone()),
                Value::String(agent.name.clone()),
                agent
                    .version
                    .as_ref()
                    .map(|v| Value::String(v.clone()))
                    .unwrap_or(Value::Null),
            ]);
        }
    }

    (columns, rows)
}

//
// Materialize recon tables from the database. Each returns all rows for all
// node+agent combinations.
//

pub async fn materialize_recon_logs(
    database: &Arc<Database>,
) -> anyhow::Result<(Vec<String>, Vec<Vec<Value>>)> {
    let columns: Vec<String> = table_columns(VirtualTable::ReconLogs)
        .into_iter()
        .map(String::from)
        .collect();

    let results = database.list_all_recon_results().await?;
    let rows: Vec<Vec<Value>> = results
        .into_iter()
        .map(|r| {
            vec![
                Value::String(r.performed_at.clone()),
                Value::String(r.node_id.clone()),
                Value::String(r.agent_short_name.clone()),
                Value::Bool(r.is_semantic),
                Value::Number(r.recon_result.tools.mcp_servers.len().into()),
                Value::Number(r.recon_result.tools.skills.len().into()),
                Value::Number(r.recon_result.tools.internal_tools.len().into()),
                Value::Number(r.recon_result.config.items.len().into()),
                Value::Number(r.recon_result.sessions.items.len().into()),
                Value::Number(r.recon_result.config.project_paths.len().into()),
            ]
        })
        .collect();

    Ok((columns, rows))
}

pub async fn materialize_recon_tool_logs(
    database: &Arc<Database>,
) -> anyhow::Result<(Vec<String>, Vec<Vec<Value>>)> {
    let columns: Vec<String> = table_columns(VirtualTable::ReconToolLogs)
        .into_iter()
        .map(String::from)
        .collect();

    let results = database.list_all_recon_results().await?;
    let mut rows = Vec::new();

    for r in results {
        for server in &r.recon_result.tools.mcp_servers {
            for tool in &server.tools {
                rows.push(vec![
                    Value::String(r.performed_at.clone()),
                    Value::String(r.node_id.clone()),
                    Value::String(r.agent_short_name.clone()),
                    Value::String("mcp".to_string()),
                    Value::String(server.name.clone()),
                    Value::String(tool.name.clone()),
                    Value::String(tool.description.clone()),
                    Value::String(server.transport.to_string()),
                ]);
            }
        }
        for tool in &r.recon_result.tools.skills {
            rows.push(vec![
                Value::String(r.performed_at.clone()),
                Value::String(r.node_id.clone()),
                Value::String(r.agent_short_name.clone()),
                Value::String("skill".to_string()),
                Value::Null,
                Value::String(tool.name.clone()),
                Value::String(tool.description.clone()),
                Value::Null,
            ]);
        }
        for tool in &r.recon_result.tools.internal_tools {
            rows.push(vec![
                Value::String(r.performed_at.clone()),
                Value::String(r.node_id.clone()),
                Value::String(r.agent_short_name.clone()),
                Value::String("internal".to_string()),
                Value::Null,
                Value::String(tool.name.clone()),
                Value::String(tool.description.clone()),
                Value::Null,
            ]);
        }
    }

    Ok((columns, rows))
}

pub async fn materialize_recon_session_logs(
    database: &Arc<Database>,
) -> anyhow::Result<(Vec<String>, Vec<Vec<Value>>)> {
    let columns: Vec<String> = table_columns(VirtualTable::ReconSessionLogs)
        .into_iter()
        .map(String::from)
        .collect();

    let results = database.list_all_recon_results().await?;
    let mut rows = Vec::new();

    for r in results {
        for session in &r.recon_result.sessions.items {
            rows.push(vec![
                Value::String(r.performed_at.clone()),
                Value::String(r.node_id.clone()),
                Value::String(r.agent_short_name.clone()),
                Value::String(session.session_id.clone()),
                Value::String(session.context_path.clone()),
                Value::String(session.last_modified.clone()),
                Value::Number(session.message_count.into()),
            ]);
        }
    }

    Ok((columns, rows))
}

pub async fn materialize_toolkit_actions_log(
    database: &Arc<Database>,
) -> anyhow::Result<(Vec<String>, Vec<Vec<Value>>)> {
    let columns: Vec<String> = table_columns(VirtualTable::ToolkitActionsLog)
        .into_iter()
        .map(String::from)
        .collect();

    let actions = database.list_toolkit_actions().await?;
    let rows = actions
        .into_iter()
        .map(|a| {
            vec![
                Value::String(a.created_at.to_rfc3339()),
                Value::String(a.id),
                Value::String(a.execution_id),
                Value::String(a.tool_name),
                Value::String(a.action),
                Value::String(a.status),
                a.node_id.map(Value::String).unwrap_or(Value::Null),
                a.agent_short_name.map(Value::String).unwrap_or(Value::Null),
                a.session_id.map(Value::String).unwrap_or(Value::Null),
                Value::String(
                    serde_json::to_string(&a.details).unwrap_or_else(|_| "{}".to_string()),
                ),
            ]
        })
        .collect();

    Ok((columns, rows))
}
