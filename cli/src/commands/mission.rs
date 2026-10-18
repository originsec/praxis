use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};
use common::missions::{
    MissionBudget, MissionCommand, MissionResponse, MissionSpec, MissionStatus,
};

use crate::client::Client;

#[derive(Debug, Args)]
pub struct MissionArgs {
    /// Output JSON (watch emits one JSON response per line)
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: MissionSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum MissionSubcommand {
    /// Start a durable mission that continues after this client disconnects
    Create {
        name: String,
        objective: String,
        #[arg(long, default_value = "")]
        scope: String,
        #[arg(long)]
        model: Option<String>,
        #[command(flatten)]
        budget: BudgetArgs,
        /// Disable the independent verification pass
        #[arg(long)]
        no_verify: bool,
    },
    List,
    Show {
        id: String,
    },
    Events {
        id: String,
        #[arg(long, default_value_t = 0)]
        after: i64,
    },
    /// Poll progress; Ctrl+C detaches without cancelling the mission
    #[command(alias = "attach")]
    Watch {
        id: String,
        #[arg(long, default_value_t = 0)]
        after: i64,
    },
    /// Resume a paused/interrupted mission; reconcile uncertain actions explicitly
    Resume {
        id: String,
        #[arg(long)]
        reconciliation: Option<String>,
        /// Replace the total budget (all three limits must be supplied)
        #[arg(long, requires_all = ["max_tokens", "max_active_seconds"])]
        max_iterations: Option<u32>,
        #[arg(long, requires_all = ["max_iterations", "max_active_seconds"])]
        max_tokens: Option<u64>,
        #[arg(long, requires_all = ["max_iterations", "max_tokens"])]
        max_active_seconds: Option<u64>,
    },
    Steer {
        id: String,
        instruction: String,
    },
    Pause {
        id: String,
    },
    Cancel {
        id: String,
    },
}

#[derive(Debug, Args)]
pub struct BudgetArgs {
    #[arg(long, default_value_t = MissionBudget::default().max_iterations)]
    max_iterations: u32,
    #[arg(long, default_value_t = MissionBudget::default().max_tokens)]
    max_tokens: u64,
    #[arg(long, default_value_t = MissionBudget::default().max_active_seconds)]
    max_active_seconds: u64,
}

impl MissionSubcommand {
    pub fn into_request(self) -> Result<MissionCommand> {
        Ok(match self {
            Self::Create {
                name,
                objective,
                scope,
                model,
                budget,
                no_verify,
            } => MissionCommand::Create {
                spec: MissionSpec {
                    name,
                    objective,
                    scope,
                    model,
                    budget: MissionBudget {
                        max_iterations: budget.max_iterations,
                        max_tokens: budget.max_tokens,
                        max_active_seconds: budget.max_active_seconds,
                    },
                    verify: !no_verify,
                },
            },
            Self::List => MissionCommand::List,
            Self::Show { id } => MissionCommand::Get { id },
            Self::Events { id, after } => MissionCommand::Events { id, after },
            Self::Resume {
                id,
                reconciliation,
                max_iterations,
                max_tokens,
                max_active_seconds,
            } => {
                let budget = match (max_iterations, max_tokens, max_active_seconds) {
                    (Some(max_iterations), Some(max_tokens), Some(max_active_seconds)) => {
                        Some(MissionBudget {
                            max_iterations,
                            max_tokens,
                            max_active_seconds,
                        })
                    }
                    (None, None, None) => None,
                    _ => return Err(anyhow!("Supply all three budget limits")),
                };
                MissionCommand::Resume {
                    id,
                    reconciliation,
                    budget,
                }
            }
            Self::Steer { id, instruction } => MissionCommand::Steer { id, instruction },
            Self::Pause { id } => MissionCommand::Pause { id },
            Self::Cancel { id } => MissionCommand::Cancel { id },
            Self::Watch { .. } => return Err(anyhow!("Watch requires the streaming interface")),
        })
    }
}

#[derive(Parser)]
#[command(name = "/mission")]
struct MissionParser {
    #[command(flatten)]
    args: MissionArgs,
}

pub fn parse(input: &str) -> Result<MissionArgs> {
    let words = shlex::split(input).ok_or_else(|| anyhow!("Unclosed quote in mission command"))?;
    Ok(MissionParser::try_parse_from(std::iter::once("/mission".to_string()).chain(words))?.args)
}

pub fn format_response(response: &MissionResponse, json: bool) -> String {
    if json {
        return serde_json::to_string(response).unwrap_or_else(|e| e.to_string());
    }
    match response {
        MissionResponse::Mission(mission) => {
            let mut text = format!(
                "{}  {}  {:?}\nObjective: {}\nScope: {}\nBudget: {}/{} iterations, {}/{} tokens, {}/{} active seconds",
                mission.id,
                mission.spec.name,
                mission.status,
                mission.spec.objective,
                mission.spec.scope,
                mission.iterations,
                mission.spec.budget.max_iterations,
                mission.tokens_used,
                mission.spec.budget.max_tokens,
                mission.active_seconds,
                mission.spec.budget.max_active_seconds
            );
            if !mission.checkpoint.summary.is_empty() {
                text.push_str(&format!("\nCheckpoint: {}", mission.checkpoint.summary));
            }
            for (label, values) in [
                ("Findings", &mission.checkpoint.findings),
                ("Evidence", &mission.checkpoint.evidence),
                ("Outstanding", &mission.checkpoint.outstanding),
                ("Uncertain actions", &mission.pending_tools),
            ] {
                if !values.is_empty() {
                    text.push_str(&format!("\n{}:\n{}", label, values.join("\n")));
                }
            }
            for (label, value) in [
                ("Result", &mission.result),
                ("Verification", &mission.verification),
                ("Error", &mission.error),
            ] {
                if let Some(value) = value {
                    text.push_str(&format!("\n{}: {}", label, value));
                }
            }
            text
        }
        MissionResponse::Missions(missions) => {
            if missions.is_empty() {
                "No missions".into()
            } else {
                missions
                    .iter()
                    .map(|m| {
                        format!(
                            "{}  {:?}  {}  {}/{} iterations",
                            m.id, m.status, m.spec.name, m.iterations, m.spec.budget.max_iterations
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        MissionResponse::Events(events) => events
            .iter()
            .map(|e| format!("{}  #{}  {}  {}", e.created_at, e.sequence, e.kind, e.data))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub async fn watch(
    client: &Client,
    id: String,
    after: i64,
    emit: impl FnMut(MissionResponse) -> bool,
) -> Result<()> {
    watch_with_request(|command| client.mission(command), id, after, emit).await
}

async fn watch_with_request<F, Fut>(
    mut request: F,
    id: String,
    mut after: i64,
    mut emit: impl FnMut(MissionResponse) -> bool,
) -> Result<()>
where
    F: FnMut(MissionCommand) -> Fut,
    Fut: std::future::Future<Output = Result<MissionResponse>>,
{
    loop {
        let response = request(MissionCommand::Events {
            id: id.clone(),
            after,
        })
        .await?;
        let MissionResponse::Events(events) = &response else {
            return Err(anyhow!("Unexpected mission events response"));
        };
        if let Some(last) = events.last() {
            after = last.sequence;
            if !emit(response) {
                return Ok(());
            }
        }
        let response = request(MissionCommand::Get { id: id.clone() }).await?;
        let MissionResponse::Mission(mission) = &response else {
            return Err(anyhow!("Unexpected mission status response"));
        };
        if mission.status != MissionStatus::Running {
            loop {
                let final_events = request(MissionCommand::Events {
                    id: id.clone(),
                    after,
                })
                .await?;
                let MissionResponse::Events(events) = &final_events else {
                    return Err(anyhow!("Unexpected mission events response"));
                };
                let Some(last) = events.last() else {
                    break;
                };
                after = last.sequence;
                if !emit(final_events) {
                    return Ok(());
                }
            }
            emit(response);
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

pub async fn execute(client: &Client, args: MissionArgs) -> Result<()> {
    if let MissionSubcommand::Watch { id, after } = args.command {
        tokio::select! {
            result = watch(client, id, after, |response| { println!("{}", format_response(&response, args.json)); true }) => result,
            _ = tokio::signal::ctrl_c() => { eprintln!("Detached; the mission continues. Use mission pause/cancel to stop it."); Ok(()) }
        }
    } else {
        let response = client.mission(args.command.into_request()?).await?;
        println!("{}", format_response(&response, args.json));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_objective_and_explicit_budget_survive_parsing() {
        let args =
            parse("create audit 'inspect two agents' --scope 'node abc' --max-iterations 8 --json")
                .unwrap();
        assert!(args.json);
        let MissionCommand::Create { spec } = args.command.into_request().unwrap() else {
            panic!()
        };
        assert_eq!(spec.objective, "inspect two agents");
        assert_eq!(spec.scope, "node abc");
        assert_eq!(spec.budget.max_iterations, 8);
        assert!(spec.verify);
    }

    #[test]
    fn resume_requires_complete_budget_and_preserves_reconciliation() {
        assert!(parse("resume abc --max-tokens 100").is_err());
        let args = parse("resume abc --reconciliation 'confirmed file write' --max-iterations 10 --max-tokens 1000 --max-active-seconds 60").unwrap();
        let MissionCommand::Resume {
            reconciliation,
            budget,
            ..
        } = args.command.into_request().unwrap()
        else {
            panic!()
        };
        assert_eq!(reconciliation.as_deref(), Some("confirmed file write"));
        assert_eq!(budget.unwrap().max_tokens, 1000);
    }

    #[test]
    fn watch_and_attach_share_read_only_behavior() {
        assert!(matches!(
            parse("attach abc --after 42").unwrap().command,
            MissionSubcommand::Watch { after: 42, .. }
        ));
        assert!(matches!(
            parse("watch abc").unwrap().command,
            MissionSubcommand::Watch { after: 0, .. }
        ));
        assert!(parse("steer abc 'unfinished").is_err());
    }

    #[tokio::test]
    async fn watching_completed_mission_drains_every_event_page() {
        use common::missions::{Mission, MissionCheckpoint, MissionEvent};
        let event = |sequence| {
            MissionResponse::Events(vec![MissionEvent {
                mission_id: "abc".into(),
                sequence,
                kind: "checkpoint".into(),
                data: serde_json::Value::Null,
                created_at: String::new(),
            }])
        };
        let completed = MissionResponse::Mission(Mission {
            id: "abc".into(),
            spec: MissionSpec {
                name: "audit".into(),
                objective: "inspect".into(),
                scope: String::new(),
                model: None,
                budget: MissionBudget::default(),
                verify: true,
            },
            status: MissionStatus::Completed,
            checkpoint: MissionCheckpoint::default(),
            iterations: 1,
            tokens_used: 1,
            active_seconds: 1,
            pending_tools: vec![],
            result: Some("done".into()),
            verification: None,
            error: None,
            created_at: String::new(),
            updated_at: String::new(),
        });
        let mut replies = std::collections::VecDeque::from(vec![
            event(500),
            completed,
            event(1000),
            event(1500),
            MissionResponse::Events(vec![]),
        ]);
        let mut cursors = Vec::new();
        let mut output = Vec::new();
        watch_with_request(
            |command| {
                if let MissionCommand::Events { after, .. } = command {
                    cursors.push(after);
                }
                std::future::ready(Ok(replies.pop_front().expect("unexpected extra request")))
            },
            "abc".into(),
            0,
            |response| {
                output.push(response);
                true
            },
        )
        .await
        .unwrap();
        assert_eq!(cursors, vec![0, 500, 1000, 1500]);
        assert_eq!(output.len(), 4);
        assert!(
            matches!(output.last(), Some(MissionResponse::Mission(mission)) if mission.status == MissionStatus::Completed)
        );
    }
}
