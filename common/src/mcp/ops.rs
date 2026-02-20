use anyhow::{anyhow, Result};
use std::time::Duration;

use crate::mcp::McpClient;
use crate::{
    AgentCommand, AgentCommandResult, AgentFileType, AgentTool, ChainDefinitionInfo,
    ChainExecutionUpdate, ChainTriggerInfo, ConfigItem, GrepFileEntry, McpServer, NodeCommand,
    NodeCommandResult, OperationDefinitionInfo, SemanticOpUpdate, SessionItem, SystemState,
    TargetSpec, TriggerConfig,
};

//
// Result types returned by shared op functions. Consumers (CLI, MCP server)
// are responsible for formatting these into their respective output formats.
//

pub struct OpAvailableResult {
    pub operations: Vec<OperationDefinitionInfo>,
    pub chains: Vec<ChainDefinitionInfo>,
}

pub enum OpRunResult {
    Operation { id: String, name: String },
    Chain { name: String, execution_id: Option<String> },
}

pub enum OpInfoResult {
    Operation(SemanticOpUpdate),
    Chain(ChainExecutionUpdate),
}

pub enum OpCancelResult {
    Operation { id: String },
    Chain { id: String },
}

pub struct OpListResult {
    pub operations: Vec<SemanticOpUpdate>,
    pub chains: Vec<ChainExecutionUpdate>,
}

//
// Resolve a node ID from a prefix by matching against connected nodes.
//

pub fn resolve_node_id(state: &SystemState, prefix: &str) -> Result<String> {
    state
        .nodes
        .iter()
        .find(|n| {
            n.node_id
                .to_lowercase()
                .starts_with(&prefix.to_lowercase())
        })
        .map(|n| n.node_id.clone())
        .ok_or_else(|| anyhow!("No node found matching '{}'. Use node_list to see connected nodes.", prefix))
}

//
// Resolve the selected agent short name for a node.
//

fn resolve_selected_agent(state: &SystemState, node_id: &str) -> Result<String> {
    state
        .nodes
        .iter()
        .find(|n| n.node_id == node_id)
        .and_then(|n| n.selected_agent.as_ref())
        .map(|a| a.short_name.clone())
        .ok_or_else(|| anyhow!("No agent selected on node. Use agent_select to select one first."))
}

//
// List all available (enabled) operations and chains.
//

pub async fn list_available(client: &(impl McpClient + Sync)) -> Result<OpAvailableResult> {
    client.request_op_def_list().await?;
    client.request_chain_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let operations: Vec<_> = client
        .get_operation_definitions()
        .await
        .into_iter()
        .filter(|op| !op.disabled)
        .collect();

    let chains: Vec<_> = client
        .get_chain_definitions()
        .await
        .into_iter()
        .filter(|c| !c.disabled)
        .collect();

    Ok(OpAvailableResult { operations, chains })
}

//
// Run an operation or chain by name. Tries operation definitions first, then
// falls back to chain definitions using the same resolution logic as the CLI.
//

pub async fn run(
    client: &(impl McpClient + Sync),
    name: &str,
    node_prefix: &str,
    agent: &str,
    working_dir: Option<String>,
) -> Result<OpRunResult> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available. The service may still be starting — try again in a moment."))?;
    let node_id = resolve_node_id(&state, node_prefix)?;

    //
    // Try operation definitions first.
    //

    client.request_op_def_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let op_defs = client.get_operation_definitions().await;
    let operation = op_defs.iter().find(|op| {
        op.full_name.to_lowercase() == name.to_lowercase()
            || op.short_name.to_lowercase() == name.to_lowercase()
            || format!("{}::{}", op.category, op.short_name).to_lowercase()
                == name.to_lowercase()
    });

    if let Some(operation) = operation {
        let operation_id = client
            .run_semantic_op(
                node_id,
                agent.to_string(),
                operation.full_name.clone(),
                working_dir,
            )
            .await?;

        return Ok(OpRunResult::Operation {
            id: operation_id,
            name: operation.name.clone(),
        });
    }

    //
    // Not an operation — try chain definitions.
    //

    client.request_chain_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let chain_defs = client.get_chain_definitions().await;
    let chain = chain_defs.iter().find(|c| {
        c.id.to_lowercase()
            .starts_with(&name.to_lowercase())
            || c.name.to_lowercase() == name.to_lowercase()
    });

    match chain {
        Some(chain) => {
            let chain_id = chain.id.clone();
            let chain_name = chain.name.clone();

            client
                .run_chain(chain_id.clone(), node_id.clone(), agent.to_string(), working_dir)
                .await?;

            //
            // Wait briefly and try to find the execution ID.
            //

            tokio::time::sleep(Duration::from_millis(500)).await;
            client.request_chain_execution_list().await?;
            tokio::time::sleep(Duration::from_millis(300)).await;

            let execs = client.get_chain_executions().await;
            let execution_id = execs
                .iter()
                .filter(|e| e.chain_id == chain_id && e.node_id == node_id)
                .max_by_key(|e| e.started_at)
                .map(|e| e.execution_id.clone());

            Ok(OpRunResult::Chain {
                name: chain_name,
                execution_id,
            })
        }
        None => Err(anyhow!("No operation or chain found matching '{}'. Use op_available to list what's available.", name)),
    }
}

