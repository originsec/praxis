use anyhow::{anyhow, Result};
use clap::Subcommand;
use common::{InterceptedTrafficEntry, TrafficSearchFilters};
use serde_json::json;

use crate::client::CliClient;
use crate::output::{format_short_id, print_error, print_header, print_json, print_success, OutputFormat};

#[derive(Subcommand)]
pub enum TrafficCommand {
    /// Search traffic with regex pattern
    Search {
        /// Regex pattern to search
        pattern: String,

        /// Filter by node ID prefix
        #[arg(short, long)]
        node: Option<String>,

        /// Filter by agent short name
        #[arg(short, long)]
        agent: Option<String>,

        /// Maximum results
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },
}

pub async fn execute(client: &mut CliClient, command: TrafficCommand, output: &OutputFormat) -> Result<()> {
    match command {
        TrafficCommand::Search { pattern, node, agent, limit } => {
            search_traffic(client, &pattern, node, agent, limit, output).await
        }
    }
}

async fn search_traffic(
    client: &CliClient,
    pattern: &str,
    node_prefix: Option<String>,
    agent: Option<String>,
    limit: usize,
    output: &OutputFormat,
) -> Result<()> {
    //
    // Resolve node_id from prefix if provided.
    //
    let resolved_node_id = if let Some(ref prefix) = node_prefix {
        let state = client.get_state().await.ok_or_else(|| anyhow!("No state available"))?;
        state.nodes.iter()
            .find(|n| n.node_id.to_lowercase().starts_with(&prefix.to_lowercase()))
            .map(|n| n.node_id.clone())
    } else {
        None
    };

    let filters = TrafficSearchFilters {
        regex_pattern: pattern.to_string(),
        node_id: resolved_node_id,
        agent_short_name: agent,
        limit,
        offset: 0,
    };

    let (entries, total_count): (Vec<InterceptedTrafficEntry>, usize) = client.search_traffic(filters).await?;

    if entries.is_empty() {
        match output {
            OutputFormat::Json => print_json(&json!({"entries": [], "total_count": 0})),
            OutputFormat::Text => print_error("No matching traffic entries found"),
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => {
            let entries_json: Vec<_> = entries.iter().map(|e| {
                let request_body = e.request_body.as_ref()
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .map(String::from);
                let response_body = e.response_body.as_ref()
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .map(String::from);

                json!({
                    "id": e.id,
                    "timestamp": e.timestamp.to_rfc3339(),
                    "node_id": &e.node_id,
                    "agent": &e.agent_short_name,
                    "direction": format!("{:?}", e.direction),
                    "method": &e.method,
                    "url": &e.url,
                    "host": &e.host,
                    "request_headers": &e.request_headers,
                    "request_body": request_body,
                    "response_status": e.response_status,
                    "response_headers": &e.response_headers,
                    "response_body": response_body
                })
            }).collect();

            print_json(&json!({
                "entries": entries_json,
                "returned_count": entries.len(),
                "total_count": total_count
            }));
        }
        OutputFormat::Text => {
            print_header(&format!("Traffic Search Results ({})", pattern));
            println!();

            for entry in &entries {
                let node_short = format_short_id(&entry.node_id);

                let status = entry.response_status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "-".to_string());

                println!(
                    "  [{}] {} {} {} -> {} [{}]",
                    entry.timestamp.format("%H:%M:%S"),
                    node_short,
                    &entry.agent_short_name,
                    entry.method.as_ref().map(|s| s.as_str()).unwrap_or("-"),
                    &entry.url,
                    status
                );
            }

            println!();
            print_success(&format!("Found {} entries (showing {})", total_count, entries.len()));
        }
    }

    Ok(())
}
