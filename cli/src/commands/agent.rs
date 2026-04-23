use anyhow::{Result, anyhow};
use clap::Subcommand;
use common::AgentFileType as NodeFileType;
use common::acp_ext::{EXT_PRAXIS_GREP_FILES, EXT_PRAXIS_READ_FILE, EXT_PRAXIS_WRITE_FILE};
use serde_json::json;

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
        #[arg(short, long)]
        agent: String,
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
        #[arg(short, long)]
        agent: String,
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
        #[arg(short, long)]
        agent: String,
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
        #[arg(short, long)]
        agent: String,
        session_file: String,
        pattern: String,
    },
}

pub async fn execute(client: &Client, command: AgentCommand) -> Result<()> {
    match command {
        AgentCommand::List { node } => list_agents(client, &node).await,
        AgentCommand::Update { node } => update_agent(client, &node).await,
        AgentCommand::Config { command } => match command {
            AgentConfigCommand::Read {
                node,
                agent,
                path,
                line_start,
                line_end,
            } => {
                read_file(
                    client,
                    &node,
                    &agent,
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
                agent,
                path,
                pattern,
            } => grep_file(client, &node, &agent, NodeFileType::Config, &path, &pattern).await,
        },
        AgentCommand::Session { command } => match command {
            AgentSessionCommand::Read {
                node,
                agent,
                session_file,
                line_start,
                line_end,
            } => {
                read_file(
                    client,
                    &node,
                    &agent,
                    NodeFileType::Session,
                    &session_file,
                    line_start,
                    line_end,
                )
                .await
            }
            AgentSessionCommand::Grep {
                node,
                agent,
                session_file,
                pattern,
            } => {
                grep_file(
                    client,
                    &node,
                    &agent,
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

async fn update_agent(client: &Client, node_prefix: &str) -> Result<()> {
    //
    // Agent info is refreshed automatically via NodeInformationUpdate
    // broadcasts; just report the cached count.
    //

    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node = state
        .nodes
        .iter()
        .find(|n| n.node_id.to_lowercase().starts_with(&node_prefix.to_lowercase()))
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    print_success(&format!(
        "Reporting cached agent info: {} agent(s)",
        node.discovered_agents.len()
    ));
    Ok(())
}

async fn read_file(
    client: &Client,
    node_prefix: &str,
    agent: &str,
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

    let mut params = json!({
        "agent_short_name": agent,
        "file_type": file_type,
        "path": path,
    });
    if let Some(v) = line_start {
        params["line_start"] = json!(v);
    }
    if let Some(v) = line_end {
        params["line_end"] = json!(v);
    }

    let result = client.acp_request(&node_id, EXT_PRAXIS_READ_FILE, params).await?;

    if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!(err.to_string()));
    }

    let title = match file_type {
        NodeFileType::Config => "Config Content",
        NodeFileType::Session => "Session Content",
    };
    print_header(title);
    println!();
    let out_path = result.get("path").and_then(|v| v.as_str()).unwrap_or(path);
    println!("  Path: {}", out_path);
    if line_start.is_some() || line_end.is_some() {
        println!("  Lines: {:?}..{:?}", line_start, line_end);
    }
    println!();
    if let Some(content) = result.get("content").and_then(|v| v.as_str()) {
        println!("{}", content);
    }
    println!();
    print_success("Read complete");
    Ok(())
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

    let result = client
        .acp_request(&node_id, EXT_PRAXIS_WRITE_FILE, json!({
            "file_type": file_type,
            "path": path,
            "contents": contents,
        }))
        .await?;

    let success = result.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    if success {
        print_success("Write complete");
        Ok(())
    } else {
        let err = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Write failed");
        Err(anyhow!(err.to_string()))
    }
}

async fn grep_file(
    client: &Client,
    node_prefix: &str,
    agent: &str,
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

    let result = client
        .acp_request(&node_id, EXT_PRAXIS_GREP_FILES, json!({
            "agent_short_name": agent,
            "file_type": file_type,
            "paths": vec![path.to_string()],
            "pattern": pattern,
        }))
        .await?;

    if result.get("pattern").is_none()
        && let Some(err) = result.get("error").and_then(|v| v.as_str())
    {
        return Err(anyhow!(err.to_string()));
    }

    let title = match file_type {
        NodeFileType::Config => "Config Grep Results",
        NodeFileType::Session => "Session Grep Results",
    };

    let results: Vec<common::GrepFileEntry> = result
        .get("results")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| anyhow!("Failed to parse grep results: {}", e))?
        .unwrap_or_default();

    if let Some(entry) = results.first()
        && let Some(error) = &entry.error
    {
        return Err(anyhow!(error.clone()));
    }

    let result_pattern = result
        .get("pattern")
        .and_then(|v| v.as_str())
        .unwrap_or(pattern);

    print_header(title);
    println!();
    println!("  Path: {}", path);
    println!("  Pattern: {}", result_pattern);
    if let Some(entry) = results.first() {
        println!("  Matches: {}", entry.matches.len());
        println!();
        for matched in &entry.matches {
            println!("  {:>6}: {}", matched.line_number, matched.line_content);
        }
    } else {
        println!("  Matches: 0");
    }
    println!();
    print_success("Grep complete");
    Ok(())
}

