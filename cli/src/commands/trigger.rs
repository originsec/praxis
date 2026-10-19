use anyhow::{Result, anyhow};
use clap::Subcommand;
use common::mcp::TriggerConfigParams;
use common::{ChainTriggerInfo, TargetSpec, TriggerConfig};

use crate::client::Client;
use crate::output::{format_short_id, print_header, print_success};

#[derive(Subcommand)]
pub enum TriggerCommand {
    /// List chain triggers
    List,

    /// Create a chain trigger
    Create {
        /// Chain display name or chain ID prefix
        #[arg(long)]
        chain: String,

        /// Trigger config as JSON, same schema as the MCP trigger_create
        /// `trigger` parameter, e.g. '{"type":"agent_change","kinds":["version_changed"]}'
        #[arg(long)]
        config: String,

        /// Target node ID (repeatable; omit for all nodes)
        #[arg(long = "node")]
        nodes: Vec<String>,

        /// Case-insensitive substring filter on node OS details
        #[arg(long)]
        os_filter: Option<String>,

        /// Target agent short name (repeatable; omit for all agents)
        #[arg(long = "agent")]
        agents: Vec<String>,

        /// Event triggers: include the triggering node despite --node filters
        #[arg(long)]
        include_triggering_node: bool,

        /// Event triggers: run only on the node that caused the event
        #[arg(long)]
        triggering_node_only: bool,
    },

    /// Delete a chain trigger by ID prefix
    Delete {
        /// Trigger ID prefix
        trigger: String,
    },

    /// Enable a chain trigger by ID prefix
    Enable {
        /// Trigger ID prefix
        trigger: String,
    },

    /// Disable a chain trigger by ID prefix
    Disable {
        /// Trigger ID prefix
        trigger: String,
    },
}

pub async fn execute(client: &Client, command: TriggerCommand) -> Result<()> {
    match command {
        TriggerCommand::List => list(client).await,
        TriggerCommand::Create {
            chain,
            config,
            nodes,
            os_filter,
            agents,
            include_triggering_node,
            triggering_node_only,
        } => {
            let target = TargetSpec {
                node_ids: nodes,
                os_filter,
                agent_short_names: agents,
                include_triggering_node,
                triggering_node_only,
            };
            create(client, &chain, &config, target).await
        }
        TriggerCommand::Delete { trigger } => delete(client, &trigger).await,
        TriggerCommand::Enable { trigger } => set_enabled(client, &trigger, true).await,
        TriggerCommand::Disable { trigger } => set_enabled(client, &trigger, false).await,
    }
}

fn parse_config(json: &str) -> Result<TriggerConfig> {
    let params: TriggerConfigParams =
        serde_json::from_str(json).map_err(|e| anyhow!("Invalid --config JSON: {}", e))?;
    TriggerConfig::try_from(params).map_err(|e| anyhow!(e))
}

async fn find_trigger(client: &Client, prefix: &str) -> Result<ChainTriggerInfo> {
    let triggers = client.fetch_chain_triggers().await?;
    let matches: Vec<_> = triggers
        .into_iter()
        .filter(|t| t.id.starts_with(prefix))
        .collect();
    match matches.len() {
        0 => Err(anyhow!("No trigger found matching '{}'", prefix)),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => Err(anyhow!("Trigger prefix '{}' is ambiguous", prefix)),
    }
}

async fn list(client: &Client) -> Result<()> {
    let triggers = client.fetch_chain_triggers().await?;
    if triggers.is_empty() {
        println!("No chain triggers");
        return Ok(());
    }
    let chains = client.fetch_chain_definitions().await.unwrap_or_default();

    print_header("Chain Triggers");
    println!();
    for t in &triggers {
        let chain = chains
            .iter()
            .find(|c| c.id == t.chain_id)
            .map(|c| c.name.as_str())
            .unwrap_or(&t.chain_id);
        println!(
            "  {} [{}] {} -> {} {}",
            format_short_id(&t.id),
            if t.enabled { "on" } else { "off" },
            t.trigger_config.type_name(),
            chain,
            serde_json::to_string(&t.trigger_config).unwrap_or_default()
        );
        if matches!(t.trigger_config, TriggerConfig::Webhook) {
            println!("      webhook: POST {}", common::webhook_path(&t.id));
        }
    }
    println!();
    print_success(&format!("{} trigger(s)", triggers.len()));
    Ok(())
}

async fn create(client: &Client, chain: &str, config: &str, target: TargetSpec) -> Result<()> {
    let trigger_config = parse_config(config)?;
    let chains = client.fetch_chain_definitions().await?;
    let needle = chain.to_lowercase();
    let chain = chains
        .iter()
        .find(|c| c.id.to_lowercase().starts_with(&needle) || c.name.to_lowercase() == needle)
        .ok_or_else(|| anyhow!("No chain found matching '{}'", chain))?;

    let trigger = client
        .create_chain_trigger_wait(chain.id.clone(), trigger_config, target)
        .await?;
    print_success(&format!(
        "Created {} trigger {} for chain {}",
        trigger.trigger_config.type_name(),
        format_short_id(&trigger.id),
        chain.name
    ));
    if let Some(secret) = trigger.webhook_secret {
        println!(
            "  webhook path:   POST {}",
            common::webhook_path(&trigger.id)
        );
        println!("  webhook secret: {}", secret);
        println!("  (shown once; send as 'Authorization: Bearer <secret>')");
    }
    Ok(())
}

async fn delete(client: &Client, prefix: &str) -> Result<()> {
    let trigger = find_trigger(client, prefix).await?;
    client.delete_chain_trigger(trigger.id.clone()).await?;
    print_success(&format!("Deleted trigger {}", format_short_id(&trigger.id)));
    Ok(())
}

async fn set_enabled(client: &Client, prefix: &str, enabled: bool) -> Result<()> {
    let trigger = find_trigger(client, prefix).await?;
    let updated = client
        .update_chain_trigger_wait(trigger.id.clone(), Some(enabled), None, None)
        .await?;
    print_success(&format!(
        "Trigger {} {}",
        format_short_id(&updated.id),
        if updated.enabled {
            "enabled"
        } else {
            "disabled"
        }
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_new_trigger_type() {
        assert!(matches!(
            parse_config(r#"{"type":"webhook"}"#).unwrap(),
            TriggerConfig::Webhook
        ));
        assert!(matches!(
            parse_config(r#"{"type":"agent_change","kinds":["version_changed"]}"#).unwrap(),
            TriggerConfig::AgentChange { .. }
        ));
        assert!(matches!(
            parse_config(r#"{"type":"session_activity","quiet_period_secs":60}"#).unwrap(),
            TriggerConfig::SessionActivity {
                quiet_period_secs: 60,
                ..
            }
        ));
        assert!(matches!(
            parse_config(r#"{"type":"recon_change","kinds":["mcp_server_added"]}"#).unwrap(),
            TriggerConfig::ReconChange { .. }
        ));
    }

    #[test]
    fn rejects_bad_config() {
        assert!(parse_config("{").is_err());
        assert!(parse_config(r#"{"type":"nope"}"#).is_err());
        assert!(parse_config(r#"{"type":"recon_change","path_glob":"["}"#).is_err());
    }
}
