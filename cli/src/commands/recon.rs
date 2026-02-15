use anyhow::{anyhow, Result};
use clap::{Subcommand, ValueEnum};
use common::{
    AgentCommand as NodeAgentCommand, AgentCommandResult, AgentFileType as NodeFileType,
    NodeCommand as NodeCmd, NodeCommandResult,
};
use serde_json::json;

use crate::client::CliClient;
use crate::output::{print_header, print_json, print_success, OutputFormat};
use crate::spinner::Spinner;

#[derive(Subcommand)]
pub enum ReconCommand {
    /// Run static reconnaissance
    Run {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,
    },

    /// Run semantic reconnaissance (includes internal tools)
    RunSemantic {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,
    },

    /// List stored recon data (all sections, or a specific section)
    List {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,

        /// Agent short name (defaults to selected agent)
        #[arg(short, long)]
        agent: Option<String>,

        /// Section to list (all if omitted)
        section: Option<ReconListSection>,
    },

    /// Read config content from a file discovered by recon
    ConfigRead {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,

        /// Path to the config file (omit to read all)
        path: Option<String>,

        #[arg(long)]
        line_start: Option<usize>,

        #[arg(long)]
        line_end: Option<usize>,
    },

    /// Read session content from a file discovered by recon
    SessionRead {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,

        /// Path to the session file (omit to read all)
        path: Option<String>,

        #[arg(long)]
        line_start: Option<usize>,

        #[arg(long)]
        line_end: Option<usize>,
    },

    /// Grep config content with regex
    ConfigGrep {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,

        /// Regex pattern to search for
        pattern: String,

        /// Path to the config file (omit to grep all)
        path: Option<String>,
    },

    /// Grep session content with regex
    SessionGrep {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,

        /// Regex pattern to search for
        pattern: String,

        /// Path to the session file (omit to grep all)
        path: Option<String>,
    },
}

#[derive(Clone, ValueEnum)]
pub enum ReconListSection {
    All,
    Sessions,
    Tools,
    Projects,
    Configs,
}

pub async fn execute(client: &mut CliClient, command: ReconCommand, output: &OutputFormat) -> Result<()> {
    match command {
        ReconCommand::Run { node } => recon_run(client, &node, false, output).await,
        ReconCommand::RunSemantic { node } => recon_run(client, &node, true, output).await,
        ReconCommand::List { node, agent, section } => {
            recon_list(client, &node, agent.as_deref(), section.as_ref(), output).await
        }
        ReconCommand::ConfigRead { node, path, line_start, line_end } => {
            match path {
                Some(p) => read_file(client, &node, NodeFileType::Config, &p, line_start, line_end, output).await,
                None => read_all(client, &node, NodeFileType::Config, line_start, line_end, output).await,
            }
        }
        ReconCommand::SessionRead { node, path, line_start, line_end } => {
            match path {
                Some(p) => read_file(client, &node, NodeFileType::Session, &p, line_start, line_end, output).await,
                None => read_all(client, &node, NodeFileType::Session, line_start, line_end, output).await,
            }
        }
        ReconCommand::ConfigGrep { node, pattern, path } => {
            match path {
                Some(p) => grep_file(client, &node, NodeFileType::Config, &p, &pattern, output).await,
                None => grep_all(client, &node, NodeFileType::Config, &pattern, output).await,
            }
        }
        ReconCommand::SessionGrep { node, pattern, path } => {
            match path {
                Some(p) => grep_file(client, &node, NodeFileType::Session, &p, &pattern, output).await,
                None => grep_all(client, &node, NodeFileType::Session, &pattern, output).await,
            }
        }
    }
}

fn find_node_id(state: &common::SystemState, prefix: &str) -> Option<String> {
    let search = prefix.to_lowercase();
    state.nodes.iter()
        .find(|n| n.node_id.to_lowercase().starts_with(&search))
        .map(|n| n.node_id.clone())
}

fn resolve_agent_short_name(
    state: &common::SystemState,
    node_id: &str,
    agent: Option<&str>,
) -> Option<String> {
    if let Some(a) = agent {
        return Some(a.to_string());
    }
    state
        .nodes
        .iter()
        .find(|n| n.node_id == node_id)
        .and_then(|n| n.selected_agent.as_ref())
        .map(|a| a.short_name.clone())
}