//
// Check status of an operation or chain execution by short ID. Tries semantic
// operations first, then falls back to chain executions.
//

pub async fn get_info(
    client: &(impl McpClient + Sync),
    short_id: &str,
) -> Result<OpInfoResult> {

    //
    // Try semantic operations first.
    //

    client.request_semantic_op_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let ops = client.get_operations().await;
    if let Some(op) = ops.iter().find(|op| op.operation_id.starts_with(short_id)) {
        return Ok(OpInfoResult::Operation(op.clone()));
    }

    //
    // Not an operation — try chain executions.
    //

    client.request_chain_execution_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let execs = client.get_chain_executions().await;
    if let Some(exec) = execs.iter().find(|e| e.execution_id.starts_with(short_id)) {
        return Ok(OpInfoResult::Chain(exec.clone()));
    }

    Err(anyhow!(
        "No operation or chain execution found matching '{}'. Use op_list to see tracked executions.",
        short_id
    ))
}

//
// Cancel a running operation or chain execution by short ID. Tries semantic
// operations first, then falls back to chain executions.
//

pub async fn cancel(
    client: &(impl McpClient + Sync),
    short_id: &str,
) -> Result<OpCancelResult> {
    let ops = client.get_operations().await;
    if let Some(op) = ops.iter().find(|op| op.operation_id.starts_with(short_id)) {
        client.cancel_semantic_op(op.operation_id.clone()).await?;
        return Ok(OpCancelResult::Operation {
            id: short_id.to_string(),
        });
    }

    let execs = client.get_chain_executions().await;
    if let Some(exec) = execs.iter().find(|e| e.execution_id.starts_with(short_id)) {
        client.cancel_chain(exec.execution_id.clone()).await?;
        return Ok(OpCancelResult::Chain {
            id: short_id.to_string(),
        });
    }

    Err(anyhow!(
        "No operation or chain execution found matching '{}'. Use op_list to see tracked executions.",
        short_id
    ))
}

//
// List all tracked (running and recent) operations and chain executions.
//

pub async fn list_tracked(client: &(impl McpClient + Sync)) -> Result<OpListResult> {
    client.request_semantic_op_list().await?;
    client.request_chain_execution_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let operations = client.get_operations().await;
    let chains = client.get_chain_executions().await;

    Ok(OpListResult { operations, chains })
}

//
// Unified recon list — returns stored recon data for a specific section or all.
//

pub struct ReconListResult {
    pub sessions: Option<Vec<SessionItem>>,
    pub projects: Option<Vec<String>>,
    pub mcp_servers: Option<Vec<McpServer>>,
    pub skills: Option<Vec<AgentTool>>,
    pub internal_tools: Option<Vec<AgentTool>>,
    pub configs: Option<Vec<ConfigItem>>,
}

pub async fn recon_list(
    client: &(impl McpClient + Sync),
    node_prefix: &str,
    agent: &str,
    section: Option<&str>,
) -> Result<ReconListResult> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available. The service may still be starting — try again in a moment."))?;
    let node_id = resolve_node_id(&state, node_prefix)?;
    let recon = client
        .get_stored_recon(&node_id, agent)
        .await?
        .ok_or_else(|| anyhow!("No stored recon for {}:{}. Run recon_run first to discover files and tools.", node_prefix, agent))?;

    let show_all = section.is_none() || section == Some("all");

    Ok(ReconListResult {
        sessions: if show_all || section == Some("sessions") {
            Some(recon.sessions)
        } else {
            None
        },
        projects: if show_all || section == Some("projects") {
            Some(recon.project_paths)
        } else {
            None
        },
        mcp_servers: if show_all || section == Some("tools") {
            Some(recon.tools.mcp_servers)
        } else {
            None
        },
        skills: if show_all || section == Some("tools") {
            Some(recon.tools.skills)
        } else {
            None
        },
        internal_tools: if show_all || section == Some("tools") {
            Some(recon.tools.internal_tools)
        } else {
            None
        },
        configs: if show_all || section == Some("configs") {
            Some(recon.config)
        } else {
            None
        },
    })
}

