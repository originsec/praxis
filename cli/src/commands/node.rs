use anyhow::{anyhow, Result};
use clap::Subcommand;
use serde_json::json;

use crate::client::CliClient;
use crate::output::{format_short_id, format_status, print_error, print_header, print_json, print_success, OutputFormat};

#[derive(Subcommand)]
pub enum NodeCommand {
    /// List all connected nodes
    List,

    /// Select a node by ID prefix
    Select {
        /// Node ID prefix
        prefix: String,
    },
}

pub async fn execute(client: &mut CliClient, command: NodeCommand, output: &OutputFormat) -> Result<()> {
    match command {
        NodeCommand::List => list_nodes(client, output).await,
        NodeCommand::Select { prefix } => select_node(client, &prefix, output).await,
    }
}

async fn list_nodes(client: &CliClient, output: &OutputFormat) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;

    if state.nodes.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"nodes": [], "count": 0})),
            OutputFormat::Text => print_error("No nodes connected"),
        }
        return Ok(());
    }

    let now = chrono::Utc::now();

    match output {
        OutputFormat::Json => {
            let nodes: Vec<_> = state.nodes.iter().map(|n| {
                let age_seconds = (now - n.last_update).num_seconds();
                let status = if age_seconds < 60 { "active" } else if age_seconds < 120 { "warning" } else { "inactive" };
                json!({
                    "node_id": n.node_id,
                    "node_id_short": format_short_id(&n.node_id),
                    "machine_name": n.machine_name,
                    "os_details": n.os_details,
                    "agent_count": n.discovered_agents.len(),
                    "status": status,
                    "last_seen_seconds": age_seconds
                })
            }).collect();
            print_json(&json!({"nodes": nodes, "count": nodes.len()}));
        }
        OutputFormat::Text => {
            print_header("Connected Nodes");
            println!();
            for node in &state.nodes {
                let age_seconds = (now - node.last_update).num_seconds();
                let status = if age_seconds < 60 {
                    format_status("active")
                } else if age_seconds < 120 {
                    format_status("warning")
                } else {
                    format_status("inactive")
                };

                println!(
                    "  {} {} ({}) - {} agents [{}]",
                    format_short_id(&node.node_id),
                    node.machine_name,
                    node.os_details,
                    node.discovered_agents.len(),
                    status
                );
            }
            println!();
            print_success(&format!("{} node(s) connected", state.nodes.len()));
        }
    }

    Ok(())
}

async fn select_node(client: &CliClient, prefix: &str, output: &OutputFormat) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;

    let search = prefix.to_lowercase();
    let found = state.nodes.iter().find(|n| n.node_id.to_lowercase().starts_with(&search));

    match found {
        Some(node) => {
            match output {
                OutputFormat::Json => {
                    print_json(&json!({
                        "status": "success",
                        "node_id": node.node_id,
                        "node_id_short": format_short_id(&node.node_id),
                        "machine_name": node.machine_name
                    }));
                }
                OutputFormat::Text => {
                    print_success(&format!(
                        "Selected node: {} ({})",
                        format_short_id(&node.node_id),
                        node.machine_name
                    ));
                }
            }
            Ok(())
        }
        None => {
            match output {
                OutputFormat::Json => {
                    print_json(&json!({"status": "error", "message": format!("No node found matching '{}'", prefix)}));
                }
                OutputFormat::Text => {
                    print_error(&format!("No node found matching '{}'", prefix));
                }
            }
            Err(anyhow!("Node not found"))
        }
    }
}
