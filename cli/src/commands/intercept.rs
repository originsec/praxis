use anyhow::{Result, anyhow};
use clap::{Subcommand, ValueEnum};
use common::InterceptMethod;

use crate::client::Client;
use crate::output::{format_short_id, print_header, print_success};

#[derive(Clone, Copy, ValueEnum)]
pub enum InterceptMethodArg {
    Proxy,
    Vpn,
    Hosts,
    Tproxy,
}

impl From<InterceptMethodArg> for InterceptMethod {
    fn from(method: InterceptMethodArg) -> Self {
        match method {
            InterceptMethodArg::Proxy => Self::Proxy,
            InterceptMethodArg::Vpn => Self::Vpn,
            InterceptMethodArg::Hosts => Self::Hosts,
            InterceptMethodArg::Tproxy => Self::Tproxy,
        }
    }
}

#[derive(Subcommand)]
pub enum InterceptCommand {
    /// Show interception state for connected nodes
    Status {
        /// Optional node ID prefix
        node: Option<String>,
    },

    /// Enable interception on a node
    Enable {
        /// Node ID prefix
        node: String,
        /// Interception method
        #[arg(long, value_enum, default_value = "proxy")]
        method: InterceptMethodArg,
    },

    /// Disable interception on a node
    Disable {
        /// Node ID prefix
        node: String,
    },
}

pub async fn execute(client: &Client, command: InterceptCommand) -> Result<()> {
    match command {
        InterceptCommand::Status { node } => status(client, node.as_deref()).await,
        InterceptCommand::Enable { node, method } => enable(client, &node, method.into()).await,
        InterceptCommand::Disable { node } => disable(client, &node).await,
    }
}

async fn status(client: &Client, prefix: Option<&str>) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let nodes: Vec<_> = match prefix {
        Some(prefix) => vec![super::find_node(&state, prefix)
            .map_err(|e| anyhow!("Node '{}': {}", prefix, e))?],
        None => state.nodes.iter().collect(),
    };

    print_header("Traffic Interception");
    if nodes.is_empty() {
        println!("No nodes connected");
        return Ok(());
    }
    for node in nodes {
        println!(
            "  {} {}: {}",
            format_short_id(&node.node_id),
            node.machine_name,
            if node.intercept_active {
                "enabled"
            } else {
                "disabled"
            }
        );
    }
    Ok(())
}

async fn enable(client: &Client, prefix: &str, method: InterceptMethod) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node = super::find_node(&state, prefix)
        .map_err(|e| anyhow!("Node '{}': {}", prefix, e))?;
    let node_id = node.node_id.clone();
    let machine_name = node.machine_name.clone();
    client.enable_intercept(node_id, Some(method)).await?;
    print_success(&format!(
        "Interception enabled on {} ({}) via {}",
        format_short_id(&node.node_id),
        machine_name,
        method
    ));
    Ok(())
}

async fn disable(client: &Client, prefix: &str) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node = super::find_node(&state, prefix)
        .map_err(|e| anyhow!("Node '{}': {}", prefix, e))?;
    let node_id = node.node_id.clone();
    let machine_name = node.machine_name.clone();
    client.disable_intercept(node_id).await?;
    print_success(&format!(
        "Interception disabled on {} ({})",
        format_short_id(&node.node_id),
        machine_name
    ));
    Ok(())
}
