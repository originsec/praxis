use anyhow::{Result, anyhow};
use clap::Subcommand;
use common::{
    AgentCommand as NodeAgentCommand, AgentCommandResult, AgentFileType as NodeFileType,
    NodeCommand as NodeCmd, NodeCommandResult,
};

use crate::client::Client;
use crate::output::{format_short_id, print_header, print_success};

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

pub async fn execute(client: &Client, command: AgentCommand) -> Result<()> {
    match command {
        AgentCommand::List { node } => list_agents(client, &node).await,
        AgentCommand::Select { node, short_name } => select_agent(client, &node, &short_name).await,
        AgentCommand::Update { node } => update_agent(client, &node).await,
        AgentCommand::Config { command } => match command {
            AgentConfigCommand::Read {
                node,
                path,
                line_start,
                line_end,
            } => {
                read_file(
                    client,
                    &node,
                    NodeFileType::Config,
                    &path,
                    line_start,
                    line_end,
                )
                .await
            }
            AgentConfigCommand::Write {
                node,
                path,
                contents,
            } => write_file(client, &node, NodeFileType::Config, &path, &contents).await,
            AgentConfigCommand::Grep {
                node,
                path,
                pattern,
            } => grep_file(client, &node, NodeFileType::Config, &path, &pattern).await,
        },
        AgentCommand::Session { command } => match command {
            AgentSessionCommand::Read {
                node,
                session_file,
                line_start,
                line_end,
            } => {
                read_file(
                    client,
                    &node,
                    NodeFileType::Session,
                    &session_file,
                    line_start,
                    line_end,
                )
                .await
            }
            AgentSessionCommand::Grep {
                node,
                session_file,
                pattern,
            } => {
                grep_file(
                    client,
                    &node,
                    NodeFileType::Session,
                    &session_file,
                    &pattern,
                )
                .await
            }
        },
    }
}

fn find_node_id(state: &common::SystemState, prefix: &str) -> Option<String> {
    let search = prefix.to_lowercase();
    state
        .nodes
        .iter()
        .find(|node| node.node_id.to_lowercase().starts_with(&search))
        .map(|node| node.node_id.clone())
}

async fn list_agents(client: &Client, node_prefix: &str) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;

    let node = state
        .nodes
        .iter()
        .find(|node| {
            node.node_id
                .to_lowercase()
                .starts_with(&node_prefix.to_lowercase())
        })
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    if node.discovered_agents.is_empty() {
        println!("No agents discovered on this node");
        return Ok(());
    }

    print_header(&format!(
        "Agents on {} ({})",
        format_short_id(&node.node_id),
        node.machine_name
    ));
    println!();

    for agent in &node.discovered_agents {
        let version = agent
            .version
            .as_deref()
            .map(|version| format!(" {}", version))
            .unwrap_or_default();
        println!("  {} - {}{}", agent.short_name, agent.name, version);
    }

    println!();
    print_success(&format!(
        "{} agent(s) discovered",
        node.discovered_agents.len()
    ));
    Ok(())
}

async fn select_agent(client: &Client, node_prefix: &str, short_name: &str) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Agent(NodeAgentCommand::Select {
        short_name: short_name.to_string(),
    });
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::Selected { short_name }) => {
            print_success(&format!("Selected agent: {}", short_name));
            Ok(())
        }
        NodeCommandResult::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn update_agent(client: &Client, node_prefix: &str) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Agent(NodeAgentCommand::Update);
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::UpdateSent) => {
            print_success("Update request sent");
            Ok(())
        }
        NodeCommandResult::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn read_file(
    client: &Client,
    node_prefix: &str,
    file_type: NodeFileType,
    path: &str,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
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
            file_type,
            path,
            content,
            line_start,
            line_end,
            error,
        }) => {
            if let Some(error) = error {
                return Err(anyhow!(error));
            }

            let title = match file_type {
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
            Ok(())
        }
        NodeCommandResult::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn write_file(
    client: &Client,
    node_prefix: &str,
    file_type: NodeFileType,
    path: &str,
    contents: &str,
) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
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
            success, error, ..
        }) => {
            if success {
                print_success("Write complete");
                Ok(())
            } else {
                Err(anyhow!(error.unwrap_or_else(|| "Write failed".to_string())))
            }
        }
        NodeCommandResult::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn grep_file(
    client: &Client,
    node_prefix: &str,
    file_type: NodeFileType,
    path: &str,
    pattern: &str,
) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Agent(NodeAgentCommand::GrepFiles {
        file_type,
        paths: vec![path.to_string()],
        pattern: pattern.to_string(),
    });
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::GrepFilesResult {
            file_type,
            pattern,
            results,
            ..
        }) => {
            let title = match file_type {
                NodeFileType::Config => "Config Grep Results",
                NodeFileType::Session => "Session Grep Results",
            };
            let entry = results.first();

            if let Some(result) = entry {
                if let Some(error) = &result.error {
                    return Err(anyhow!(error.clone()));
                }
            }

            print_header(title);
            println!();
            println!("  Path: {}", path);
            println!("  Pattern: {}", pattern);
            if let Some(result) = entry {
                println!("  Matches: {}", result.matches.len());
                println!();
                for matched in &result.matches {
                    println!("  {:>6}: {}", matched.line_number, matched.line_content);
                }
            } else {
                println!("  Matches: 0");
            }
            println!();
            print_success("Grep complete");
            Ok(())
        }
        NodeCommandResult::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("Unexpected response")),
    }
}
