use anyhow::{anyhow, Result};
use clap::Subcommand;
use common::{NodeCommand as NodeCmd, NodeCommandResult, SessionCommand as NodeSessionCommand, SessionCommandResult, SessionContext};
use serde_json::json;

use crate::client::CliClient;
use crate::output::{format_short_id, print_error, print_json, print_success, OutputFormat};

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

        /// Prompt text
        text: String,
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
        SessionCommand::Prompt { node, text } => send_prompt(client, &node, &text, output).await,
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
    let response = client.send_command(&node_id, cmd).await?;

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
            match output {
                OutputFormat::Json => print_json(&json!({"status": "error", "message": message})),
                OutputFormat::Text => print_error(&message),
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

    let response = client.send_command(&node_id, cmd).await?;

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
                    println!("{}", response);
                }
            }
            Ok(())
        }
        NodeCommandResult::Error { message } => {
            match output {
                OutputFormat::Json => print_json(&json!({"status": "error", "message": message})),
                OutputFormat::Text => print_error(&message),
            }
            Err(anyhow!("{}", message))
        }
        _ => Err(anyhow!("Unexpected response")),
    }
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
            match output {
                OutputFormat::Json => print_json(&json!({"status": "error", "message": message})),
                OutputFormat::Text => print_error(&message),
            }
            Err(anyhow!("{}", message))
        }
        _ => Err(anyhow!("Unexpected response")),
    }
}
