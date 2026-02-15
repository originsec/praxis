use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use clap::Subcommand;
use common::{ChainExecutionUpdate, SemanticOpStatus};
use serde_json::json;
use std::time::Duration;

use crate::client::CliClient;
use crate::output::{format_short_id, format_status, print_error, print_header, print_json, print_markdown, print_success, OutputFormat};

#[derive(Subcommand)]
pub enum OpCommand {
    /// List available operations and chains
    Available,

    /// Run an operation or chain
    Run {
        /// Operation or chain name
        name: String,

        /// Node ID prefix
        #[arg(short, long)]
        node: String,

        /// Agent short name
        #[arg(short, long)]
        agent: String,

        /// Working directory
        #[arg(short, long)]
        working_dir: Option<String>,
    },

    /// Check operation or chain status
    Status {
        /// Short ID
        short_id: String,
    },

    /// Cancel a running operation or chain
    Cancel {
        /// Short ID
        short_id: String,
    },

    /// List tracked operations and chains
    List,
}

pub async fn execute(client: &mut CliClient, command: OpCommand, output: &OutputFormat) -> Result<()> {
    match command {
        OpCommand::Available => list_available(client, output).await,
        OpCommand::Run { name, node, agent, working_dir } => {
            run(client, &name, &node, &agent, working_dir, output).await
        }
        OpCommand::Status { short_id } => get_status(client, &short_id, output).await,
        OpCommand::Cancel { short_id } => cancel(client, &short_id, output).await,
        OpCommand::List => list_running(client, output).await,
    }
}

async fn list_available(client: &CliClient, output: &OutputFormat) -> Result<()> {

    //
    // Fetch both operation and chain definitions concurrently.
    //

    client.request_op_def_list().await?;
    client.request_chain_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let op_defs = client.get_operation_definitions().await;
    let chain_defs = client.get_chain_definitions().await;

    let enabled_ops: Vec<_> = op_defs.iter().filter(|op| !op.disabled).collect();
    let enabled_chains: Vec<_> = chain_defs.iter().filter(|c| !c.disabled).collect();

    if enabled_ops.is_empty() && enabled_chains.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"operations": [], "chains": [], "count": 0})),
            OutputFormat::Text => print_error("No operations or chains available"),
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => {
            let ops_json: Vec<_> = enabled_ops.iter().map(|op| {
                json!({
                    "type": "operation",
                    "category": op.category,
                    "short_name": op.short_name,
                    "full_name": op.full_name,
                    "name": op.name,
                    "description": op.description,
                    "timeout": op.timeout
                })
            }).collect();
            let chains_json: Vec<_> = enabled_chains.iter().map(|c| {
                json!({
                    "type": "chain",
                    "id": c.id,
                    "id_short": format_short_id(&c.id),
                    "name": c.name,
                    "description": c.description,
                    "category": c.category,
                    "element_count": c.element_count,
                    "operation_count": c.operation_count,
                    "timeout": c.timeout
                })
            }).collect();
            print_json(&json!({
                "operations": ops_json,
                "chains": chains_json,
                "operation_count": ops_json.len(),
                "chain_count": chains_json.len()
            }));
        }
        OutputFormat::Text => {
            if !enabled_ops.is_empty() {
                print_header("Available Operations");
                println!();

                let mut categories: std::collections::HashMap<&str, Vec<_>> = std::collections::HashMap::new();
                for op in &enabled_ops {
                    categories.entry(&op.category).or_default().push(op);
                }

                let mut sorted_categories: Vec<_> = categories.keys().collect();
                sorted_categories.sort();

                for category in sorted_categories {
                    println!("  {}:", category);
                    for op in &categories[category] {
                        println!("    {} - {}", op.short_name, op.description);
                    }
                    println!();
                }

                print_success(&format!("{} operation(s) available", enabled_ops.len()));
            }

            if !enabled_chains.is_empty() {
                print_header("Available Chains");
                println!();

                let mut categories: std::collections::HashMap<&str, Vec<_>> = std::collections::HashMap::new();
                for chain in &enabled_chains {
                    categories.entry(&chain.category).or_default().push(chain);
                }

                let mut sorted_categories: Vec<_> = categories.keys().collect();
                sorted_categories.sort();

                for category in sorted_categories {
                    println!("  {}:", category);
                    for chain in &categories[category] {
                        println!(
                            "    {} ({}) - {} ({} elements, {} ops)",
                            chain.name,
                            format_short_id(&chain.id),
                            chain.description,
                            chain.element_count,
                            chain.operation_count
                        );
                    }
                    println!();
                }

                print_success(&format!("{} chain(s) available", enabled_chains.len()));
            }
        }
    }

    Ok(())
}

