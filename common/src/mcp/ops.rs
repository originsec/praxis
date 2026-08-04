use anyhow::{Result, anyhow};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::time::Duration;
use uuid::Uuid;

use crate::acp_ext::{EXT_PRAXIS_GREP_FILES, EXT_PRAXIS_READ_FILE};
use crate::mcp::McpClient;
use crate::{
    AgentFileType, AgentTool, ChainConnection, ChainDefinitionFull, ChainDefinitionInfo,
    ChainDefinitionInput, ChainElement, ChainExecutionUpdate, ChainTriggerInfo, ChainTriggerType,
    ConfigItem, GrepFileEntry, McpServer, OperationDefinitionInfo, SemanticOpUpdate,
    SemanticOperationSpec, SessionItem, SystemState, TargetSpec, TriggerConfig,
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
    Operation {
        id: String,
        name: String,
    },
    Chain {
        name: String,
        execution_id: Option<String>,
    },
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

pub enum OpDefinitionResult {
    Operation(OperationDefinitionInfo),
    Chain(ChainDefinitionFull),
}

//
// Resolve a node ID from a prefix by matching against connected nodes.
//

pub fn resolve_node_id(state: &SystemState, prefix: &str) -> Result<String> {
    state
        .nodes
        .iter()
        .find(|n| n.node_id.to_lowercase().starts_with(&prefix.to_lowercase()))
        .map(|n| n.node_id.clone())
        .ok_or_else(|| {
            anyhow!(
                "No node found matching '{}'. Use node_list to see connected nodes.",
                prefix
            )
        })
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
// Get the full definition of an operation or chain by name.
//

pub async fn get_definition(
    client: &(impl McpClient + Sync),
    name: &str,
) -> Result<OpDefinitionResult> {
    client.request_op_def_list().await?;
    client.request_chain_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let ops = client.get_operation_definitions().await;
    if let Some(op) = ops
        .iter()
        .find(|d| d.full_name == name || d.short_name == name || d.name == name)
    {
        return Ok(OpDefinitionResult::Operation(op.clone()));
    }

    let chains = client.get_chain_definitions().await;
    if let Some(chain_info) = chains
        .iter()
        .find(|c| c.name == name || c.id.starts_with(name))
    {
        client.request_chain(&chain_info.id).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        if let Some(chain_full) = client.get_current_chain().await {
            return Ok(OpDefinitionResult::Chain(chain_full));
        }
        return Err(anyhow!(
            "Chain '{}' found but failed to fetch full definition",
            name
        ));
    }

    Err(anyhow!(
        "No operation or chain found matching '{}'. Use op_available to see definitions.",
        name
    ))
}

//
// Create or update an operation definition. Returns the full_name of the
// created/updated definition.
//

pub async fn op_create(
    client: &(impl McpClient + Sync),
    spec: SemanticOperationSpec,
    category: &str,
    short_name: &str,
) -> Result<String> {
    client.create_op_def(spec, category, short_name).await
}

//
// Delete an operation definition by full name or short name.
//

pub async fn op_delete(client: &(impl McpClient + Sync), name: &str) -> Result<String> {
    //
    // Resolve to full_name if a short name or display name was given.
    //

    client.request_op_def_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let ops = client.get_operation_definitions().await;
    let full_name = ops
        .iter()
        .find(|d| {
            d.full_name == name
                || d.short_name == name
                || d.name == name
        })
        .map(|d| d.full_name.clone())
        .ok_or_else(|| {
            anyhow!(
                "No operation definition found matching '{}'. Use op_available to list definitions.",
                name
            )
        })?;

    client.delete_op_def(&full_name).await?;
    Ok(full_name)
}

//
// Create a valid linear chain from existing operation definitions. The graph
// always has a manual trigger and explicit termination so it can be run
// manually or attached to an automated chain trigger.
//

pub async fn chain_create(
    client: &(impl McpClient + Sync),
    name: &str,
    description: &str,
    category: &str,
    operation_names: &[String],
    timeout: Option<u64>,
) -> Result<ChainDefinitionInfo> {
    let name = name.trim();
    let category = category.trim();
    if name.is_empty() {
        return Err(anyhow!("Chain name cannot be empty"));
    }
    if category.is_empty() {
        return Err(anyhow!("Chain category cannot be empty"));
    }
    if operation_names.is_empty() {
        return Err(anyhow!("A chain must contain at least one operation"));
    }

    client.request_op_def_list().await?;
    client.request_chain_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let chains = client.get_chain_definitions().await;
    if chains
        .iter()
        .any(|chain| chain.name.eq_ignore_ascii_case(name))
    {
        return Err(anyhow!(
            "A chain named '{}' already exists. Use op_available to inspect it.",
            name
        ));
    }

    let operation_definitions = client.get_operation_definitions().await;
    let mut resolved_operations = Vec::with_capacity(operation_names.len());
    for requested_name in operation_names {
        let requested_name = requested_name.trim();
        if requested_name.is_empty() {
            return Err(anyhow!("Operation names cannot be empty"));
        }

        let exact = operation_definitions
            .iter()
            .find(|operation| operation.full_name.eq_ignore_ascii_case(requested_name));
        let operation = match exact {
            Some(operation) => operation.clone(),
            None => {
                let matches: Vec<_> = operation_definitions
                    .iter()
                    .filter(|operation| {
                        operation.short_name.eq_ignore_ascii_case(requested_name)
                            || operation.name.eq_ignore_ascii_case(requested_name)
                    })
                    .collect();
                match matches.as_slice() {
                    [operation] => (*operation).clone(),
                    [] => {
                        return Err(anyhow!(
                            "No operation definition found matching '{}'. Use op_available to list definitions.",
                            requested_name
                        ));
                    }
                    _ => {
                        return Err(anyhow!(
                            "Operation name '{}' is ambiguous. Use its full category::short_name.",
                            requested_name
                        ));
                    }
                }
            }
        };

        if operation.disabled {
            return Err(anyhow!(
                "Operation '{}' is disabled and cannot be added to a new chain",
                operation.full_name
            ));
        }
        resolved_operations.push(operation.full_name.clone());
    }

    let definition = build_linear_chain_definition(
        name,
        description.trim(),
        category,
        &resolved_operations,
        timeout,
    );
    client.create_chain_definition(definition).await
}

fn build_linear_chain_definition(
    name: &str,
    description: &str,
    category: &str,
    operation_names: &[String],
    timeout: Option<u64>,
) -> ChainDefinitionInput {
    let trigger_id = Uuid::new_v4().to_string();
    let operation_ids: Vec<_> = operation_names
        .iter()
        .map(|_| Uuid::new_v4().to_string())
        .collect();
    let termination_id = Uuid::new_v4().to_string();

    let mut elements = Vec::with_capacity(operation_names.len() + 2);
    elements.push(ChainElement::Trigger {
        id: trigger_id.clone(),
        trigger_type: ChainTriggerType::Manual,
    });
    for (operation_name, id) in operation_names.iter().zip(&operation_ids) {
        elements.push(ChainElement::Operation {
            id: id.clone(),
            operation_name: operation_name.clone(),
            model_ref: None,
            session_group: None,
            block_config: None,
        });
    }
    elements.push(ChainElement::Termination {
        id: termination_id.clone(),
        block_config: None,
    });

    let mut element_ids = Vec::with_capacity(elements.len());
    element_ids.push(trigger_id);
    element_ids.extend(operation_ids);
    element_ids.push(termination_id);
    let connections = element_ids
        .windows(2)
        .map(|pair| ChainConnection {
            id: Uuid::new_v4().to_string(),
            from_element: pair[0].clone(),
            to_element: pair[1].clone(),
            from_port: 0,
            to_port: 0,
            condition: None,
        })
        .collect();

    ChainDefinitionInput {
        name: name.to_string(),
        description: description.to_string(),
        category: category.to_string(),
        elements,
        connections,
        disabled: false,
        timeout,
        positions: HashMap::new(),
    }
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
    let state = client.get_state().await.ok_or_else(|| {
        anyhow!("No state available. The service may still be starting — try again in a moment.")
    })?;
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
            || format!("{}::{}", op.category, op.short_name).to_lowercase() == name.to_lowercase()
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
        c.id.to_lowercase().starts_with(&name.to_lowercase())
            || c.name.to_lowercase() == name.to_lowercase()
    });

    match chain {
        Some(chain) => {
            let chain_id = chain.id.clone();
            let chain_name = chain.name.clone();

            client
                .run_chain(
                    chain_id.clone(),
                    node_id.clone(),
                    agent.to_string(),
                    working_dir,
                )
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
        None => Err(anyhow!(
            "No operation or chain found matching '{}'. Use op_available to list what's available.",
            name
        )),
    }
}

//
// Check status of an operation or chain execution by short ID. Tries semantic
// operations first, then falls back to chain executions.
//

pub async fn get_info(client: &(impl McpClient + Sync), short_id: &str) -> Result<OpInfoResult> {
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

pub async fn cancel(client: &(impl McpClient + Sync), short_id: &str) -> Result<OpCancelResult> {
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
    let state = client.get_state().await.ok_or_else(|| {
        anyhow!("No state available. The service may still be starting — try again in a moment.")
    })?;
    let node_id = resolve_node_id(&state, node_prefix)?;
    let recon = client
        .get_stored_recon(&node_id, agent)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "No stored recon for {}:{}. Run recon_run first to discover files and tools.",
                node_prefix,
                agent
            )
        })?;

    let show_all = section.is_none() || section == Some("all");

    Ok(ReconListResult {
        sessions: if show_all || section == Some("sessions") {
            Some(recon.sessions.items)
        } else {
            None
        },
        projects: if show_all || section == Some("projects") {
            Some(recon.config.project_paths)
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
            Some(recon.config.items)
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
    agent: &str,
    file_type: AgentFileType,
) -> Result<ResolvedRecon> {
    let state = client.get_state().await.ok_or_else(|| {
        anyhow!("No state available. The service may still be starting — try again in a moment.")
    })?;
    let node_id = resolve_node_id(&state, node_prefix)?;
    let recon = client
        .get_stored_recon(&node_id, agent)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "No stored recon data for agent '{}' on this node. Run recon_run first.",
                agent
            )
        })?;

    let paths: Vec<String> = match file_type {
        AgentFileType::Config => recon.config.items.iter().map(|c| c.path.clone()).collect(),
        AgentFileType::Session => recon
            .sessions
            .items
            .iter()
            .map(|s| s.session_file.clone())
            .collect(),
    };

    Ok(ResolvedRecon { node_id, paths })
}

