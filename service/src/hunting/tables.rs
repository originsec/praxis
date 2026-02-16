use std::sync::Arc;
use serde_json::Value;

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
    ReconMetadataLogs,
    EventLogs,
}

impl VirtualTable {
    #[allow(dead_code)]
    pub fn is_db_backed(&self) -> bool {
        matches!(
            self,
            VirtualTable::TrafficLogs
                | VirtualTable::TrafficMatchLogs
                | VirtualTable::ReconLogs
                | VirtualTable::ReconToolLogs
                | VirtualTable::ReconSessionLogs
                | VirtualTable::ReconMetadataLogs
                | VirtualTable::EventLogs
        )
    }
}

pub fn resolve_table(name: &str) -> Option<VirtualTable> {
    match name.to_lowercase().as_str() {
        "trafficlogs" => Some(VirtualTable::TrafficLogs),
        "trafficmatchlogs" => Some(VirtualTable::TrafficMatchLogs),
        "nodelogs" => Some(VirtualTable::NodeLogs),
        "agentlogs" => Some(VirtualTable::AgentLogs),
        "reconlogs" => Some(VirtualTable::ReconLogs),
        "recontoollogs" => Some(VirtualTable::ReconToolLogs),
        "reconsessionlogs" => Some(VirtualTable::ReconSessionLogs),
        "reconmetadatalogs" => Some(VirtualTable::ReconMetadataLogs),
        "eventlogs" => Some(VirtualTable::EventLogs),
        _ => None,
    }
}

pub fn table_columns(table: VirtualTable) -> Vec<&'static str> {
    match table {
        VirtualTable::TrafficLogs => vec![
            "timestamp", "traffic_id", "node_id", "agent_short_name", "intercept_method",
            "direction", "method", "url", "host", "request_headers", "request_body",
            "response_status", "response_headers", "response_body",
        ],
        VirtualTable::TrafficMatchLogs => vec![
            "timestamp", "traffic_id", "node_id", "agent_short_name", "rule_id",
            "rule_name", "summary", "method", "url", "host", "direction",
            "response_status",
        ],
        VirtualTable::NodeLogs => vec![
            "timestamp", "node_id", "machine_name", "os_details", "intercept_active",
        ],
        VirtualTable::AgentLogs => vec![
            "timestamp", "node_id", "agent_short_name", "agent_name", "version",
        ],
        VirtualTable::ReconLogs => vec![
            "timestamp", "node_id", "agent_short_name", "is_semantic",
            "mcp_server_count", "skill_count", "internal_tool_count",
            "config_count", "session_count", "project_path_count",
        ],
        VirtualTable::ReconToolLogs => vec![
            "timestamp", "node_id", "agent_short_name", "tool_type",
            "server_name", "tool_name", "tool_description", "transport",
        ],
        VirtualTable::ReconSessionLogs => vec![
            "timestamp", "node_id", "agent_short_name", "session_id",
            "context_path", "last_modified", "message_count",
        ],
        VirtualTable::ReconMetadataLogs => vec![
            "timestamp", "node_id", "agent_short_name", "entry_type", "value",
        ],
        VirtualTable::EventLogs => vec![
            "timestamp", "source", "source_id", "level", "target", "message",
        ],
    }
}

//
// Materialize in-memory tables from node registry.
//

pub async fn materialize_node_logs(
    registry: &Arc<NodeRegistry>,
) -> (Vec<String>, Vec<Vec<Value>>) {
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
                Value::Number(r.recon_result.config.len().into()),
                Value::Number(r.recon_result.sessions.len().into()),
                Value::Number(r.recon_result.project_paths.len().into()),
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
        for session in &r.recon_result.sessions {
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

pub async fn materialize_recon_metadata_logs(
    database: &Arc<Database>,
) -> anyhow::Result<(Vec<String>, Vec<Vec<Value>>)> {
    let columns: Vec<String> = table_columns(VirtualTable::ReconMetadataLogs)
        .into_iter()
        .map(String::from)
        .collect();

    let results = database.list_all_recon_results().await?;
    let mut rows = Vec::new();

    for r in results {
        if let Some(meta) = &r.recon_result.metadata {
            if let Some(identities) = &meta.user_identities {
                for identity in identities {
                    rows.push(vec![
                        Value::String(r.performed_at.clone()),
                        Value::String(r.node_id.clone()),
                        Value::String(r.agent_short_name.clone()),
                        Value::String("user_identity".to_string()),
                        Value::String(identity.clone()),
                    ]);
                }
            }
            if let Some(keys) = &meta.api_keys {
                for key in keys {
                    rows.push(vec![
                        Value::String(r.performed_at.clone()),
                        Value::String(r.node_id.clone()),
                        Value::String(r.agent_short_name.clone()),
                        Value::String("api_key".to_string()),
                        Value::String(key.clone()),
                    ]);
                }
            }
        }
    }

    Ok((columns, rows))
}
