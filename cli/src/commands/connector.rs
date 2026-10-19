use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::Subcommand;
use common::connector_author::{ConnectorAuthorOutcome, ConnectorAuthorRequest};

use crate::client::Client;
use crate::event::ConnectorAuthorEvent;
use crate::output::{print_header, print_success};

#[derive(Subcommand)]
pub enum ConnectorCommand {
    /// Generate a draft Lua connector for an unsupported agent with the
    /// configured LLM (saved as a disabled user script for review)
    Generate {
        /// Node ID prefix where the agent is installed
        #[arg(short, long)]
        node: String,
        /// Agent executable name or absolute path
        #[arg(short, long)]
        exe: String,
        /// Repair attempts after a failed dry run (max 5)
        #[arg(long, default_value_t = common::connector_author::DEFAULT_MAX_REPAIRS)]
        max_repairs: u32,
        /// Print the generated script
        #[arg(long)]
        print: bool,
    },

    /// Propose a repair for a failing connector (saved as a new disabled
    /// user script; the original is untouched)
    Repair {
        /// Node ID prefix where the connector fails
        #[arg(short, long)]
        node: String,
        /// Script id, unique id prefix, or name
        #[arg(short, long)]
        script: String,
        /// Description of the failure (otherwise detected with a dry run)
        #[arg(short, long)]
        error: Option<String>,
        /// Executable to probe (defaults to the fingerprinted path)
        #[arg(long)]
        exe: Option<String>,
        /// Repair attempts after a failed dry run (max 5)
        #[arg(long, default_value_t = common::connector_author::DEFAULT_MAX_REPAIRS)]
        max_repairs: u32,
        /// Print the full proposed script in addition to the diff
        #[arg(long)]
        print: bool,
    },

    /// Dry-run a connector on a node: syntax, callbacks, fingerprint and
    /// static recon (no session)
    Validate {
        /// Node ID prefix to run the dry run on
        #[arg(short, long)]
        node: String,
        /// Saved script id, unique id prefix, or name
        #[arg(short, long, conflicts_with = "file", required_unless_present = "file")]
        script: Option<String>,
        /// Local .lua file to validate (always sandboxed)
        #[arg(short, long)]
        file: Option<PathBuf>,
    },
}

pub async fn execute(client: &Client, command: ConnectorCommand) -> Result<()> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    let node_prefix = match &command {
        ConnectorCommand::Generate { node, .. }
        | ConnectorCommand::Repair { node, .. }
        | ConnectorCommand::Validate { node, .. } => node.clone(),
    };
    let node_id = super::find_node_id(&state, &node_prefix)
        .map_err(|e| anyhow!("Node '{}': {}", node_prefix, e))?;

    let (request, print_script) = match command {
        ConnectorCommand::Generate {
            exe,
            max_repairs,
            print,
            ..
        } => (
            ConnectorAuthorRequest::Generate {
                node_id,
                executable: exe,
                max_repairs: Some(max_repairs),
            },
            print,
        ),
        ConnectorCommand::Repair {
            script,
            error,
            exe,
            max_repairs,
            print,
            ..
        } => (
            ConnectorAuthorRequest::Repair {
                node_id,
                script,
                error,
                executable: exe,
                max_repairs: Some(max_repairs),
            },
            print,
        ),
        ConnectorCommand::Validate { script, file, .. } => {
            let script_content = file
                .map(|path| {
                    std::fs::read_to_string(&path)
                        .map_err(|e| anyhow!("Failed to read {}: {}", path.display(), e))
                })
                .transpose()?;
            (
                ConnectorAuthorRequest::Validate {
                    node_id,
                    script,
                    script_content,
                },
                false,
            )
        }
    };

    let outcome = run(client, request).await?;
    print_outcome(&outcome, print_script);

    //
    // A validate outcome's message is the dry-run report already printed
    // above; summarise it in one line instead.
    //
    let message = match (outcome.kind.as_str(), outcome.ok) {
        ("validate", true) => "Dry run passed".to_string(),
        ("validate", false) => "Dry run failed".to_string(),
        _ => outcome.message.clone(),
    };
    if outcome.ok {
        print_success(&message);
        Ok(())
    } else {
        Err(anyhow!(message))
    }
}

//
// Start the workflow and stream its progress to stderr until the outcome
// arrives. Ctrl+C cancels the workflow on the service.
//

async fn run(client: &Client, request: ConnectorAuthorRequest) -> Result<ConnectorAuthorOutcome> {
    let (request_id, mut rx) = client.start_connector_author(request).await?;
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Some(ConnectorAuthorEvent::Progress { message, .. }) => eprintln!("... {}", message),
                Some(ConnectorAuthorEvent::Finished { outcome, .. }) => return Ok(outcome),
                None => return Err(anyhow!("Connection to the service closed")),
            },
            _ = tokio::signal::ctrl_c() => {
                let _ = client.cancel_connector_author(&request_id).await;
                return Err(anyhow!("Cancelled"));
            }
        }
    }
}

fn print_outcome(outcome: &ConnectorAuthorOutcome, print_script: bool) {
    print_header(&format!("Connector {}", outcome.kind));
    if let (Some(name), Some(id)) = (&outcome.script_name, &outcome.script_id) {
        println!("  Script:   {} ({})", name, common::short_id(id));
    }
    if let (Some(name), Some(id)) = (&outcome.original_script_name, &outcome.original_script_id) {
        println!("  Original: {} ({})", name, common::short_id(id));
    }
    if let Some(model) = &outcome.model {
        println!("  Model:    {} ({} attempt(s))", model, outcome.attempts);
    }
    if let Some(validation) = &outcome.validation {
        println!();
        for line in validation.report().lines() {
            println!("  {}", line);
        }
    }
    if let Some(diff) = &outcome.diff {
        println!();
        print!("{}", diff);
    }
    if print_script && let Some(script) = &outcome.script {
        println!();
        print!("{}", script);
    }
    println!();
}
