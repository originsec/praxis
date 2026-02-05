use anyhow::{anyhow, Result};
use clap::Subcommand;
use common::{AgentCommand as NodeAgentCommand, NodeCommand as NodeCmd, NodeCommandResult, AgentCommandResult};
use serde_json::json;

use crate::client::CliClient;
use crate::output::{format_short_id, print_error, print_header, print_json, print_success, OutputFormat};

#[derive(Subcommand)]
pub enum AgentCommand {
    /// List agents on a node
    List {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,
    },

    /// Select an agent
    Select {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,

        /// Agent short name
        short_name: String,
    },

    /// Request agent info update
    Update {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,
    },

    /// Perform static reconnaissance
    Recon {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,
    },

    /// Perform semantic reconnaissance (includes internal tools)
    ReconSemantic {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,
    },
}

pub async fn execute(client: &mut CliClient, command: AgentCommand, output: &OutputFormat) -> Result<()> {
    match command {
        AgentCommand::List { node } => list_agents(client, &node, output).await,
        AgentCommand::Select { node, short_name } => select_agent(client, &node, &short_name, output).await,
        AgentCommand::Update { node } => update_agent(client, &node, output).await,
        AgentCommand::Recon { node } => recon_agent(client, &node, false, output).await,
        AgentCommand::ReconSemantic { node } => recon_agent(client, &node, true, output).await,
    }
}

fn find_node_id(state: &common::SystemState, prefix: &str) -> Option<String> {
    let search = prefix.to_lowercase();
    state.nodes.iter()
        .find(|n| n.node_id.to_lowercase().starts_with(&search))
        .map(|n| n.node_id.clone())
}

async fn list_agents(client: &CliClient, node_prefix: &str, output: &OutputFormat) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;

    let node = state.nodes.iter()
        .find(|n| n.node_id.to_lowercase().starts_with(&node_prefix.to_lowercase()))
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    if node.discovered_agents.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"agents": [], "count": 0})),
            OutputFormat::Text => print_error("No agents discovered on this node"),
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => {
            let agents: Vec<_> = node.discovered_agents.iter().map(|a| {
                json!({
                    "short_name": a.short_name,
                    "name": a.name,
                    "available": a.available
                })
            }).collect();
            print_json(&json!({"agents": agents, "count": agents.len()}));
        }
        OutputFormat::Text => {
            print_header(&format!("Agents on {} ({})", format_short_id(&node.node_id), node.machine_name));
            println!();
            for agent in &node.discovered_agents {
                let status = if agent.available { "available" } else { "unavailable" };
                println!("  {} - {} [{}]", agent.short_name, agent.name, status);
            }
            println!();
            print_success(&format!("{} agent(s) discovered", node.discovered_agents.len()));
        }
    }

    Ok(())
}

async fn select_agent(client: &CliClient, node_prefix: &str, short_name: &str, output: &OutputFormat) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix).ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Agent(NodeAgentCommand::Select { short_name: short_name.to_string() });
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::Selected { short_name }) => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "success", "short_name": short_name})),
                OutputFormat::Text => print_success(&format!("Selected agent: {}", short_name)),
            }
            Ok(())
        }
        NodeCommandResult::Error { message } => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "error", "message": message})),
                OutputFormat::Text => print_error(&message),
            }
            Err(anyhow!("{}", message))
        }
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn update_agent(client: &CliClient, node_prefix: &str, output: &OutputFormat) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix).ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Agent(NodeAgentCommand::Update);
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::UpdateSent) => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "success", "message": "Update request sent"})),
                OutputFormat::Text => print_success("Update request sent"),
            }
            Ok(())
        }
        NodeCommandResult::Error { message } => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "error", "message": message})),
                OutputFormat::Text => print_error(&message),
            }
            Err(anyhow!("{}", message))
        }
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn recon_agent(client: &CliClient, node_prefix: &str, semantic: bool, output: &OutputFormat) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix).ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = if semantic {
        NodeCmd::Agent(NodeAgentCommand::ReconSemantic)
    } else {
        NodeCmd::Agent(NodeAgentCommand::Recon)
    };

    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::ReconComplete { result }) => {
            let mcp_tools_count: usize = result.tools.mcp_servers.iter().map(|s| s.tools.len()).sum();

            match output {
                OutputFormat::Json => {
                    print_json(&json!({
                        "status": "success",
                        "mcp_servers": result.tools.mcp_servers.len(),
                        "mcp_tools": mcp_tools_count,
                        "skills": result.tools.skills.len(),
                        "internal_tools": result.tools.internal_tools.len(),
                        "config_items": result.config.len(),
                        "sessions": result.sessions.len(),
                        "project_paths": result.project_paths
                    }));
                }
                OutputFormat::Text => {
                    let recon_type = if semantic { "Semantic recon" } else { "Recon" };
                    print_header(&format!("{} Results", recon_type));
                    println!();
                    println!("  MCP Servers: {} ({} tools)", result.tools.mcp_servers.len(), mcp_tools_count);
                    println!("  Skills: {}", result.tools.skills.len());
                    if semantic {
                        println!("  Internal Tools: {}", result.tools.internal_tools.len());
                    }
                    println!("  Config Items: {}", result.config.len());
                    println!("  Sessions: {}", result.sessions.len());
                    println!("  Project Paths: {}", result.project_paths.len());

                    if !result.project_paths.is_empty() {
                        println!();
                        println!("  Projects:");
                        for path in &result.project_paths {
                            println!("    - {}", path);
                        }
                    }

                    println!();
                    print_success(&format!("{} complete", recon_type));
                }
            }
            Ok(())
        }
        NodeCommandResult::Error { message } => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "error", "message": message})),
                OutputFormat::Text => print_error(&message),
            }
            Err(anyhow!("{}", message))
        }
        _ => Err(anyhow!("Unexpected response")),
    }
}
