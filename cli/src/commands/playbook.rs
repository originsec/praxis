use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use clap::Subcommand;

use crate::client::Client;
use crate::output::{print_header, print_success};

#[derive(Subcommand)]
pub enum PlaybookCommand {
    /// List orchestrator playbooks
    List,

    /// Print a playbook's SKILL.md document
    Show {
        /// Playbook name
        name: String,
    },

    /// Create or replace a playbook from a SKILL.md file or a skill
    /// directory containing one
    Save {
        /// Path to SKILL.md (or its directory)
        path: PathBuf,
    },

    /// Delete a playbook
    Delete {
        /// Playbook name
        name: String,
    },
}

pub async fn execute(client: &Client, command: PlaybookCommand) -> Result<()> {
    match command {
        PlaybookCommand::List => list(client).await,
        PlaybookCommand::Show { name } => show(client, &name).await,
        PlaybookCommand::Save { path } => save(client, &path).await,
        PlaybookCommand::Delete { name } => delete(client, &name).await,
    }
}

async fn list(client: &Client) -> Result<()> {
    let playbooks = client.list_playbooks().await?;
    if playbooks.is_empty() {
        println!("No playbooks");
        return Ok(());
    }

    print_header("Orchestrator Playbooks");
    println!();
    for playbook in &playbooks {
        let builtin = if playbook.is_builtin {
            " [builtin]"
        } else {
            ""
        };
        println!("  {}{}", playbook.name, builtin);
        println!("      {}", playbook.description.replace('\n', " "));
    }
    println!();
    print_success(&format!("{} playbook(s)", playbooks.len()));
    Ok(())
}

async fn show(client: &Client, name: &str) -> Result<()> {
    let playbook = client
        .list_playbooks()
        .await?
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| anyhow!("No playbook named '{}'", name))?;
    print!("{}", playbook.content);
    if !playbook.content.ends_with('\n') {
        println!();
    }
    Ok(())
}

//
// Accept either the SKILL.md file itself or a skill directory (the Agent
// Skills layout), and validate locally before sending.
//
async fn save(client: &Client, path: &Path) -> Result<()> {
    let file = if path.is_dir() {
        path.join("SKILL.md")
    } else {
        path.to_path_buf()
    };
    let content = std::fs::read_to_string(&file)
        .map_err(|e| anyhow!("Failed to read {}: {}", file.display(), e))?;
    common::playbook::parse_skill_md(&content)
        .map_err(|e| anyhow!("Invalid SKILL.md ({}): {}", file.display(), e))?;
    let name = client.save_playbook(content).await?;
    print_success(&format!("Saved playbook '{}'", name));
    Ok(())
}

async fn delete(client: &Client, name: &str) -> Result<()> {
    if client.delete_playbook(name).await? {
        print_success(&format!("Deleted playbook '{}'", name));
        Ok(())
    } else {
        Err(anyhow!("No playbook named '{}'", name))
    }
}