async fn run(
    client: &CliClient,
    name: &str,
    node_prefix: &str,
    agent: &str,
    working_dir: Option<String>,
    output: &OutputFormat,
) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;

    let node_id = state.nodes.iter()
        .find(|n| n.node_id.to_lowercase().starts_with(&node_prefix.to_lowercase()))
        .map(|n| n.node_id.clone())
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    //
    // Try operation definitions first.
    //

    client.request_op_def_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let op_defs = client.get_operation_definitions().await;
    let operation = op_defs.iter().find(|op| {
        op.full_name.to_lowercase() == name.to_lowercase() ||
        op.short_name.to_lowercase() == name.to_lowercase() ||
        format!("{}::{}", op.category, op.short_name).to_lowercase() == name.to_lowercase()
    });

    if let Some(operation) = operation {
        let operation_id = client.run_semantic_op(
            node_id,
            agent.to_string(),
            operation.full_name.clone(),
            working_dir,
        ).await?;

        let short_id = format_short_id(&operation_id);

        match output {
            OutputFormat::Json => {
                print_json(&json!({
                    "status": "success",
                    "operation_id": short_id,
                    "operation_name": operation.name
                }));
            }
            OutputFormat::Text => {
                print_success(&format!("Operation queued: {} ({})", operation.name, short_id));
            }
        }

        return Ok(());
    }

    //
    // Not an operation — try chain definitions.
    //

    client.request_chain_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let chain_defs = client.get_chain_definitions().await;
    let chain = chain_defs.iter().find(|c| {
        c.id.to_lowercase().starts_with(&name.to_lowercase()) ||
        c.name.to_lowercase() == name.to_lowercase()
    });

    match chain {
        Some(chain) => {
            run_chain(client, chain, &node_id, agent, working_dir, output).await
        }
        None => {
            Err(anyhow!("No operation or chain found matching '{}'", name))
        }
    }
}

async fn run_chain(
    client: &CliClient,
    chain: &common::ChainDefinitionInfo,
    node_id: &str,
    agent: &str,
    working_dir: Option<String>,
    output: &OutputFormat,
) -> Result<()> {
    client.run_chain(
        chain.id.clone(),
        node_id.to_string(),
        agent.to_string(),
        working_dir,
    ).await?;

    //
    // Wait briefly and check for execution.
    //

    tokio::time::sleep(Duration::from_millis(500)).await;
    client.request_chain_execution_list().await?;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let execs: Vec<ChainExecutionUpdate> = client.get_chain_executions().await;
    let matching_exec = execs.iter()
        .filter(|e| e.chain_id == chain.id && e.node_id == node_id)
        .max_by_key(|e| e.started_at);

    match output {
        OutputFormat::Json => {
            if let Some(exec) = matching_exec {
                print_json(&json!({
                    "status": "success",
                    "execution_id": format_short_id(&exec.execution_id),
                    "chain_name": chain.name
                }));
            } else {
                print_json(&json!({
                    "status": "success",
                    "message": "Chain queued",
                    "chain_name": chain.name
                }));
            }
        }
        OutputFormat::Text => {
            if let Some(exec) = matching_exec {
                print_success(&format!("Chain '{}' started ({})", chain.name, format_short_id(&exec.execution_id)));
            } else {
                print_success(&format!("Chain '{}' queued", chain.name));
            }
        }
    }

    Ok(())
}

async fn get_status(client: &CliClient, short_id: &str, output: &OutputFormat) -> Result<()> {

    //
    // Try semantic operations first.
    //

    client.request_semantic_op_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let ops = client.get_operations().await;
    let found_op = ops.iter().find(|op| op.operation_id.starts_with(short_id));

    if let Some(op) = found_op {
        return show_op_status(op, output);
    }

    //
    // Not an operation — try chain executions.
    //

    client.request_chain_execution_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let execs: Vec<ChainExecutionUpdate> = client.get_chain_executions().await;
    let found_exec = execs.iter().find(|e| e.execution_id.starts_with(short_id));

    if let Some(exec) = found_exec {
        return show_chain_status(exec, output);
    }

    match output {
        OutputFormat::Json => print_json(&json!({"status": "error", "message": format!("Not found: {}", short_id)})),
        OutputFormat::Text => print_error(&format!("No operation or chain found matching '{}'", short_id)),
    }
    Err(anyhow!("Not found: {}", short_id))
}

