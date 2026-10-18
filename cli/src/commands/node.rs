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

    /// Show endpoint posture: managed policy, telemetry, sandbox, security products
    Posture {
        /// Node ID prefix
        node: String,
        /// Only show this agent (short name)
        #[arg(short, long)]
        agent: Option<String>,
        /// Re-scan the node instead of showing the stored result
        #[arg(short, long)]
        refresh: bool,
        /// Print JSON (includes raw managed-policy contents)
        #[arg(long)]
        json: bool,
    },
}

pub async fn execute(client: &Client, command: NodeCommand) -> Result<()> {
    match command {
        NodeCommand::List => list_nodes(client).await,
        NodeCommand::Select { node } => select_node(client, &node).await,
        NodeCommand::Reset { node } => reset_node(client, &node).await,
        NodeCommand::Posture {
            node,
            agent,
            refresh,
            json,
        } => show_posture(client, &node, agent.as_deref(), refresh, json).await,
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

async fn show_posture(
    client: &Client,
    prefix: &str,
    agent: Option<&str>,
    refresh: bool,
    json: bool,
) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node = super::find_node(&state, prefix)
        .map_err(|e| anyhow!("Node '{}': {}", prefix, e))?;

    let mut lookup = client.fetch_posture(&node.node_id, refresh).await?;
    if lookup.posture.is_none() && lookup.error.is_none() && !refresh {
        lookup = client.fetch_posture(&node.node_id, true).await?;
    }
    let Some(posture) = lookup.posture.as_ref() else {
        return Err(anyhow!(
            "{}",
            lookup
                .error
                .unwrap_or_else(|| "No posture available for this node".to_string())
        ));
    };
    let posture = posture.filtered(agent, json);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "node_id": node.node_id,
                "performed_at": lookup.performed_at,
                "error": lookup.error,
                "posture": posture,
            }))?
        );
        return Ok(());
    }

    print_header(&format!(
        "Posture: {} ({})",
        format_short_id(&node.node_id),
        node.machine_name
    ));
    if let Some(at) = &lookup.performed_at {
        println!("  Scanned: {}", at);
    }
    if let Some(err) = &lookup.error {
        println!("  Refresh failed (showing stored result): {}", err);
    }

    println!();
    println!("Host security products ({}):", posture.host.os);
    if posture.host.signatures_evaluated == 0 {
        println!("  (no signatures were evaluated)");
    } else if posture.host.security_products.is_empty() {
        println!(
            "  none detected ({} signatures checked)",
            posture.host.signatures_evaluated
        );
    }
    for p in &posture.host.security_products {
        println!(
            "  {} [{}] {} - {}",
            p.name,
            p.category,
            if p.running { "running" } else { "installed" },
            p.evidence.join("; ")
        );
    }

    for a in &posture.agents {
        println!();
        println!("{} ({})", a.agent_name, a.agent_short_name);
        println!(
            "  Managed policy: {}",
            if a.managed_policy {
                "PRESENT - user-level edits to enforced keys will not take effect"
            } else {
                "none found"
            }
        );
        for src in a.policy_sources.iter().filter(|s| s.present) {
            println!("    {}", src.location);
            if let Some(err) = &src.error {
                println!("      error: {}", err);
            }
            for k in &src.keys {
                println!("      [{}] {} = {}", k.category, k.key, k.summary);
            }
        }
        let telemetry = match a.telemetry.enabled {
            Some(true) => "enabled",
            Some(false) => "disabled",
            None => "not configured",
        };
        print!("  Telemetry: {}", telemetry);
        if !a.telemetry.endpoints.is_empty() {
            print!(" -> {}", a.telemetry.endpoints.join(", "));
        }
        println!();
        for st in &a.settings {
            println!(
                "    {} [{}] {} = {} ({}: {})",
                if st.effective { "*" } else { " " },
                st.category.as_str(),
                st.key,
                st.value,
                st.scope.as_str(),
                st.source
            );
        }
        for note in &a.notes {
            println!("  note: {}", note);
        }
    }
    println!();
    println!("* = effective at precedence");
    Ok(())
}
