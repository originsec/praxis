use anyhow::{Result, anyhow};
use clap::Subcommand;

use crate::client::Client;
use crate::output::{format_short_id, print_header, print_success};

#[derive(Subcommand)]
pub enum NodeCommand {
    /// List all connected nodes
    List,

    /// Select a node by ID prefix
    Select {
        /// Node ID prefix
        node: String,
    },

    /// Reset a node (cancel operations, clear state, re-register)
    Reset {
        /// Node ID prefix
        node: String,
    },
}

pub async fn execute(client: &Client, command: NodeCommand) -> Result<()> {
    match command {
        NodeCommand::List => list_nodes(client).await,
        NodeCommand::Select { node } => select_node(client, &node).await,
        NodeCommand::Reset { node } => reset_node(client, &node).await,
    }
}

async fn list_nodes(client: &Client) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;

    if state.nodes.is_empty() {
        println!("No nodes connected");
        return Ok(());
    }

    let now = chrono::Utc::now();
    print_header("Connected Nodes");
    println!();

    for node in &state.nodes {
        let age_seconds = (now - node.last_update).num_seconds();
        let status = if age_seconds < 60 {
            "active"
        } else if age_seconds < 120 {
            "warning"
        } else {
            "inactive"
        };
        let privileged = if node.privileged { " [privileged]" } else { "" };

        println!(
            "  {} {} ({}) - {} agents [{}]{}",
            format_short_id(&node.node_id),
            node.machine_name,
            node.os_details,
            node.discovered_agents.len(),
            status,
            privileged
        );
    }

    println!();
    print_success(&format!("{} node(s) connected", state.nodes.len()));
    Ok(())
}

async fn select_node(client: &Client, prefix: &str) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;

    let node = super::find_node(&state, prefix)
        .map_err(|e| anyhow!("Node '{}': {}", prefix, e))?;

    print_success(&format!(
        "Selected node: {} ({})",
        format_short_id(&node.node_id),
        node.machine_name
    ));
    Ok(())
}

async fn reset_node(client: &Client, prefix: &str) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;

    let node = super::find_node(&state, prefix)
        .map_err(|e| anyhow!("Node '{}': {}", prefix, e))?;

    client.reset_node(&node.node_id).await?;
    print_success(&format!(
        "Reset command sent to node: {} ({})",
        format_short_id(&node.node_id),
        node.machine_name
    ));
    Ok(())
}