fn show_op_status(op: &common::SemanticOpUpdate, output: &OutputFormat) -> Result<()> {
    let status_str = match op.status {
        SemanticOpStatus::Running => "Running",
        SemanticOpStatus::Queued => "Queued",
        SemanticOpStatus::Completed => "Completed",
        SemanticOpStatus::Failed => "Failed",
        SemanticOpStatus::Cancelled => "Cancelled",
    };

    match output {
        OutputFormat::Json => {
            print_json(&json!({
                "status": "success",
                "operation": {
                    "id": format_short_id(&op.operation_id),
                    "name": op.spec.name,
                    "node_id": format_short_id(&op.node_id),
                    "op_status": status_str,
                    "result": op.result,
                    "output": op.output,
                    "queue_position": op.queue_position
                }
            }));
        }
        OutputFormat::Text => {
            print_header(&format!("Operation {} - {}", format_short_id(&op.operation_id), op.spec.name));
            println!();
            println!("  Status: {}", format_status(status_str));
            println!("  Node: {}", format_short_id(&op.node_id));
            if let Some(pos) = op.queue_position {
                println!("  Queue Position: {}", pos);
            }
            if let Some(ref result) = op.result {
                println!("  Result: {}", result);
            }
            if let Some(ref out) = op.output {
                println!();
                println!("  Output:");
                print_markdown(out);
            }
        }
    }
    Ok(())
}

fn show_chain_status(exec: &ChainExecutionUpdate, output: &OutputFormat) -> Result<()> {
    let status_str = exec.status.to_string();

    match output {
        OutputFormat::Json => {
            let element_statuses: Vec<_> = exec.elements.iter().map(|(id, elem)| {
                json!({
                    "element_id": id,
                    "status": format!("{:?}", elem.status)
                })
            }).collect();

            print_json(&json!({
                "status": "success",
                "execution": {
                    "id": format_short_id(&exec.execution_id),
                    "chain_name": exec.chain_name,
                    "node_id": format_short_id(&exec.node_id),
                    "agent": exec.agent_short_name,
                    "exec_status": status_str,
                    "element_count": exec.elements.len(),
                    "elements": element_statuses,
                    "started_at": exec.started_at.to_rfc3339(),
                    "ended_at": exec.ended_at.map(|t: DateTime<Utc>| t.to_rfc3339())
                }
            }));
        }
        OutputFormat::Text => {
            print_header(&format!("Chain Execution {} - {}", format_short_id(&exec.execution_id), exec.chain_name));
            println!();
            println!("  Status: {}", format_status(&status_str));
            println!("  Node: {}", format_short_id(&exec.node_id));
            println!("  Agent: {}", exec.agent_short_name);
            println!("  Elements: {}", exec.elements.len());
            println!("  Started: {}", exec.started_at.format("%Y-%m-%d %H:%M:%S"));
            if let Some(ended) = exec.ended_at {
                let ended: DateTime<Utc> = ended;
                println!("  Ended: {}", ended.format("%Y-%m-%d %H:%M:%S"));
            }

            if !exec.elements.is_empty() {
                println!();
                println!("  Element Status:");
                for (id, elem) in &exec.elements {
                    let elem_status = match &elem.status {
                        common::ElementExecutionStatus::Pending => "Pending".to_string(),
                        common::ElementExecutionStatus::WaitingForInputs => "Waiting".to_string(),
                        common::ElementExecutionStatus::Running => "Running".to_string(),
                        common::ElementExecutionStatus::Completed { .. } => "Completed".to_string(),
                        common::ElementExecutionStatus::Failed { error } => format!("Failed: {}", error),
                        common::ElementExecutionStatus::Skipped => "Skipped".to_string(),
                    };
                    println!("    {} [{}]", format_short_id(id), format_status(&elem_status));
                }
            }
        }
    }
    Ok(())
}

