use anyhow::{anyhow, Result};
use clap::Subcommand;
use common::{
    AgentCommand as NodeAgentCommand, AgentCommandResult, AgentFileType as NodeFileType,
    NodeCommand as NodeCmd, NodeCommandResult,
};
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

    /// Config content operations
    Config {
        #[command(subcommand)]
        command: AgentConfigCommand,
    },

    /// Session content operations
    Session {
        #[command(subcommand)]
        command: AgentSessionCommand,
    },
}

#[derive(Subcommand)]
pub enum AgentConfigCommand {
    /// Read config content from a file
    Read {
        #[arg(short, long)]
        node: String,
        path: String,
        #[arg(long)]
        line_start: Option<usize>,
        #[arg(long)]
        line_end: Option<usize>,
    },
    /// Write config content to a file
    Write {
        #[arg(short, long)]
        node: String,
        path: String,
        contents: String,
    },
    /// Grep config content in a file with regex
    Grep {
        #[arg(short, long)]
        node: String,
        path: String,
        pattern: String,
    },
}

#[derive(Subcommand)]
pub enum AgentSessionCommand {
    /// Read session content
    Read {
        #[arg(short, long)]
        node: String,
        session_file: String,
        #[arg(long)]
        line_start: Option<usize>,
        #[arg(long)]
        line_end: Option<usize>,
    },
    /// Grep session content with regex
    Grep {
        #[arg(short, long)]
        node: String,
        session_file: String,
        pattern: String,
    },
}

