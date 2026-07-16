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
        let line = format_intercept_status_line(node);
        println!(
            "  {} {}: {}",
            format_short_id(&node.node_id),
            node.machine_name,
            line
        );
    }
    Ok(())
}

/// Pure formatting for non-interactive `intercept status` (unit-tested).
pub fn format_intercept_status_line(node: &common::NodeState) -> String {
    if let Some(ref st) = node.intercept_status {
        return format_retained_intercept_status(st);
    }
    if node.intercept_active {
        "enabled".to_string()
    } else {
        "disabled".to_string()
    }
}

fn format_retained_intercept_status(st: &common::InterceptStatus) -> String {
    if st.cleanup_required {
        let base = if st.enabled {
            "cleanup-required (partially active)"
        } else {
            "cleanup-required"
        };
        return base.to_string();
    }
    if !st.enabled {
        return "disabled".to_string();
    }
    let method = st
        .method
        .map(|m| format!("{:?}", m).to_lowercase())
        .unwrap_or_else(|| "on".into());
    let port = st
        .proxy_port
        .map(|p| format!(":{}", p))
        .unwrap_or_default();
    let domains = if st.intercepted_domains.is_empty() {
        String::new()
    } else {
        format!(" ({} domains)", st.intercepted_domains.len())
    };
    format!("enabled {}{}{}", method, port, domains)
}

#[cfg(test)]
mod tests {
    use super::{format_intercept_status_line, format_retained_intercept_status};
    use common::{InterceptMethod, InterceptStatus, NodeState, NodeStatus};
    use chrono::Utc;

    fn bare_node(active: bool) -> NodeState {
        NodeState {
            node_id: "n1".into(),
            node_type: "native".into(),
            capabilities: vec![],
            machine_name: "host".into(),
            os_details: String::new(),
            discovered_agents: vec![],
            selected_agent: None,
            intercept_active: active,
            intercept_supported: true,
            intercept_status: None,
            last_update: Utc::now(),
            status: NodeStatus::Online,
            active_terminal_id: None,
            privileged: false,
        }
    }

    #[test]
    fn status_line_shows_cleanup_required_from_retained_status() {
        let mut node = bare_node(true);
        node.intercept_status = Some(InterceptStatus {
            node_id: "n1".into(),
            enabled: true,
            method: Some(InterceptMethod::Proxy),
            proxy_port: Some(8080),
            intercepted_domains: vec!["a.com".into()],
            cleanup_required: true,
        });
        let line = format_intercept_status_line(&node);
        assert!(
            line.contains("cleanup"),
            "expected cleanup in status line, got {line}"
        );
    }

    #[test]
    fn status_line_shows_method_and_port_when_enabled() {
        let st = InterceptStatus {
            node_id: "n1".into(),
            enabled: true,
            method: Some(InterceptMethod::Vpn),
            proxy_port: Some(9090),
            intercepted_domains: vec!["x".into(), "y".into()],
            cleanup_required: false,
        };
        let line = format_retained_intercept_status(&st);
        assert!(line.contains("vpn"), "{line}");
        assert!(line.contains("9090"), "{line}");
        assert!(line.contains("2 domains"), "{line}");
    }

    #[test]
    fn status_line_falls_back_to_boolean_when_no_retained() {
        assert_eq!(format_intercept_status_line(&bare_node(true)), "enabled");
        assert_eq!(format_intercept_status_line(&bare_node(false)), "disabled");
    }
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
