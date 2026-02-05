use anyhow::{anyhow, Result};
use clap::Subcommand;
use common::SemanticOpStatus;
use serde_json::json;
use std::time::Duration;

use crate::client::CliClient;
use crate::output::{format_short_id, format_status, print_error, print_header, print_json, print_success, OutputFormat};

#[derive(Subcommand)]
pub enum OpCommand {
    /// List available operations
    List,

    /// Run an operation
    Run {
        /// Operation name (e.g., "recon::network_scan" or just "network_scan")
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

    /// Check operation status
    Status {
        /// Operation short ID
        short_id: String,
    },

    /// Cancel a running operation
    Cancel {
        /// Operation short ID
        short_id: String,
    },

    /// List running/queued operations
    Running,
}

pub async fn execute(client: &mut CliClient, command: OpCommand, output: &OutputFormat) -> Result<()> {
    match command {
        OpCommand::List => list_operations(client, output).await,
        OpCommand::Run { name, node, agent, working_dir } => {
            run_operation(client, &name, &node, &agent, working_dir, output).await
        }
        OpCommand::Status { short_id } => get_status(client, &short_id, output).await,
        OpCommand::Cancel { short_id } => cancel_operation(client, &short_id, output).await,
        OpCommand::Running => list_running(client, output).await,
    }
}

async fn list_operations(client: &CliClient, output: &OutputFormat) -> Result<()> {
    //
    // Request fresh operation definitions.
    //
    client.request_op_def_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let op_defs = client.get_operation_definitions().await;

    let enabled_ops: Vec<_> = op_defs.iter().filter(|op| !op.disabled).collect();

    if enabled_ops.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"operations": [], "count": 0})),
            OutputFormat::Text => print_error("No operations available"),
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => {
            let ops_json: Vec<_> = enabled_ops.iter().map(|op| {
                json!({
                    "category": op.category,
                    "short_name": op.short_name,
                    "full_name": op.full_name,
                    "name": op.name,
                    "description": op.description,
                    "timeout": op.timeout
                })
            }).collect();
            print_json(&json!({"operations": ops_json, "count": ops_json.len()}));
        }
        OutputFormat::Text => {
            print_header("Available Operations");
            println!();

            //
            // Group by category.
            //
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
    }

    Ok(())
}

async fn run_operation(
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
    // Get operation definitions to validate name.
    //
    client.request_op_def_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let op_defs = client.get_operation_definitions().await;
    let operation = op_defs.iter().find(|op| {
        op.full_name.to_lowercase() == name.to_lowercase() ||
        op.short_name.to_lowercase() == name.to_lowercase() ||
        format!("{}::{}", op.category, op.short_name).to_lowercase() == name.to_lowercase()
    }).ok_or_else(|| anyhow!("Operation not found: {}", name))?;

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

    Ok(())
}

async fn get_status(client: &CliClient, short_id: &str, output: &OutputFormat) -> Result<()> {
    //
    // Request fresh operation list.
    //
    client.request_semantic_op_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let ops = client.get_operations().await;
    let found = ops.iter().find(|op| op.operation_id.starts_with(short_id));

    match found {
        Some(op) => {
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
                        for line in out.lines().take(20) {
                            println!("    {}", line);
                        }
                        if out.lines().count() > 20 {
                            println!("    ... (truncated)");
                        }
                    }
                }
            }
            Ok(())
        }
        None => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "error", "message": format!("Operation not found: {}", short_id)})),
                OutputFormat::Text => print_error(&format!("Operation not found: {}", short_id)),
            }
            Err(anyhow!("Operation not found"))
        }
    }
}

async fn cancel_operation(client: &CliClient, short_id: &str, output: &OutputFormat) -> Result<()> {
    let ops = client.get_operations().await;
    let found = ops.iter().find(|op| op.operation_id.starts_with(short_id));

    match found {
        Some(op) => {
            client.cancel_semantic_op(op.operation_id.clone()).await?;

            match output {
                OutputFormat::Json => print_json(&json!({"status": "success", "message": format!("Cancel request sent for {}", short_id)})),
                OutputFormat::Text => print_success(&format!("Cancel request sent for {}", short_id)),
            }
            Ok(())
        }
        None => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "error", "message": format!("Operation not found: {}", short_id)})),
                OutputFormat::Text => print_error(&format!("Operation not found: {}", short_id)),
            }
            Err(anyhow!("Operation not found"))
        }
    }
}

async fn list_running(client: &CliClient, output: &OutputFormat) -> Result<()> {
    //
    // Request fresh operation list.
    //
    client.request_semantic_op_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let ops = client.get_operations().await;

    if ops.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"operations": [], "count": 0})),
            OutputFormat::Text => print_error("No tracked operations"),
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
                    "id": format_short_id(&op.operation_id),
                    "name": op.spec.name,
                    "node_id": format_short_id(&op.node_id),
                    "status": status_str,
                    "queue_position": op.queue_position
                })
            }).collect();
            print_json(&json!({"operations": ops_json, "count": ops_json.len()}));
        }
        OutputFormat::Text => {
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
    }

    Ok(())
}
