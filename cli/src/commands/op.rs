use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Subcommand;
use colored::Colorize;
use common::mcp::ops::{self, OpCancelResult, OpRunResult, OpInfoResult};
use common::SemanticOpStatus;
use serde_json::json;

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

    /// Show operation or chain info
    Info {
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
        OpCommand::Info { short_id } => get_info(client, &short_id, output).await,
        OpCommand::Cancel { short_id } => cancel(client, &short_id, output).await,
        OpCommand::List => list_tracked(client, output).await,
    }
}

async fn list_available(client: &CliClient, output: &OutputFormat) -> Result<()> {
    let result = ops::list_available(client).await?;

    if result.operations.is_empty() && result.chains.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"operations": [], "chains": [], "count": 0})),
            OutputFormat::Text => print_error("No operations or chains available"),
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => {
            let ops_json: Vec<_> = result.operations.iter().map(|op| {
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
            let chains_json: Vec<_> = result.chains.iter().map(|c| {
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
            if !result.operations.is_empty() {
                print_header("Available Operations");
                println!();

                let mut categories: std::collections::HashMap<&str, Vec<_>> = std::collections::HashMap::new();
                for op in &result.operations {
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

                print_success(&format!("{} operation(s) available", result.operations.len()));
            }

            if !result.chains.is_empty() {
                print_header("Available Chains");
                println!();

                let mut categories: std::collections::HashMap<&str, Vec<_>> = std::collections::HashMap::new();
                for chain in &result.chains {
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

                print_success(&format!("{} chain(s) available", result.chains.len()));
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
    let result = ops::run(client, name, node_prefix, agent, working_dir).await?;

    match result {
        OpRunResult::Operation { id, name } => {
            let short_id = format_short_id(&id);
            match output {
                OutputFormat::Json => {
                    print_json(&json!({
                        "status": "success",
                        "operation_id": short_id,
                        "operation_name": name
                    }));
                }
                OutputFormat::Text => {
                    print_success(&format!("Operation queued: {} ({})", name, short_id));
                }
            }
        }
        OpRunResult::Chain { name, execution_id } => {
            match output {
                OutputFormat::Json => {
                    if let Some(ref exec_id) = execution_id {
                        print_json(&json!({
                            "status": "success",
                            "execution_id": format_short_id(exec_id),
                            "chain_name": name
                        }));
                    } else {
                        print_json(&json!({
                            "status": "success",
                            "message": "Chain queued",
                            "chain_name": name
                        }));
                    }
                }
                OutputFormat::Text => {
                    if let Some(ref exec_id) = execution_id {
                        print_success(&format!("Chain '{}' started ({})", name, format_short_id(exec_id)));
                    } else {
                        print_success(&format!("Chain '{}' queued", name));
                    }
                }
            }
        }
    }

    Ok(())
}

async fn get_info(client: &CliClient, short_id: &str, output: &OutputFormat) -> Result<()> {
    let result = ops::get_info(client, short_id).await;

    match result {
        Ok(OpInfoResult::Operation(op)) => show_op_info(&op, output),
        Ok(OpInfoResult::Chain(exec)) => show_chain_info(&exec, output),
        Err(_) => {
            let msg = format!("No operation or chain found matching '{}'", short_id);
            if matches!(output, OutputFormat::Json) {
                print_json(&json!({"status": "error", "message": msg}));
            }
            Err(anyhow::anyhow!(msg))
        }
    }
}

fn show_op_info(op: &common::SemanticOpUpdate, output: &OutputFormat) -> Result<()> {
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
            println!("  Node:   {}", format_short_id(&op.node_id));
            if let Some(pos) = op.queue_position {
                println!("  Queue:  {}", pos);
            }
            if let Some(ref result) = op.result {
                println!();
                println!("  {}", "Result:".bold());
                print_markdown(result);
            }
            if let Some(ref out) = op.output {
                println!();
                println!("  {}", "Output:".bold());
                print_markdown(out);
            }
        }
    }
    Ok(())
}

fn show_chain_info(exec: &common::ChainExecutionUpdate, output: &OutputFormat) -> Result<()> {
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
            println!("  Status:   {}", format_status(&status_str));
            println!("  Node:     {}", format_short_id(&exec.node_id));
            println!("  Agent:    {}", exec.agent_short_name);
            println!("  Elements: {}", exec.elements.len());
            println!("  Started:  {}", exec.started_at.format("%Y-%m-%d %H:%M:%S"));
            if let Some(ended) = exec.ended_at {
                let ended: DateTime<Utc> = ended;
                let duration = ended - exec.started_at;
                println!("  Ended:    {} ({}s)", ended.format("%Y-%m-%d %H:%M:%S"), duration.num_seconds());
            }

            if !exec.elements.is_empty() {
                println!();
                println!("  {}:", "Elements".bold());
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
    let result = ops::cancel(client, short_id).await;

    match result {
        Ok(OpCancelResult::Operation { id }) => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "success", "message": format!("Cancel request sent for operation {}", id)})),
                OutputFormat::Text => print_success(&format!("Cancel request sent for operation {}", id)),
            }
            Ok(())
        }
        Ok(OpCancelResult::Chain { id }) => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "success", "message": format!("Cancel request sent for chain {}", id)})),
                OutputFormat::Text => print_success(&format!("Cancel request sent for chain {}", id)),
            }
            Ok(())
        }
        Err(_) => {
            let msg = format!("No operation or chain found matching '{}'", short_id);
            if matches!(output, OutputFormat::Json) {
                print_json(&json!({"status": "error", "message": msg}));
            }
            Err(anyhow::anyhow!(msg))
        }
    }
}

async fn list_tracked(client: &CliClient, output: &OutputFormat) -> Result<()> {
    let result = ops::list_tracked(client).await?;

    if result.operations.is_empty() && result.chains.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"operations": [], "chains": [], "count": 0})),
            OutputFormat::Text => print_error("No tracked operations or chains"),
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => {
            let ops_json: Vec<_> = result.operations.iter().map(|op| {
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
            let execs_json: Vec<_> = result.chains.iter().map(|exec| {
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
            if !result.operations.is_empty() {
                print_header("Tracked Operations");
                println!();

                for op in &result.operations {
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
                print_success(&format!("{} operation(s) tracked", result.operations.len()));
            }

            if !result.chains.is_empty() {
                print_header("Tracked Chain Executions");
                println!();

                for exec in &result.chains {
                    println!(
                        "  {} {} on {} [{}]",
                        format_short_id(&exec.execution_id),
                        exec.chain_name,
                        format_short_id(&exec.node_id),
                        format_status(&exec.status.to_string())
                    );
                }

                println!();
                print_success(&format!("{} chain execution(s) tracked", result.chains.len()));
            }
        }
    }

    Ok(())
}