//
// Resolve node prefix, selected agent, and recon data. Returns the resolved
// node ID and the list of known file paths for the given file type.
//

struct ResolvedRecon {
    node_id: String,
    paths: Vec<String>,
}

async fn resolve_recon(
    client: &(impl McpClient + Sync),
    node_prefix: &str,
    file_type: AgentFileType,
) -> Result<ResolvedRecon> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available. The service may still be starting — try again in a moment."))?;
    let node_id = resolve_node_id(&state, node_prefix)?;
    let agent = resolve_selected_agent(&state, &node_id)?;
    let recon = client
        .get_stored_recon(&node_id, &agent)
        .await?
        .ok_or_else(|| anyhow!("No stored recon data. Run recon_run first, then select an agent with agent_select."))?;

    let paths: Vec<String> = match file_type {
        AgentFileType::Config => recon.config.iter().map(|c| c.path.clone()).collect(),
        AgentFileType::Session => recon.sessions.iter().map(|s| s.session_file.clone()).collect(),
    };

    Ok(ResolvedRecon { node_id, paths })
}

fn has_glob_chars(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[')
}

fn validate_paths(recon_paths: &[String], paths: &[String], file_type: AgentFileType) -> Result<()> {
    let type_name = match file_type {
        AgentFileType::Config => "config",
        AgentFileType::Session => "session",
    };
    for path in paths {
        if has_glob_chars(path) {
            continue; // glob paths are validated by the node after expansion
        }
        if !recon_paths.iter().any(|p| p == path) {
            return Err(anyhow!(
                "Path '{}' not found in recon {} files. Use recon_list to see available files.",
                path, type_name
            ));
        }
    }
    Ok(())
}

//
// Read a single file on a node. Resolves node prefix, validates the path
// exists in stored recon data.
//

pub struct ReadFileResult {
    pub path: String,
    pub content: Option<String>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub error: Option<String>,
}

pub async fn recon_read_file(
    client: &(impl McpClient + Sync),
    node_prefix: &str,
    file_type: AgentFileType,
    path: &str,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<ReadFileResult> {
    let resolved = resolve_recon(client, node_prefix, file_type).await?;
    validate_paths(&resolved.paths, &[path.to_string()], file_type)?;
    read_file_inner(client, &resolved.node_id, file_type, path, line_start, line_end).await
}

async fn read_file_inner(
    client: &(impl McpClient + Sync),
    node_id: &str,
    file_type: AgentFileType,
    path: &str,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<ReadFileResult> {
    let cmd = NodeCommand::Agent(AgentCommand::ReadFile {
        file_type,
        path: path.to_string(),
        line_start,
        line_end,
    });
    let response = client.send_command(node_id, cmd).await?;
    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::ReadFileResult {
            path, content, line_start, line_end, error, ..
        }) => Ok(ReadFileResult { path, content, line_start, line_end, error }),
        NodeCommandResult::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("Unexpected response")),
    }
}

//
// Read all files of a given type from stored recon.
//