fn has_glob_chars(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[')
}

fn validate_paths(
    recon_paths: &[String],
    paths: &[String],
    file_type: AgentFileType,
) -> Result<()> {
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
                path,
                type_name
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
    agent: &str,
    file_type: AgentFileType,
    path: &str,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<ReadFileResult> {
    let resolved = resolve_recon(client, node_prefix, agent, file_type).await?;
    validate_paths(&resolved.paths, &[path.to_string()], file_type)?;
    read_file_inner(
        client,
        &resolved.node_id,
        agent,
        file_type,
        path,
        line_start,
        line_end,
    )
    .await
}

async fn read_file_inner(
    client: &(impl McpClient + Sync),
    node_id: &str,
    agent: &str,
    file_type: AgentFileType,
    path: &str,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<ReadFileResult> {
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

    let result = client
        .acp_request(node_id, EXT_PRAXIS_READ_FILE, params)
        .await?;

    if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
        // Extension errors wrapped by `ext_err` return {"error": "..."} only.
        // A successful read_file returns the full ReadFileResult struct which
        // also has an optional `error` field. Distinguish by presence of
        // `path`.
        if !result.get("path").is_some() {
            return Err(anyhow!(err.to_string()));
        }
    }

    Ok(ReadFileResult {
        path: result
            .get("path")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| path.to_string()),
        content: result
            .get("content")
            .and_then(|v| v.as_str())
            .map(String::from),
        line_start: result
            .get("line_start")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize),
        line_end: result
            .get("line_end")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize),
        error: result
            .get("error")
            .and_then(|v| v.as_str())
            .map(String::from),
    })
}

