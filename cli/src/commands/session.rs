use anyhow::{anyhow, Result};
use clap::Subcommand;
use common::{NodeCommand as NodeCmd, NodeCommandResult, SessionCommand as NodeSessionCommand, SessionCommandResult, SessionContext};
use serde_json::json;

use crate::client::CliClient;
use crate::output::{format_short_id, print_json, print_success, OutputFormat};
use crate::spinner::Spinner;

#[derive(Subcommand)]
pub enum SessionCommand {
    /// Create a new session with the selected agent
    Create {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,

        /// Enable YOLO mode (auto-approve actions)
        #[arg(short, long)]
        yolo: bool,

        /// Project/working directory path
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Send a prompt to the session
    Prompt {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,

        /// Prompt text (omit for interactive mode)
        text: Option<String>,
    },

    /// Close the current session
    Close {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,
    },
}

pub async fn execute(client: &mut CliClient, command: SessionCommand, output: &OutputFormat) -> Result<()> {
    match command {
        SessionCommand::Create { node, yolo, project } => create_session(client, &node, yolo, project, output).await,
        SessionCommand::Prompt { node, text: Some(text) } => send_prompt(client, &node, &text, output).await,
        SessionCommand::Prompt { node, text: None } => interactive_prompt(client, &node, output).await,
        SessionCommand::Close { node } => close_session(client, &node, output).await,
    }
}

fn find_node_id(state: &common::SystemState, prefix: &str) -> Option<String> {
    let search = prefix.to_lowercase();
    state.nodes.iter()
        .find(|n| n.node_id.to_lowercase().starts_with(&search))
        .map(|n| n.node_id.clone())
}

async fn create_session(client: &CliClient, node_prefix: &str, yolo: bool, project: Option<String>, output: &OutputFormat) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix).ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let context = SessionContext {
        working_dir: project.clone(),
        yolo_mode: yolo,
    };

    let cmd = NodeCmd::Session(NodeSessionCommand::Create { context });
    let spinner = if matches!(output, OutputFormat::Text) {
        Some(Spinner::start("Creating session..."))
    } else {
        None
    };
    let response = client.send_command(&node_id, cmd).await;
    if let Some(s) = spinner { s.finish().await; }
    let response = response?;

    match response.result {
        NodeCommandResult::Session(SessionCommandResult::Created { session_id }) => {
            match output {
                OutputFormat::Json => {
                    print_json(&json!({
                        "status": "success",
                        "session_id": session_id,
                        "session_id_short": format_short_id(&session_id),
                        "yolo_mode": yolo,
                        "project": project
                    }));
                }
                OutputFormat::Text => {
                    let project_info = project.as_ref().map(|p| format!(" in {}", p)).unwrap_or_default();
                    print_success(&format!("Session created: {}{}", format_short_id(&session_id), project_info));
                    if yolo {
                        println!("  YOLO mode enabled");
                    }
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

async fn send_prompt(client: &CliClient, node_prefix: &str, text: &str, output: &OutputFormat) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix).ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let transaction_id = uuid::Uuid::new_v4().to_string();
    let cmd = NodeCmd::Session(NodeSessionCommand::Prompt {
        text: text.to_string(),
        transaction_id: transaction_id.clone(),
    });

    let spinner = if matches!(output, OutputFormat::Text) {
        Some(Spinner::start_with_elapsed("Thinking..."))
    } else {
        None
    };

    //
    // Race send_command against Ctrl+C. On interrupt, send CancelTransaction
    // to abort the running prompt on the node.
    //

    let response = tokio::select! {
        result = client.send_command(&node_id, cmd) => {
            if let Some(s) = spinner { s.finish().await; }
            result?
        }
        _ = tokio::signal::ctrl_c() => {
            if let Some(s) = spinner { s.finish().await; }
            let cancel_cmd = NodeCmd::Session(NodeSessionCommand::CancelTransaction {
                transaction_id: transaction_id.clone(),
                force: true,
            });
            let _ = client.send_command(&node_id, cancel_cmd).await;
            if matches!(output, OutputFormat::Json) {
                print_json(&json!({"status": "cancelled", "message": "Prompt cancelled"}));
            }
            return Err(anyhow!("Prompt cancelled"));
        }
    };

    match response.result {
        NodeCommandResult::Session(SessionCommandResult::PromptResponse { response, .. }) => {
            match output {
                OutputFormat::Json => {
                    print_json(&json!({
                        "status": "success",
                        "prompt": text,
                        "response": response
                    }));
                }
                OutputFormat::Text => {
                    crate::output::print_markdown(&response);
                }
            }
            Ok(())
        }
        NodeCommandResult::Session(SessionCommandResult::TransactionCancelled { .. }) => {
            if matches!(output, OutputFormat::Json) {
                print_json(&json!({"status": "cancelled", "message": "Prompt cancelled"}));
            }
            Err(anyhow!("Prompt cancelled"))
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

async fn interactive_prompt(client: &CliClient, node_prefix: &str, output: &OutputFormat) -> Result<()> {
    use colored::Colorize;
    use rustyline::error::ReadlineError;
    use rustyline::history::DefaultHistory;
    use rustyline::{Config, Editor};

    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    //
    // Bail early if there's no active session on this node.
    //

    let has_session = state.nodes.iter()
        .find(|n| n.node_id == node_id)
        .and_then(|n| n.selected_agent.as_ref())
        .and_then(|a| a.session_id.as_ref())
        .is_some();

    if !has_session {
        return Err(anyhow!("No active session — create one first with 'session create'"));
    }

    let config = Config::builder().build();
    let mut rl: Editor<(), DefaultHistory> = Editor::with_config(config)?;

    println!();
    println!("  {} {}", "Interactive session".bold(), "(ctrl+c to exit)".dimmed());
    println!();

    let prompt = format!("  {} ", "▸".cyan());

    loop {
        match rl.readline(&prompt) {
            Ok(line) => {
                let text = line.trim();
                if text.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(text);

                let transaction_id = uuid::Uuid::new_v4().to_string();
                let cmd = NodeCmd::Session(NodeSessionCommand::Prompt {
                    text: text.to_string(),
                    transaction_id: transaction_id.clone(),
                });

                let spinner = if matches!(output, OutputFormat::Text) {
                    Some(Spinner::start_with_elapsed("Thinking..."))
                } else {
                    None
                };

                let response = tokio::select! {
                    result = client.send_command(&node_id, cmd) => {
                        if let Some(s) = spinner { s.finish().await; }
                        match result {
                            Ok(r) => r,
                            Err(e) => {
                                crate::output::print_error(&e.to_string());
                                continue;
                            }
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        if let Some(s) = spinner { s.finish().await; }
                        let cancel_cmd = NodeCmd::Session(NodeSessionCommand::CancelTransaction {
                            transaction_id: transaction_id.clone(),
                            force: true,
                        });
                        let _ = client.send_command(&node_id, cancel_cmd).await;
                        println!();
                        println!("  {}", "Cancelled".dimmed());
                        continue;
                    }
                };

                match response.result {
                    NodeCommandResult::Session(SessionCommandResult::PromptResponse { response, .. }) => {
                        match output {
                            OutputFormat::Json => {
                                print_json(&json!({
                                    "status": "success",
                                    "prompt": text,
                                    "response": response
                                }));
                            }
                            OutputFormat::Text => {
                                println!();
                                crate::output::print_markdown(&response);
                                println!();
                            }
                        }
                    }
                    NodeCommandResult::Session(SessionCommandResult::TransactionCancelled { .. }) => {
                        println!("  {}", "Cancelled".dimmed());
                    }
                    NodeCommandResult::Error { message } => {
                        crate::output::print_error(&message);
                    }
                    _ => {
                        crate::output::print_error("Unexpected response");
                    }
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(e) => {
                crate::output::print_error(&format!("Input error: {}", e));
                break;
            }
        }
    }

    Ok(())
}

async fn close_session(client: &CliClient, node_prefix: &str, output: &OutputFormat) -> Result<()> {
    let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix).ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Session(NodeSessionCommand::Close);
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Session(SessionCommandResult::Closed) => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "success", "message": "Session closed"})),
                OutputFormat::Text => print_success("Session closed"),
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
