use anyhow::{Result, anyhow};
use clap::Subcommand;
use common::acp_ext::{
    EXT_PRAXIS_CHANGES_APPLY, EXT_PRAXIS_CHANGES_CANCEL, EXT_PRAXIS_CHANGES_GET,
    EXT_PRAXIS_CHANGES_LIST, EXT_PRAXIS_CHANGES_REVERT, EXT_PRAXIS_CHANGES_REVERT_ALL,
    EXT_PRAXIS_CHANGES_STATUS,
};
use common::config_change::{
    ChangeApplyParams, ChangeApplyResult, ChangeDetail, ChangeKind, ChangeListResult, ChangeOp,
    ChangeQuery, ChangeRevertAllParams, ChangeRevertAllResult, ChangeRevertOutcome,
    ChangeRevertParams, ChangeState, ChangeStatusParams, ChangeStatusResult, ConfigChangeRecord,
    ConfigFileKind, ConflictPolicy, ENGAGEMENT_ENV_VAR, default_operator, parse_value_arg,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::client::Client;
use crate::output::{format_short_id, print_header, print_success};

#[derive(Subcommand)]
pub enum ChangeCommand {
    /// List recorded config changes (newest first)
    List {
        #[command(flatten)]
        filter: ChangeFilterArgs,
        /// Only "active" or "reverted" changes
        #[arg(long)]
        state: Option<String>,
        /// Only "apply" (changes) or "revert" (rollback records)
        #[arg(long)]
        kind: Option<String>,
        /// Exact config file path
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        /// Print raw JSON
        #[arg(long)]
        json: bool,
    },

    /// Show one change in full, with a before/after diff
    Show {
        /// Change ID or unique prefix
        id: String,
        /// Print raw JSON (includes before/after content)
        #[arg(long)]
        json: bool,
    },

    /// Apply a recorded change to a config file on a node
    Apply {
        #[arg(short, long)]
        node: String,
        /// Absolute path of the config file on the node
        #[arg(short, long)]
        path: String,
        /// overwrite | set | merge | remove | list_append | list_remove |
        /// append_block | remove_block
        #[arg(long)]
        op: String,
        /// JSON Pointer key path, e.g. /mcpServers/name or /env/VAR
        #[arg(short, long)]
        key: Option<String>,
        /// Value (JSON, or a plain string)
        #[arg(short, long)]
        value: Option<String>,
        /// Content for overwrite / append_block
        #[arg(long, conflicts_with = "content_file")]
        content: Option<String>,
        /// Read overwrite / append_block content from a local file
        #[arg(long)]
        content_file: Option<String>,
        /// Change ID whose marker block to remove (remove_block)
        #[arg(long)]
        block_id: Option<String>,
        /// Override the file format (json, jsonc, toml, yaml, markdown, text)
        #[arg(long)]
        file_kind: Option<String>,
        /// Agent the config belongs to (inferred from recon when omitted)
        #[arg(short, long)]
        agent: Option<String>,
        #[arg(short, long, env = ENGAGEMENT_ENV_VAR)]
        engagement: Option<String>,
        /// Reason for the change
        #[arg(long)]
        note: Option<String>,
        /// Show the diff without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Revert one change (restores the original content)
    Revert {
        /// Change ID or unique prefix
        id: String,
        /// When the file changed since: abort | structured | force
        #[arg(long, default_value = "abort")]
        on_conflict: String,
        #[arg(long)]
        note: Option<String>,
    },

    /// Revert every active change for a node / agent / engagement, newest first
    RevertAll {
        #[command(flatten)]
        filter: ChangeFilterArgs,
        /// Explicit change IDs (repeatable)
        #[arg(long = "id")]
        ids: Vec<String>,
        /// Required to revert everything when no filter is given
        #[arg(long)]
        all: bool,
        /// When a file changed since: abort (skip) | structured | force
        #[arg(long, default_value = "abort")]
        on_conflict: String,
        #[arg(long)]
        note: Option<String>,
    },

    /// Drift check: does each active change still match the file on the node?
    Status {
        #[command(flatten)]
        filter: ChangeFilterArgs,
        /// Check a single change
        #[arg(long = "id")]
        id: Option<String>,
    },
}

#[derive(clap::Args)]
pub struct ChangeFilterArgs {
    /// Node ID prefix
    #[arg(short, long)]
    node: Option<String>,
    /// Agent short name
    #[arg(short, long)]
    agent: Option<String>,
    /// Engagement label
    #[arg(short, long)]
    engagement: Option<String>,
}

pub async fn execute(client: &Client, command: ChangeCommand) -> Result<()> {
    match command {
        ChangeCommand::List {
            filter,
            state,
            kind,
            path,
            limit,
            json,
        } => {
            let mut query = filter.query(client).await?;
            query.state = parse_choice::<ChangeState>(state.as_deref(), "state")?;
            query.kind = parse_choice::<ChangeKind>(kind.as_deref(), "kind")?;
            query.path = path;
            query.limit = limit;
            let value = client
                .service_request(EXT_PRAXIS_CHANGES_LIST, json!(query))
                .await?;
            if json {
                return print_json(&value);
            }
            let result: ChangeListResult = serde_json::from_value(value)?;
            print_list(&result.changes);
            Ok(())
        }
        ChangeCommand::Show { id, json } => {
            let value = client
                .service_request(EXT_PRAXIS_CHANGES_GET, json!({ "change_id": id }))
                .await?;
            if json {
                return print_json(&value);
            }
            let detail: ChangeDetail = serde_json::from_value(value)?;
            print_detail(&detail);
            Ok(())
        }
        ChangeCommand::Apply {
            node,
            path,
            op,
            key,
            value,
            content,
            content_file,
            block_id,
            file_kind,
            agent,
            engagement,
            note,
            dry_run,
        } => {
            let content = match content_file {
                Some(f) => {
                    Some(std::fs::read_to_string(&f).map_err(|e| anyhow!("reading {}: {}", f, e))?)
                }
                None => content,
            };
            let change_op = ChangeOp::from_parts(
                &op,
                key.as_deref(),
                value.as_deref().map(parse_value_arg),
                content.as_deref(),
                block_id.as_deref(),
            )?;
            let file_kind = file_kind
                .map(|k| {
                    ConfigFileKind::parse(&k).ok_or_else(|| anyhow!("unknown file kind '{}'", k))
                })
                .transpose()?;
            let token = uuid::Uuid::new_v4().to_string();
            let params = ChangeApplyParams {
                node_id: resolve_node(client, &node).await?,
                agent_short_name: agent,
                path,
                file_kind,
                config_type: None,
                op: change_op,
                engagement,
                note,
                operator: Some(default_operator("cli")),
                dry_run,
                cancel_token: Some(token.clone()),
            };
            let value = cancellable(client, &token, EXT_PRAXIS_CHANGES_APPLY, &params).await?;
            let result: ChangeApplyResult = serde_json::from_value(value)?;
            print_apply(&result);
            Ok(())
        }
        ChangeCommand::Revert {
            id,
            on_conflict,
            note,
        } => {
            let token = uuid::Uuid::new_v4().to_string();
            let params = ChangeRevertParams {
                change_id: id,
                on_conflict: parse_policy(&on_conflict)?,
                operator: Some(default_operator("cli")),
                note,
                cancel_token: Some(token.clone()),
            };
            let value = cancellable(client, &token, EXT_PRAXIS_CHANGES_REVERT, &params).await?;
            let outcome: ChangeRevertOutcome = serde_json::from_value(value)?;
            print_outcome(&outcome, true);
            if outcome.outcome.is_success() {
                Ok(())
            } else {
                Err(anyhow!("revert {}", outcome.outcome.as_str()))
            }
        }
        ChangeCommand::RevertAll {
            filter,
            ids,
            all,
            on_conflict,
            note,
        } => {
            let mut query = filter.query(client).await?;
            query.change_ids = ids;
            let token = uuid::Uuid::new_v4().to_string();
            let params = ChangeRevertAllParams {
                query,
                all,
                on_conflict: parse_policy(&on_conflict)?,
                operator: Some(default_operator("cli")),
                note,
                cancel_token: Some(token.clone()),
            };
            let value = cancellable(client, &token, EXT_PRAXIS_CHANGES_REVERT_ALL, &params).await?;
            let result: ChangeRevertAllResult = serde_json::from_value(value)?;
            print_header("Revert all");
            println!();
            for o in &result.outcomes {
                print_outcome(o, false);
            }
            let ok = result
                .outcomes
                .iter()
                .filter(|o| o.outcome.is_success())
                .count();
            println!();
            if result.cancelled {
                return Err(anyhow!(
                    "cancelled after {} of {} change(s)",
                    ok,
                    result.outcomes.len()
                ));
            }
            if ok == result.outcomes.len() {
                print_success(&format!("{} change(s) reverted", ok));
                Ok(())
            } else {
                Err(anyhow!(
                    "{} of {} change(s) reverted; resolve the rest individually",
                    ok,
                    result.outcomes.len()
                ))
            }
        }
        ChangeCommand::Status { filter, id } => {
            let mut query = filter.query(client).await?;
            query.change_ids = id.into_iter().collect();
            let token = uuid::Uuid::new_v4().to_string();
            let params = ChangeStatusParams {
                query,
                cancel_token: Some(token.clone()),
            };
            let value = cancellable(client, &token, EXT_PRAXIS_CHANGES_STATUS, &params).await?;
            let result: ChangeStatusResult = serde_json::from_value(value)?;
            print_header("Change drift");
            println!();
            if result.entries.is_empty() {
                println!("  No active changes");
            }
            for e in &result.entries {
                println!(
                    "  {}  {:<11}  {}  {}{}",
                    format_short_id(&e.change_id),
                    e.status.as_str(),
                    format_short_id(&e.node_id),
                    e.path,
                    e.detail
                        .as_ref()
                        .map(|d| format!("  ({})", d))
                        .unwrap_or_default()
                );
            }
            Ok(())
        }
    }
}

impl ChangeFilterArgs {
    async fn query(&self, client: &Client) -> Result<ChangeQuery> {
        let node_id = match &self.node {
            Some(prefix) => Some(resolve_node(client, prefix).await?),
            None => None,
        };
        Ok(ChangeQuery {
            node_id,
            agent_short_name: self.agent.clone(),
            engagement: self.engagement.clone(),
            ..Default::default()
        })
    }
}

async fn resolve_node(client: &Client, prefix: &str) -> Result<String> {
    let state = client
        .get_state()
        .await
        .ok_or_else(|| anyhow!("No state available"))?;
    super::find_node_id(&state, prefix).map_err(|e| anyhow!("Node '{}': {}", prefix, e))
}

//
// Send a ledger request, cancelling it on the service if the user hits
// Ctrl+C before it completes.
//

async fn cancellable<T: Serialize>(
    client: &Client,
    token: &str,
    method: &str,
    params: &T,
) -> Result<Value> {
    tokio::select! {
        r = client.service_request(method, json!(params)) => r,
        _ = tokio::signal::ctrl_c() => {
            let _ = client
                .service_notification(EXT_PRAXIS_CHANGES_CANCEL, json!({ "cancel_token": token }))
                .await;
            Err(anyhow!("cancelled"))
        }
    }
}

fn parse_choice<T: serde::de::DeserializeOwned>(
    raw: Option<&str>,
    what: &str,
) -> Result<Option<T>> {
    raw.map(|s| {
        serde_json::from_value(Value::String(s.to_lowercase()))
            .map_err(|_| anyhow!("invalid {} '{}'", what, s))
    })
    .transpose()
}

fn parse_policy(raw: &str) -> Result<ConflictPolicy> {
    ConflictPolicy::parse(&raw.to_lowercase())
        .ok_or_else(|| anyhow!("on-conflict must be abort, structured or force"))
}

fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn status_label(c: &ConfigChangeRecord) -> String {
    match (c.kind, c.state) {
        (ChangeKind::Revert, _) => "revert".to_string(),
        (_, ChangeState::Reverted) => "reverted".to_string(),
        (_, ChangeState::Active) => match c.drift_status {
            Some(d) => format!("active/{}", d.as_str()),
            None => "active".to_string(),
        },
    }
}

fn print_list(changes: &[ConfigChangeRecord]) {
    if changes.is_empty() {
        println!("No recorded changes");
        return;
    }
    print_header("Config changes");
    println!();
    for c in changes {
        println!(
            "  {}  {}  {}  {:<12}  {:<16}  {:<12}  {}  {}",
            format_short_id(&c.id),
            c.created_at.format("%Y-%m-%d %H:%M:%S"),
            format_short_id(&c.node_id),
            c.agent_short_name.as_deref().unwrap_or("-"),
            status_label(c),
            c.engagement.as_deref().unwrap_or("-"),
            c.summary,
            c.path
        );
    }
    println!();
    print_success(&format!("{} change(s)", changes.len()));
}

fn print_detail(d: &ChangeDetail) {
    let c = &d.change;
    print_header(&format!("Change {}", format_short_id(&c.id)));
    println!();
    println!("  ID:          {}", c.id);
    println!("  Time:        {}", c.created_at.to_rfc3339());
    println!("  Kind:        {}", c.kind.as_str());
    println!("  Status:      {}", status_label(c));
    println!("  Node:        {}", c.node_id);
    println!(
        "  Agent:       {}",
        c.agent_short_name.as_deref().unwrap_or("-")
    );
    println!("  Path:        {}", c.path);
    println!(
        "  File:        {}{}",
        c.file_kind.as_str(),
        c.config_type
            .as_ref()
            .map(|t| format!(" ({})", t))
            .unwrap_or_default()
    );
    println!("  Operation:   {}", c.summary);
    println!("  Operator:    {}", c.operator);
    println!("  Engagement:  {}", c.engagement.as_deref().unwrap_or("-"));
    println!("  Note:        {}", c.note.as_deref().unwrap_or("-"));
    println!(
        "  Before:      {}",
        if c.before_exists {
            c.before_sha256.as_deref().unwrap_or("-")
        } else {
            "(file did not exist)"
        }
    );
    println!(
        "  After:       {}",
        if c.after_exists {
            c.after_sha256.as_deref().unwrap_or("-")
        } else {
            "(file removed)"
        }
    );
    if let Some(r) = &c.reverts_change_id {
        println!("  Reverts:     {}", r);
    }
    if let Some(r) = &c.reverted_by {
        println!(
            "  Reverted by: {} at {}",
            r,
            c.reverted_at.map(|t| t.to_rfc3339()).unwrap_or_default()
        );
    }
    println!();
    if d.diff.is_empty() {
        println!("  (no content difference)");
    } else {
        print!("{}", d.diff);
    }
}

fn print_apply(r: &ChangeApplyResult) {
    if r.no_op {
        print_success("No change needed (file already in the requested state)");
        return;
    }
    if !r.diff.is_empty() {
        print!("{}", r.diff);
        println!();
    }
    match &r.change {
        Some(c) => print_success(&format!(
            "Change {} recorded ({})",
            format_short_id(&c.id),
            c.id
        )),
        None if r.dry_run => print_success("Dry run: nothing written"),
        None => {}
    }
}

fn print_outcome(o: &ChangeRevertOutcome, show_diff: bool) {
    println!(
        "  {}  {:<19}  {}  {}  {}",
        format_short_id(&o.change_id),
        o.outcome.as_str(),
        format_short_id(&o.node_id),
        o.path,
        o.message
    );
    if show_diff && let Some(diff) = &o.diff {
        println!();
        println!("  Changes made to the file since Praxis wrote it:");
        print!("{}", diff);
        println!();
        if o.structured_undo_available {
            println!(
                "  Re-run with --on-conflict structured to undo only this change, or --on-conflict force to restore the original."
            );
        } else {
            println!("  Re-run with --on-conflict force to restore the original content.");
        }
    }
}