//
// Read all files of a given type from stored recon.
//

pub async fn recon_read_all(
    client: &(impl McpClient + Sync),
    node_prefix: &str,
    agent: &str,
    file_type: AgentFileType,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<Vec<ReadFileResult>> {
    let resolved = resolve_recon(client, node_prefix, agent, file_type).await?;

    if resolved.paths.is_empty() {
        return Err(anyhow!(
            "No files found in recon data. Run recon_run to discover files."
        ));
    }

    let mut results = Vec::new();
    for path in &resolved.paths {
        match read_file_inner(
            client,
            &resolved.node_id,
            agent,
            file_type,
            path,
            line_start,
            line_end,
        )
        .await
        {
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
    agent: &str,
    file_type: AgentFileType,
    paths: &[String],
    pattern: &str,
) -> Result<GrepFilesResult> {
    let resolved = resolve_recon(client, node_prefix, agent, file_type).await?;
    grep_files_inner(client, &resolved.node_id, agent, file_type, paths, pattern).await
}

async fn grep_files_inner(
    client: &(impl McpClient + Sync),
    node_id: &str,
    agent: &str,
    file_type: AgentFileType,
    paths: &[String],
    pattern: &str,
) -> Result<GrepFilesResult> {
    let params = json!({
        "agent_short_name": agent,
        "file_type": file_type,
        "paths": paths,
        "pattern": pattern,
    });

    let result = client
        .acp_request(node_id, EXT_PRAXIS_GREP_FILES, params)
        .await?;

    //
    // If the node returned only an `error` (ext_err shape), surface it.
    //

    if result.get("pattern").is_none() {
        if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
            return Err(anyhow!(err.to_string()));
        }
    }

    let pattern = result
        .get("pattern")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| pattern.to_string());
    let results: Vec<GrepFileEntry> = result
        .get("results")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| anyhow!("Failed to parse grep results: {}", e))?
        .unwrap_or_default();
    let errors: Vec<String> = result
        .get("errors")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| anyhow!("Failed to parse grep errors: {}", e))?
        .unwrap_or_default();

    Ok(GrepFilesResult {
        pattern,
        results,
        errors,
    })
}

