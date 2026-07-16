use anyhow::{Result, anyhow};
use clap::Subcommand;
use serde_json::json;

use crate::client::Client;
use crate::output::{format_short_id, print_success};
use crate::state::CliState;

#[derive(Subcommand)]
pub enum SessionCommand {
    /// Create a new ACP session on a node with a specific agent
    Create {
        /// Node ID prefix
        #[arg(short, long)]
        node: String,

        /// Agent short name (e.g. 'claude-code', 'codex')
        #[arg(short, long)]
        agent: String,

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
            agent,
            yolo,
            project,
            timeout,
        } => create_session(client, &node, &agent, yolo, project, timeout).await,
        SessionCommand::Prompt { node, text } => send_prompt(client, &node, &text).await,
        SessionCommand::Close { node } => close_session(client, &node).await,
    }
}

async fn create_session(
    client: &Client,
    node_prefix: &str,
    agent: &str,
    yolo: bool,
    project: Option<String>,
    timeout: Option<u64>,
) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node_id = super::find_node_id(&state, node_prefix)
        .map_err(|e| anyhow!("Node '{}': {}", node_prefix, e))?;

    let prompt_timeout_secs = match timeout {
        Some(t) => Some(t),
        None => client
            .get_config(vec!["prompt_timeout_secs".to_string()])
            .await
            .ok()
            .and_then(|cfg| {
                cfg.get("prompt_timeout_secs")
                    .and_then(|v| v.parse::<u64>().ok())
            }),
    };

    let cwd = project.clone().unwrap_or_else(|| "/".to_string());

    let mut praxis_meta = json!({
        "nodeId": node_id,
        "connector": agent,
        "yolo": yolo,
        "interactive": false,
    });
    if let Some(t) = prompt_timeout_secs {
        praxis_meta["promptTimeoutSecs"] = json!(t);
    }

    let result = client
        .acp_request(
            &node_id,
            "session/new",
            json!({
                "cwd": cwd,
                "mcpServers": [],
                "_meta": { "praxis": praxis_meta }
            }),
        )
        .await?;

    let session_id = result
        .get("sessionId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("session/new response missing sessionId"))?
        .to_string();

    //
    // Persist the session id so subsequent `session prompt` / `session
    // close` invocations can find it.
    //

    let mut cli_state = CliState::load().unwrap_or_default();
    cli_state.set_session(&node_id, &session_id)?;

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

async fn send_prompt(client: &Client, node_prefix: &str, text: &str) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node_id = super::find_node_id(&state, node_prefix)
        .map_err(|e| anyhow!("Node '{}': {}", node_prefix, e))?;

    let cli_state = CliState::load().unwrap_or_default();
    let session_id = cli_state
        .get_session(&node_id)
        .ok_or_else(|| anyhow!("No active session for node. Run `session create` first."))?;

    let (_result, text) = client
        .acp_request_collecting_text(
            &node_id,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }],
            }),
        )
        .await?;

    println!("{}", text);
    Ok(())
}

async fn close_session(client: &Client, node_prefix: &str) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node_id = super::find_node_id(&state, node_prefix)
        .map_err(|e| anyhow!("Node '{}': {}", node_prefix, e))?;

    let mut cli_state = CliState::load().unwrap_or_default();
    let session_id = cli_state
        .get_session(&node_id)
        .ok_or_else(|| anyhow!("No active session for node."))?;

    client
        .acp_request(
            &node_id,
            "session/close",
            json!({
                "sessionId": session_id,
            }),
        )
        .await?;

    cli_state.clear_session(&node_id)?;

    print_success("Session closed");
    Ok(())
}