async fn cancel(client: &CliClient, short_id: &str, output: &OutputFormat) -> Result<()> {

    //
    // Try semantic operations first.
    //

    let ops = client.get_operations().await;
    let found_op = ops.iter().find(|op| op.operation_id.starts_with(short_id));

    if let Some(op) = found_op {
        client.cancel_semantic_op(op.operation_id.clone()).await?;

        match output {
            OutputFormat::Json => print_json(&json!({"status": "success", "message": format!("Cancel request sent for operation {}", short_id)})),
            OutputFormat::Text => print_success(&format!("Cancel request sent for operation {}", short_id)),
        }
        return Ok(());
    }

    //
    // Not an operation — try chain executions.
    //

    let execs: Vec<ChainExecutionUpdate> = client.get_chain_executions().await;
    let found_exec = execs.iter().find(|e| e.execution_id.starts_with(short_id));

    if let Some(exec) = found_exec {
        client.cancel_chain(exec.execution_id.clone()).await?;

        match output {
            OutputFormat::Json => print_json(&json!({"status": "success", "message": format!("Cancel request sent for chain {}", short_id)})),
            OutputFormat::Text => print_success(&format!("Cancel request sent for chain {}", short_id)),
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => print_json(&json!({"status": "error", "message": format!("Not found: {}", short_id)})),
        OutputFormat::Text => print_error(&format!("No operation or chain found matching '{}'", short_id)),
    }
    Err(anyhow!("Not found: {}", short_id))
}

async fn list_running(client: &CliClient, output: &OutputFormat) -> Result<()> {

    //
    // Fetch both operations and chain executions.
    //

    client.request_semantic_op_list().await?;
    client.request_chain_execution_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let ops = client.get_operations().await;
    let execs: Vec<ChainExecutionUpdate> = client.get_chain_executions().await;

    if ops.is_empty() && execs.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"operations": [], "chains": [], "count": 0})),
            OutputFormat::Text => print_error("No tracked operations or chains"),
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => {
            let ops_json: Vec<_> = ops.iter().map(|op| {
                let status_str = match op.status {
                    SemanticOpStatus::Running => "Running",
                    SemanticOpStatus::Queued => "Queued",
                    SemanticOpStatus::Completed => "Completed",
                    SemanticOpStatus::Failed => "Failed",
                    SemanticOpStatus::Cancelled => "Cancelled",
                };
                json!({
                    "type": "operation",
                    "id": format_short_id(&op.operation_id),
                    "name": op.spec.name,
                    "node_id": format_short_id(&op.node_id),
                    "status": status_str,
                    "queue_position": op.queue_position
                })
            }).collect();
            let execs_json: Vec<_> = execs.iter().map(|exec| {
                json!({
                    "type": "chain",
                    "id": format_short_id(&exec.execution_id),
                    "chain_name": exec.chain_name,
                    "node_id": format_short_id(&exec.node_id),
                    "agent": exec.agent_short_name,
                    "status": exec.status.to_string(),
                    "element_count": exec.elements.len()
                })
            }).collect();
            print_json(&json!({
                "operations": ops_json,
                "chains": execs_json,
                "operation_count": ops_json.len(),
                "chain_count": execs_json.len()
            }));
        }
        OutputFormat::Text => {
            if !ops.is_empty() {
                print_header("Tracked Operations");
                println!();

                for op in &ops {
                    let status_str = match op.status {
                        SemanticOpStatus::Running => "Running",
                        SemanticOpStatus::Queued => "Queued",
                        SemanticOpStatus::Completed => "Completed",
                        SemanticOpStatus::Failed => "Failed",
                        SemanticOpStatus::Cancelled => "Cancelled",
                    };

                    println!(
                        "  {} {} on {} [{}]",
                        format_short_id(&op.operation_id),
                        op.spec.name,
                        format_short_id(&op.node_id),
                        format_status(status_str)
                    );
                }

                println!();
                print_success(&format!("{} operation(s) tracked", ops.len()));
            }

            if !execs.is_empty() {
                print_header("Tracked Chain Executions");
                println!();

                for exec in &execs {
                    println!(
                        "  {} {} on {} [{}]",
                        format_short_id(&exec.execution_id),
                        exec.chain_name,
                        format_short_id(&exec.node_id),
                        format_status(&exec.status.to_string())
                    );
                }

                println!();
                print_success(&format!("{} chain execution(s) tracked", execs.len()));
            }
        }
    }

    Ok(())
}