//
// Grep all files of a given type from stored recon in a single round-trip.
// Returns only files with matches (filters out empty results).
//

pub async fn recon_grep_all(
    client: &(impl McpClient + Sync),
    node_prefix: &str,
    agent: &str,
    file_type: AgentFileType,
    pattern: &str,
) -> Result<GrepFilesResult> {
    let resolved = resolve_recon(client, node_prefix, agent, file_type).await?;

    if resolved.paths.is_empty() {
        return Err(anyhow!(
            "No files found in recon data. Run recon_run to discover files."
        ));
    }

    let mut result = grep_files_inner(
        client,
        &resolved.node_id,
        agent,
        file_type,
        &resolved.paths,
        pattern,
    )
    .await?;

    // Filter to only files with matches
    result
        .results
        .retain(|r| r.error.is_some() || !r.matches.is_empty());
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
) -> Result<(ChainTriggerInfo, Option<String>)> {
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
    let requested_agents = target_spec.agent_short_names.clone();

    let trigger = client
        .create_chain_trigger(chain_id.clone(), trigger_config, target_spec)
        .await?;

    let warning = unmatched_agent_short_names_warning(client, &requested_agents).await;
    Ok((trigger, warning))
}

//
// Trigger target specs match agent short names by exact, case-sensitive
// string equality (see resolve_targets), and distinct connectors can
// register visually similar short names for genuinely different things
// (e.g. the CLI-discovered "claudecode" vs the "claude-code" Claude Code
// Bridge connector). Getting it wrong doesn't error -- it silently resolves
// zero targets forever, so warn eagerly when a requested name doesn't match
// any currently discovered agent. This is advisory only, not a hard
// failure: new_node triggers legitimately target agent kinds that haven't
// connected yet.
//
async fn unmatched_agent_short_names_warning(
    client: &(impl McpClient + Sync),
    requested: &[String],
) -> Option<String> {
    if requested.is_empty() {
        return None;
    }
    let state = client.get_state().await?;
    let known: BTreeSet<&str> = state
        .nodes
        .iter()
        .flat_map(|n| n.discovered_agents.iter())
        .map(|a| a.short_name.as_str())
        .collect();
    let unknown: Vec<&String> = requested
        .iter()
        .filter(|name| !known.contains(name.as_str()))
        .collect();
    if unknown.is_empty() {
        return None;
    }
    Some(format!(
        "agent_short_names {:?} do not match any agent short name currently seen on a \
         connected node (currently known: {:?}). If this trigger targets an agent kind \
         that hasn't connected yet this may be expected -- otherwise call \
         node_list/agent_list to confirm the real short name before trusting it.",
        unknown, known
    ))
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
    client.delete_chain_trigger(id).await
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
    let trigger = client.toggle_chain_trigger(id, enabled).await?;
    Ok((trigger.id, trigger.enabled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_linear_operation_chain() {
        let operations = vec![
            "custom::cicd".to_string(),
            "recon::system_info".to_string(),
        ];
        let chain = build_linear_chain_definition(
            "CI/CD on connect",
            "Run CI/CD discovery on new nodes",
            "custom",
            &operations,
            Some(600),
        );

        assert_eq!(chain.elements.len(), 4);
        assert_eq!(chain.connections.len(), 3);
        assert!(matches!(chain.elements[0], ChainElement::Trigger { .. }));
        assert!(matches!(
            &chain.elements[1],
            ChainElement::Operation { operation_name, .. } if operation_name == "custom::cicd"
        ));
        assert!(matches!(
            &chain.elements[2],
            ChainElement::Operation { operation_name, .. } if operation_name == "recon::system_info"
        ));
        assert!(matches!(chain.elements[3], ChainElement::Termination { .. }));

        for (connection, elements) in chain.connections.iter().zip(chain.elements.windows(2)) {
            assert_eq!(connection.from_element, element_id(&elements[0]));
            assert_eq!(connection.to_element, element_id(&elements[1]));
        }
    }

    fn element_id(element: &ChainElement) -> &str {
        match element {
            ChainElement::Trigger { id, .. }
            | ChainElement::Operation { id, .. }
            | ChainElement::Transform { id, .. }
            | ChainElement::GenericPrompt { id, .. }
            | ChainElement::Memory { id, .. }
            | ChainElement::Loop { id, .. }
            | ChainElement::Tool { id, .. }
            | ChainElement::Payload { id, .. }
            | ChainElement::Termination { id, .. } => id,
        }
    }
}