async fn recon_run(client: &CliClient, node_prefix: &str, semantic: bool, output: &OutputFormat) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix).ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = if semantic {
        NodeCmd::Agent(NodeAgentCommand::ReconSemantic)
    } else {
        NodeCmd::Agent(NodeAgentCommand::Recon)
    };

    let spinner = if matches!(output, OutputFormat::Text) {
        let msg = if semantic { "Running semantic recon..." } else { "Running recon..." };
        Some(Spinner::start(msg))
    } else {
        None
    };
    let response = client.send_command(&node_id, cmd).await;
    if let Some(s) = spinner { s.finish().await; }
    let response = response?;

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
                        "project_paths": result.project_paths.len()
                    }));
                }
                OutputFormat::Text => {
                    let recon_type = if semantic { "Semantic recon" } else { "Recon" };
                    print_header(&format!("{} Summary", recon_type));
                    println!();
                    println!("  MCP Servers: {} ({} tools)", result.tools.mcp_servers.len(), mcp_tools_count);
                    println!("  Skills: {}", result.tools.skills.len());
                    if semantic {
                        println!("  Internal Tools: {}", result.tools.internal_tools.len());
                    }
                    println!("  Config Items: {}", result.config.len());
                    println!("  Sessions: {}", result.sessions.len());
                    println!("  Project Paths: {}", result.project_paths.len());
                    println!();
                    print_success(&format!("{} complete. Use 'recon list' for details.", recon_type));
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

async fn recon_list(
    client: &CliClient,
    node_prefix: &str,
    agent: Option<&str>,
    section: Option<&ReconListSection>,
    output: &OutputFormat,
) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;
    let agent_name = resolve_agent_short_name(&state, &node_id, agent)
        .ok_or_else(|| anyhow!("No agent selected and --agent not provided"))?;

    let spinner = if matches!(output, OutputFormat::Text) {
        Some(Spinner::start("Fetching recon data..."))
    } else {
        None
    };
    let recon = client
        .get_recon_result(&node_id, &agent_name)
        .await;
    if let Some(s) = spinner { s.finish().await; }
    let recon = recon?
        .ok_or_else(|| anyhow!("No stored recon for {}:{}", node_prefix, agent_name))?;

    let show_all = section.is_none() || matches!(section, Some(ReconListSection::All));

    match output {
        OutputFormat::Json => {
            let mut result = json!({});
            if show_all || matches!(section, Some(ReconListSection::Tools)) {
                let mcp_tools_count: usize = recon.tools.mcp_servers.iter().map(|s| s.tools.len()).sum();
                result["mcp_servers"] = json!(recon.tools.mcp_servers.len());
                result["mcp_tools"] = json!(mcp_tools_count);
                result["skills"] = json!(recon.tools.skills.len());
                result["internal_tools"] = json!(recon.tools.internal_tools.len());
            }
            if show_all || matches!(section, Some(ReconListSection::Sessions)) {
                let sessions: Vec<_> = recon.sessions.iter().map(|s| {
                    json!({
                        "session_id": s.session_id,
                        "session_file": s.session_file,
                        "context_path": s.context_path,
                        "last_modified": s.last_modified,
                        "message_count": s.message_count
                    })
                }).collect();
                result["sessions"] = json!(sessions);
            }
            if show_all || matches!(section, Some(ReconListSection::Projects)) {
                result["projects"] = json!(recon.project_paths);
            }
            if show_all || matches!(section, Some(ReconListSection::Configs)) {
                let configs: Vec<_> = recon.config.iter().map(|c| {
                    json!({"path": c.path, "config_type": c.config_type})
                }).collect();
                result["configs"] = json!(configs);
            }
            print_json(&result);
        }
        OutputFormat::Text => {
            print_header("Stored Recon Data");
            println!();

            if show_all || matches!(section, Some(ReconListSection::Tools)) {
                let mcp_tools_count: usize = recon.tools.mcp_servers.iter().map(|s| s.tools.len()).sum();
                println!("  MCP Servers: {} ({} tools)", recon.tools.mcp_servers.len(), mcp_tools_count);
                for server in &recon.tools.mcp_servers {
                    println!("    {} ({})", server.name, server.transport);
                    if !server.tools.is_empty() {
                        let names: Vec<&str> = server.tools.iter().map(|t| t.name.as_str()).collect();
                        println!("      {}", names.join(", "));
                    }
                }

                println!("  Skills: {}", recon.tools.skills.len());
                if !recon.tools.skills.is_empty() {
                    let names: Vec<String> = recon.tools.skills.iter()
                        .map(|s| format!("/{}", s.name))
                        .collect();
                    println!("    {}", names.join(", "));
                }

                if !recon.tools.internal_tools.is_empty() {
                    println!("  Internal Tools: {}", recon.tools.internal_tools.len());
                    let names: Vec<&str> = recon.tools.internal_tools.iter()
                        .map(|t| t.name.as_str())
                        .collect();
                    println!("    {}", names.join(", "));
                }
                println!();
            }

            if show_all || matches!(section, Some(ReconListSection::Configs)) {
                println!("  Config Items: {}", recon.config.len());
                for item in &recon.config {
                    println!("    {} ({})", item.path, item.config_type);
                }
                println!();
            }

            if show_all || matches!(section, Some(ReconListSection::Sessions)) {
                println!("  Sessions: {}", recon.sessions.len());
                for s in &recon.sessions {
                    println!("    {} ({} msgs)", s.session_id, s.message_count);
                    println!("      {}", s.session_file);
                }
                println!();
            }

            if show_all || matches!(section, Some(ReconListSection::Projects)) {
                println!("  Project Paths: {}", recon.project_paths.len());
                for path in &recon.project_paths {
                    println!("    {}", path);
                }
                println!();
            }

            print_success("Listed from stored recon");
        }
    }
    Ok(())
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
    let spinner = if matches!(output, OutputFormat::Text) {
        Some(Spinner::start("Reading file..."))
    } else {
        None
    };
    let response = client.send_command(&node_id, cmd).await;
    if let Some(s) = spinner { s.finish().await; }
    let response = response?;

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
    let spinner = if matches!(output, OutputFormat::Text) {
        Some(Spinner::start("Searching..."))
    } else {
        None
    };
    let response = client.send_command(&node_id, cmd).await;
    if let Some(s) = spinner { s.finish().await; }
    let response = response?;

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

//
// Resolve all paths for a file type from stored recon data.
//

async fn resolve_all_paths(
    client: &CliClient,
    node_prefix: &str,
    file_type: NodeFileType,
) -> Result<Vec<String>> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;
    let agent_name = resolve_agent_short_name(&state, &node_id, None)
        .ok_or_else(|| anyhow!("No agent selected"))?;

    let recon = client
        .get_recon_result(&node_id, &agent_name)
        .await?
        .ok_or_else(|| anyhow!("No stored recon data — run recon first"))?;

    let paths = match file_type {
        NodeFileType::Config => recon.config.iter().map(|c| c.path.clone()).collect(),
        NodeFileType::Session => recon.sessions.iter().map(|s| s.session_file.clone()).collect(),
    };
    Ok(paths)
}