pub async fn recon_read_all(
    client: &(impl McpClient + Sync),
    node_prefix: &str,
    file_type: AgentFileType,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<Vec<ReadFileResult>> {
    let resolved = resolve_recon(client, node_prefix, file_type).await?;

    if resolved.paths.is_empty() {
        return Err(anyhow!("No files found in recon data. Run recon_run to discover files."));
    }

    let mut results = Vec::new();
    for path in &resolved.paths {
        match read_file_inner(client, &resolved.node_id, file_type, path, line_start, line_end).await {
            Ok(r) => results.push(r),
            Err(e) => results.push(ReadFileResult {
                path: path.clone(),
                content: None,
                line_start,
                line_end,
                error: Some(e.to_string()),
            }),
        }
    }
    Ok(results)
}

//
// Grep files on a node. Sends a single GrepFiles command with all paths.
//

pub struct GrepFilesResult {
    pub pattern: String,
    pub results: Vec<GrepFileEntry>,
    pub errors: Vec<String>,
}

pub async fn recon_grep_file(
    client: &(impl McpClient + Sync),
    node_prefix: &str,
    file_type: AgentFileType,
    paths: &[String],
    pattern: &str,
) -> Result<GrepFilesResult> {
    let resolved = resolve_recon(client, node_prefix, file_type).await?;
    grep_files_inner(client, &resolved.node_id, file_type, paths, pattern).await
}

async fn grep_files_inner(
    client: &(impl McpClient + Sync),
    node_id: &str,
    file_type: AgentFileType,
    paths: &[String],
    pattern: &str,
) -> Result<GrepFilesResult> {
    let cmd = NodeCommand::Agent(AgentCommand::GrepFiles {
        file_type,
        paths: paths.to_vec(),
        pattern: pattern.to_string(),
    });
    let response = client.send_command(node_id, cmd).await?;
    match response.result {
        NodeCommandResult::Agent(AgentCommandResult::GrepFilesResult {
            pattern, results, errors, ..
        }) => Ok(GrepFilesResult { pattern, results, errors }),
        NodeCommandResult::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("Unexpected response")),
    }
}

//
// Grep all files of a given type from stored recon in a single round-trip.
// Returns only files with matches (filters out empty results).
//

pub async fn recon_grep_all(
    client: &(impl McpClient + Sync),
    node_prefix: &str,
    file_type: AgentFileType,
    pattern: &str,
) -> Result<GrepFilesResult> {
    let resolved = resolve_recon(client, node_prefix, file_type).await?;

    if resolved.paths.is_empty() {
        return Err(anyhow!("No files found in recon data. Run recon_run to discover files."));
    }

    let mut result = grep_files_inner(
        client, &resolved.node_id, file_type, &resolved.paths, pattern,
    ).await?;

    // Filter to only files with matches
    result.results.retain(|r| r.error.is_some() || !r.matches.is_empty());
    Ok(result)
}

//
// Chain trigger operations.
//

pub async fn trigger_list(
    client: &(impl McpClient + Sync),
    chain_id: Option<String>,
) -> Result<Vec<ChainTriggerInfo>> {
    client.request_chain_trigger_list(chain_id).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(client.get_chain_triggers().await)
}

pub async fn trigger_create(
    client: &(impl McpClient + Sync),
    chain_name: &str,
    trigger_config: TriggerConfig,
    target_spec: TargetSpec,
) -> Result<String> {
    //
    // Resolve chain by name or ID prefix.
    //
    client.request_chain_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let chains = client.get_chain_definitions().await;
    let chain = chains
        .iter()
        .find(|c| {
            c.id.to_lowercase().starts_with(&chain_name.to_lowercase())
                || c.name.to_lowercase() == chain_name.to_lowercase()
        })
        .ok_or_else(|| anyhow!("No chain found matching '{}'", chain_name))?;
    let chain_id = chain.id.clone();

    client.create_chain_trigger(chain_id.clone(), trigger_config, target_spec).await?;
    Ok(chain_id)
}

pub async fn trigger_delete(
    client: &(impl McpClient + Sync),
    trigger_id_prefix: &str,
) -> Result<String> {
    let triggers = trigger_list(client, None).await?;
    let trigger = triggers
        .iter()
        .find(|t| t.id.starts_with(trigger_id_prefix))
        .ok_or_else(|| anyhow!("No trigger found matching '{}'", trigger_id_prefix))?;
    let id = trigger.id.clone();
    client.delete_chain_trigger(id.clone()).await?;
    Ok(id)
}

pub async fn trigger_toggle(
    client: &(impl McpClient + Sync),
    trigger_id_prefix: &str,
    enabled: bool,
) -> Result<(String, bool)> {
    let triggers = trigger_list(client, None).await?;
    let trigger = triggers
        .iter()
        .find(|t| t.id.starts_with(trigger_id_prefix))
        .ok_or_else(|| anyhow!("No trigger found matching '{}'", trigger_id_prefix))?;
    let id = trigger.id.clone();
    client.toggle_chain_trigger(id.clone(), enabled).await?;
    Ok((id, enabled))
}
