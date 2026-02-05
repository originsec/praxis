use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use clap::Subcommand;
use common::ChainExecutionUpdate;
use serde_json::json;
use std::time::Duration;

use crate::client::CliClient;
use crate::output::{format_short_id, format_status, print_error, print_header, print_json, print_success, OutputFormat};

#[derive(Subcommand)]
pub enum ChainCommand {
    /// List available chains
    List,

    /// Run a chain
    Run {
        /// Chain ID or name
        chain_id: String,

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

    /// Check chain execution status
    Status {
        /// Execution short ID
        short_id: String,
    },

    /// Cancel a chain execution
    Cancel {
        /// Execution short ID
        short_id: String,
    },

    /// List running/queued chain executions
    Running,
}

pub async fn execute(client: &mut CliClient, command: ChainCommand, output: &OutputFormat) -> Result<()> {
    match command {
        ChainCommand::List => list_chains(client, output).await,
        ChainCommand::Run { chain_id, node, agent, working_dir } => {
            run_chain(client, &chain_id, &node, &agent, working_dir, output).await
        }
        ChainCommand::Status { short_id } => get_status(client, &short_id, output).await,
        ChainCommand::Cancel { short_id } => cancel_chain(client, &short_id, output).await,
        ChainCommand::Running => list_running(client, output).await,
    }
}

async fn list_chains(client: &CliClient, output: &OutputFormat) -> Result<()> {
    //
    // Request fresh chain definitions.
    //
    client.request_chain_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let chain_defs = client.get_chain_definitions().await;
    let enabled_chains: Vec<_> = chain_defs.iter().filter(|c| !c.disabled).collect();

    if enabled_chains.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"chains": [], "count": 0})),
            OutputFormat::Text => print_error("No chains available"),
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => {
            let chains_json: Vec<_> = enabled_chains.iter().map(|c| {
                json!({
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
            print_json(&json!({"chains": chains_json, "count": chains_json.len()}));
        }
        OutputFormat::Text => {
            print_header("Available Chains");
            println!();

            //
            // Group by category.
            //
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

    Ok(())
}

async fn run_chain(
    client: &CliClient,
    chain_id_input: &str,
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
    // Get chain definitions to find the chain.
    //
    client.request_chain_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let chain_defs = client.get_chain_definitions().await;
    let chain = chain_defs.iter().find(|c| {
        c.id.to_lowercase().starts_with(&chain_id_input.to_lowercase()) ||
        c.name.to_lowercase() == chain_id_input.to_lowercase()
    }).ok_or_else(|| anyhow!("Chain not found: {}", chain_id_input))?;

    client.run_chain(
        chain.id.clone(),
        node_id.clone(),
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
    // Request fresh execution list.
    //
    client.request_chain_execution_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let execs: Vec<ChainExecutionUpdate> = client.get_chain_executions().await;
    let found = execs.iter().find(|e| e.execution_id.starts_with(short_id));

    match found {
        Some(exec) => {
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
        None => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "error", "message": format!("Chain execution not found: {}", short_id)})),
                OutputFormat::Text => print_error(&format!("Chain execution not found: {}", short_id)),
            }
            Err(anyhow!("Chain execution not found"))
        }
    }
}

async fn cancel_chain(client: &CliClient, short_id: &str, output: &OutputFormat) -> Result<()> {
    let execs: Vec<ChainExecutionUpdate> = client.get_chain_executions().await;
    let found = execs.iter().find(|e| e.execution_id.starts_with(short_id));

    match found {
        Some(exec) => {
            client.cancel_chain(exec.execution_id.clone()).await?;

            match output {
                OutputFormat::Json => print_json(&json!({"status": "success", "message": format!("Cancel request sent for {}", short_id)})),
                OutputFormat::Text => print_success(&format!("Cancel request sent for {}", short_id)),
            }
            Ok(())
        }
        None => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "error", "message": format!("Chain execution not found: {}", short_id)})),
                OutputFormat::Text => print_error(&format!("Chain execution not found: {}", short_id)),
            }
            Err(anyhow!("Chain execution not found"))
        }
    }
}

async fn list_running(client: &CliClient, output: &OutputFormat) -> Result<()> {
    //
    // Request fresh execution list.
    //
    client.request_chain_execution_list().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let execs: Vec<ChainExecutionUpdate> = client.get_chain_executions().await;

    if execs.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"executions": [], "count": 0})),
            OutputFormat::Text => print_error("No tracked chain executions"),
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => {
            let execs_json: Vec<_> = execs.iter().map(|exec| {
                json!({
                    "id": format_short_id(&exec.execution_id),
                    "chain_name": exec.chain_name,
                    "node_id": format_short_id(&exec.node_id),
                    "agent": exec.agent_short_name,
                    "status": exec.status.to_string(),
                    "element_count": exec.elements.len()
                })
            }).collect();
            print_json(&json!({"executions": execs_json, "count": execs_json.len()}));
        }
        OutputFormat::Text => {
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

    Ok(())
}
