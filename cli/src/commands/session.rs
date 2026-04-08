use anyhow::{Result, anyhow};
use clap::Subcommand;
use common::{
    NodeCommand as NodeCmd, NodeCommandResult, SessionCommand as NodeSessionCommand,
    SessionCommandResult, SessionContext,
};

use crate::client::Client;
use crate::output::{format_short_id, print_success};

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

        /// Prompt timeout in seconds
        #[arg(short = 'T', long)]
        timeout: Option<u64>,
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

pub async fn execute(client: &Client, command: SessionCommand) -> Result<()> {
    match command {
        SessionCommand::Create {
            node,
            yolo,
            project,
            timeout,
        } => create_session(client, &node, yolo, project, timeout).await,
        SessionCommand::Prompt { node, text } => send_prompt(client, &node, &text).await,
        SessionCommand::Close { node } => close_session(client, &node).await,
    }
}

fn find_node_id(state: &common::SystemState, prefix: &str) -> Option<String> {
    let search = prefix.to_lowercase();
    state
        .nodes
        .iter()
        .find(|node| node.node_id.to_lowercase().starts_with(&search))
        .map(|node| node.node_id.clone())
}

async fn create_session(
    client: &Client,
    node_prefix: &str,
    yolo: bool,
    project: Option<String>,
    timeout: Option<u64>,
) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    //
    // Use explicit timeout if provided, otherwise fetch from service config.
    //

    let prompt_timeout_secs = match timeout {
        Some(t) => Some(t),
        None => client
            .get_config(vec!["prompt_timeout_secs".to_string()])
            .await
            .ok()
            .and_then(|cfg| cfg.get("prompt_timeout_secs").and_then(|v| v.parse().ok())),
    };

    let context = SessionContext {
        working_dir: project.clone(),
        yolo_mode: yolo,
        prompt_timeout_secs,
        interactive: false,
    };

    let cmd = NodeCmd::Session(NodeSessionCommand::Create { context });
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Session(SessionCommandResult::Created { session_id }) => {
            let project_info = project
                .as_ref()
                .map(|path| format!(" in {}", path))
                .unwrap_or_default();
            print_success(&format!(
                "Session created: {}{}",
                format_short_id(&session_id),
                project_info
            ));
            if yolo {
                println!("  YOLO mode enabled");
            }
            Ok(())
        }
        NodeCommandResult::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn send_prompt(client: &Client, node_prefix: &str, text: &str) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Session(NodeSessionCommand::Prompt {
        text: text.to_string(),
        transaction_id: uuid::Uuid::new_v4().to_string(),
    });
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Session(SessionCommandResult::PromptResponse { response, .. }) => {
            println!("{}", response);
            Ok(())
        }
        NodeCommandResult::Session(SessionCommandResult::TransactionCancelled { .. }) => {
            Err(anyhow!("Prompt cancelled"))
        }
        NodeCommandResult::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("Unexpected response")),
    }
}

async fn close_session(client: &Client, node_prefix: &str) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node_id = find_node_id(&state, node_prefix)
        .ok_or_else(|| anyhow!("No node found matching '{}'", node_prefix))?;

    let cmd = NodeCmd::Session(NodeSessionCommand::Close);
    let response = client.send_command(&node_id, cmd).await?;

    match response.result {
        NodeCommandResult::Session(SessionCommandResult::Closed) => {
            print_success("Session closed");
            Ok(())
        }
        NodeCommandResult::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("Unexpected response")),
    }
}