async fn read_all(
    client: &CliClient,
    node_prefix: &str,
    file_type: NodeFileType,
    line_start: Option<usize>,
    line_end: Option<usize>,
    output: &OutputFormat,
) -> Result<()> {
    let paths = resolve_all_paths(client, node_prefix, file_type).await?;
    if paths.is_empty() {
        return Err(anyhow!("No files found in recon data"));
    }
    for path in &paths {
        read_file(client, node_prefix, file_type, path, line_start, line_end, output).await?;
    }
    Ok(())
}

async fn grep_all(
    client: &CliClient,
    node_prefix: &str,
    file_type: NodeFileType,
    pattern: &str,
    output: &OutputFormat,
) -> Result<()> {
    let paths = resolve_all_paths(client, node_prefix, file_type).await?;
    if paths.is_empty() {
        return Err(anyhow!("No files found in recon data"));
    }

    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let spinner = if matches!(output, OutputFormat::Text) {
        Some(Spinner::start("Searching all files..."))
    } else {
        None
    };

    //
    // Collect results, keeping only files with matches.
    //

    let mut results = Vec::new();
    for path in &paths {
        let cmd = NodeCmd::Agent(NodeAgentCommand::GrepFile {
            file_type,
            path: path.to_string(),
            pattern: pattern.to_string(),
        });
        let response = client.send_command(&node_id, cmd).await?;
        if let NodeCommandResult::Agent(AgentCommandResult::GrepFileResult {
            path, matches, error, ..
        }) = response.result {
            if error.is_none() && !matches.is_empty() {
                results.push((path, matches));
            }
        }
    }

    if let Some(s) = spinner { s.finish().await; }

    let title = match file_type {
        NodeFileType::Config => "Config Grep Results",
        NodeFileType::Session => "Session Grep Results",
    };

    match output {
        OutputFormat::Json => {
            let entries: Vec<_> = results.iter().map(|(path, matches)| {
                json!({ "path": path, "matches": matches, "match_count": matches.len() })
            }).collect();
            print_json(&json!({
                "pattern": pattern,
                "files_with_matches": entries.len(),
                "results": entries,
            }));
        }
        OutputFormat::Text => {
            print_header(title);
            println!();
            println!("  Pattern: {}", pattern);
            println!("  Files with matches: {} / {}", results.len(), paths.len());

            for (path, matches) in &results {
                println!();
                println!("  {} ({} matches)", path, matches.len());
                for m in matches {
                    println!("  {:>6}: {}", m.line_number, m.line_content);
                }
            }

            println!();
            print_success("Grep complete");
        }
    }
    Ok(())
}