pub async fn execute(client: &mut CliClient, command: AgentCommand, output: &OutputFormat) -> Result<()> {
    match command {
        AgentCommand::List { node } => list_agents(client, &node, output).await,
        AgentCommand::Select { node, short_name } => select_agent(client, &node, &short_name, output).await,
        AgentCommand::Update { node } => update_agent(client, &node, output).await,
        AgentCommand::Config { command } => match command {
            AgentConfigCommand::Read { node, path, line_start, line_end } => {
                read_file(client, &node, NodeFileType::Config, &path, line_start, line_end, output).await
            }
            AgentConfigCommand::Write { node, path, contents } => {
                write_file(client, &node, NodeFileType::Config, &path, &contents, output).await
            }
            AgentConfigCommand::Grep { node, path, pattern } => {
                grep_file(client, &node, NodeFileType::Config, &path, &pattern, output).await
            }
        },
        AgentCommand::Session { command } => match command {
            AgentSessionCommand::Read { node, session_file, line_start, line_end } => {
                read_file(client, &node, NodeFileType::Session, &session_file, line_start, line_end, output).await
            }
            AgentSessionCommand::Grep { node, session_file, pattern } => {
                grep_file(client, &node, NodeFileType::Session, &session_file, &pattern, output).await
            }
        },
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
                    "available": a.available,
                    "version": a.version
                })
            }).collect();
            print_json(&json!({"agents": agents, "count": agents.len()}));
        }
        OutputFormat::Text => {
            print_header(&format!("Agents on {} ({})", format_short_id(&node.node_id), node.machine_name));
            println!();
            for agent in &node.discovered_agents {
                let version_suffix = agent.version.as_deref().map(|v| format!(" {}", v)).unwrap_or_default();
                println!("  {} - {}{}", agent.short_name, agent.name, version_suffix);
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
            if matches!(output, OutputFormat::Json) {
                print_json(&json!({"status": "error", "message": message}));
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
            if matches!(output, OutputFormat::Json) {
                print_json(&json!({"status": "error", "message": message}));
            }
            Err(anyhow!("{}", message))
        }
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn read_file(
    client: &CliClient,
    node_prefix: &str,
    file_type: NodeFileType,
    path: &str,
    line_start: Option<usize>,
    line_end: Option<usize>,
    output: &OutputFormat,
) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Agent(NodeAgentCommand::ReadFile {
        file_type,
        path: path.to_string(),
        line_start,
        line_end,
    });
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::ReadFileResult {
            file_type: result_file_type,
            path,
            content,
            line_start,
            line_end,
            error,
        }) => {
            match output {
                OutputFormat::Json => {
                    print_json(&json!({
                        "file_type": format!("{:?}", result_file_type),
                        "path": path,
                        "content": content,
                        "line_start": line_start,
                        "line_end": line_end,
                        "error": error
                    }));
                }
                OutputFormat::Text => {
                    if let Some(error) = error {
                        return Err(anyhow!(error));
                    }
                    let title = match result_file_type {
                        NodeFileType::Config => "Config Content",
                        NodeFileType::Session => "Session Content",
                    };
                    print_header(title);
                    println!();
                    println!("  Path: {}", path);
                    if line_start.is_some() || line_end.is_some() {
                        println!("  Lines: {:?}..{:?}", line_start, line_end);
                    }
                    println!();
                    if let Some(content) = content {
                        println!("{}", content);
                    }
                    println!();
                    print_success("Read complete");
                }
            }
            Ok(())
        }
        NodeCommandResult::Error { message } => {
            if matches!(output, OutputFormat::Json) {
                print_json(&json!({"status": "error", "message": message}));
            }
            Err(anyhow!("{}", message))
        }
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn write_file(
    client: &CliClient,
    node_prefix: &str,
    file_type: NodeFileType,
    path: &str,
    contents: &str,
    output: &OutputFormat,
) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Agent(NodeAgentCommand::WriteFile {
        file_type,
        path: path.to_string(),
        contents: contents.to_string(),
    });
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::WriteFileResult {
            file_type: result_file_type,
            path,
            success,
            error,
        }) => {
            match output {
                OutputFormat::Json => {
                    print_json(&json!({
                        "file_type": format!("{:?}", result_file_type),
                        "path": path,
                        "success": success,
                        "error": error
                    }));
                }
                OutputFormat::Text => {
                    if success {
                        print_success("Write complete");
                    } else {
                        let msg = error.unwrap_or_else(|| "Write failed".to_string());
                        return Err(anyhow!(msg));
                    }
                }
            }
            Ok(())
        }
        NodeCommandResult::Error { message } => {
            if matches!(output, OutputFormat::Json) {
                print_json(&json!({"status": "error", "message": message}));
            }
            Err(anyhow!("{}", message))
        }
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn grep_file(
    client: &CliClient,
    node_prefix: &str,
    file_type: NodeFileType,
    path: &str,
    pattern: &str,
    output: &OutputFormat,
) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Agent(NodeAgentCommand::GrepFile {
        file_type,
        path: path.to_string(),
        pattern: pattern.to_string(),
    });
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::GrepFileResult {
            file_type: result_file_type,
            path,
            pattern,
            matches,
            error,
        }) => {
            match output {
                OutputFormat::Json => {
                    print_json(&json!({
                        "file_type": format!("{:?}", result_file_type),
                        "path": path,
                        "pattern": pattern,
                        "matches": matches,
                        "match_count": matches.len(),
                        "error": error
                    }));
                }
                OutputFormat::Text => {
                    if let Some(error) = error {
                        return Err(anyhow!(error));
                    }
                    let title = match result_file_type {
                        NodeFileType::Config => "Config Grep Results",
                        NodeFileType::Session => "Session Grep Results",
                    };
                    print_header(title);
                    println!();
                    println!("  Path: {}", path);
                    println!("  Pattern: {}", pattern);
                    println!("  Matches: {}", matches.len());
                    println!();
                    for m in matches {
                        println!("  {:>6}: {}", m.line_number, m.line_content);
                    }
                    println!();
                    print_success("Grep complete");
                }
            }
            Ok(())
        }
        NodeCommandResult::Error { message } => {
            if matches!(output, OutputFormat::Json) {
                print_json(&json!({"status": "error", "message": message}));
            }
            Err(anyhow!("{}", message))
        }
        _ => Err(anyhow!("Unexpected response")),
    }
}
