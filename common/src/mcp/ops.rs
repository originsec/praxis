use anyhow::{anyhow, Result};
use std::time::Duration;

use crate::mcp::McpClient;
use crate::{
    ChainDefinitionInfo, ChainExecutionUpdate, OperationDefinitionInfo, SemanticOpUpdate,
    SystemState,
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
        .ok_or_else(|| anyhow!("No node found matching '{}'", prefix))
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
        .ok_or_else(|| anyhow!("No state available"))?;
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
        None => Err(anyhow!("No operation or chain found matching '{}'", name)),
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
        "No operation or chain execution found matching '{}'",
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
        "No operation or chain execution found matching '{}'",
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
