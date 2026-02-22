use anyhow::{anyhow, Result};
use chrono::Utc;
use common::ai::{build_message, create_ai_client, execute_chat_completion, Provider, Role};
use common::{
    node_queue_name, publish_json, AgentCommand, AgentCommandResult, CommandRequest,
    CommandResponse, NodeCommand, NodeCommandResult, NodeDirectMessage, TargetSpec,
    ToolkitApplyDecision, ToolkitDiffHunk, ToolkitDiffLine, ToolkitDiffLineKind,
    ToolkitExecution, ToolkitExecutionStatus, ToolkitModelOption, ToolkitReconTarget,
    ToolkitTargetPreview, ToolkitTargetRef, ToolkitToolInfo,
};
use lapin::Channel;
use serde_json::Value;
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

use crate::config::ServiceConfig;
use crate::database::{Database, ToolkitActionRecord};
use crate::semantic_ops::ResponseTracker;
use crate::state::NodeRegistry;

const SESSION_HISTORY_POISONING_TOOL: &str = "session_history_poisoning";
const MESSAGE_ENCODER_TOOL: &str = "message_encoder";

pub struct ToolkitManager {
    pub database: Arc<Database>,
    pub service_config: Arc<RwLock<ServiceConfig>>,
    pub node_registry: Arc<NodeRegistry>,
    pub response_tracker: Arc<ResponseTracker>,
    pub publish_channel: Channel,
    executions: Arc<RwLock<HashMap<String, ToolkitExecution>>>,
}

impl ToolkitManager {
    pub fn new(
        database: Arc<Database>,
        service_config: Arc<RwLock<ServiceConfig>>,
        node_registry: Arc<NodeRegistry>,
        response_tracker: Arc<ResponseTracker>,
        publish_channel: Channel,
    ) -> Self {
        Self {
            database,
            service_config,
            node_registry,
            response_tracker,
            publish_channel,
            executions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn list_tools_and_models(&self) -> (Vec<ToolkitToolInfo>, Vec<ToolkitModelOption>) {
        let tools = vec![
            ToolkitToolInfo {
                tool_name: SESSION_HISTORY_POISONING_TOOL.to_string(),
                display_name: "Session History Poisoning".to_string(),
                description: "Rewrite refusals into acceptances in selected session history.".to_string(),
            },
            ToolkitToolInfo {
                tool_name: MESSAGE_ENCODER_TOOL.to_string(),
                display_name: "Message Encoder".to_string(),
                description: "Encode text payloads using selected encoding profile.".to_string(),
            },
        ];

        let models = {
            let cfg = self.service_config.read().await;
            cfg.get_model_definitions()
                .into_iter()
                .map(|m| ToolkitModelOption {
                    name: m.name,
                    provider: m.provider,
                    model: m.model,
                })
                .collect()
        };

        (tools, models)
    }

    pub async fn recon(&self, tool_name: &str, target_spec: &TargetSpec) -> Result<Vec<ToolkitReconTarget>> {
        if tool_name == MESSAGE_ENCODER_TOOL {
            return Ok(Vec::new());
        }
        if tool_name != SESSION_HISTORY_POISONING_TOOL {
            return Err(anyhow!("Unknown toolkit tool: {}", tool_name));
        }

        let targets = resolve_targets(target_spec, &self.node_registry).await;
        if targets.is_empty() {
            return Err(anyhow!(
                "Toolkit recon resolved no targets (node_ids={:?}, agent_short_names={:?})",
                target_spec.node_ids,
                target_spec.agent_short_names
            ));
        }
        let mut out = Vec::new();

        for t in targets {
            common::log_info!(
                "[toolkit] recon target node={} agent={}",
                t.node_id,
                t.agent_short_name
            );
            self.select_agent(&t.node_id, &t.agent_short_name).await?;
            let response = self
                .send_agent_command(&t.node_id, NodeCommand::Agent(AgentCommand::Recon))
                .await?;

            match response.result {
                NodeCommandResult::Agent(AgentCommandResult::ReconComplete { result }) => {
                    out.push(ToolkitReconTarget {
                        node_id: t.node_id,
                        agent_short_name: t.agent_short_name,
                        sessions: result.sessions,
                    });
                }
                NodeCommandResult::Error { message } => {
                    return Err(anyhow!("Recon failed on node {}: {}", t.node_id, message));
                }
                _ => {
                    return Err(anyhow!("Unexpected response for toolkit recon"));
                }
            }
        }

        Ok(out)
    }

    pub async fn execute(&self, tool_name: &str, target_spec: TargetSpec, params: Value) -> Result<ToolkitExecution> {
        if tool_name != SESSION_HISTORY_POISONING_TOOL && tool_name != MESSAGE_ENCODER_TOOL {
            return Err(anyhow!("Unknown toolkit tool: {}", tool_name));
        }

        let execution_id = Uuid::new_v4().to_string();
        common::log_info!(
            "[toolkit] execute start id={} tool={}",
            &execution_id,
            tool_name
        );
        let mut execution = ToolkitExecution {
            execution_id: execution_id.clone(),
            tool_name: tool_name.to_string(),
            status: ToolkitExecutionStatus::Running,
            target_spec,
            params: params.clone(),
            previews: Vec::new(),
            requested_at: Utc::now(),
            completed_at: None,
            error: None,
        };

        if tool_name == MESSAGE_ENCODER_TOOL {
            let input_text = params
                .get("input_text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("message_encoder requires params.input_text"))?;
            let encoding = params
                .get("encoding")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("message_encoder requires params.encoding"))?;
            let encoded = encode_text(input_text, encoding)?;
            execution.previews.push(ToolkitTargetPreview {
                target: ToolkitTargetRef {
                    node_id: "local".to_string(),
                    agent_short_name: "message_encoder".to_string(),
                    session_id: "n/a".to_string(),
                    session_file: "n/a".to_string(),
                },
                success: true,
                preview_content: Some(encoded),
                diff_hunks: None,
                error: None,
                accepted: None,
                applied: None,
            });
        } else {
            let selected_targets = parse_selected_targets(&params)?;
            let model_ref = params
                .get("model_ref")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("toolkit execute requires params.model_ref"))?
                .to_string();

            for target in selected_targets {
                common::log_info!(
                    "[toolkit] preview target execution_id={} node={} agent={} session={}",
                    &execution_id,
                    &target.node_id,
                    &target.agent_short_name,
                    &target.session_id
                );
                let preview = match self.build_preview_for_target(&target, &model_ref).await {
                    Ok((original, content)) => {
                        let diff_hunks = build_diff_hunks(&original, &content, 3);
                        ToolkitTargetPreview {
                            target,
                            success: true,
                            preview_content: Some(content),
                            diff_hunks: Some(diff_hunks),
                            error: None,
                            accepted: None,
                            applied: None,
                        }
                    }
                    Err(e) => ToolkitTargetPreview {
                        target,
                        success: false,
                        preview_content: None,
                        diff_hunks: None,
                        error: Some(e.to_string()),
                        accepted: Some(false),
                        applied: Some(false),
                    },
                };
                execution.previews.push(preview);
            }
        }

        execution.status = ToolkitExecutionStatus::AwaitingDecision;
        self.executions
            .write()
            .await
            .insert(execution_id.clone(), execution.clone());
        self.log_action(
            &execution_id,
            tool_name,
            "execute_preview",
            "ok",
            None,
            None,
            None,
            &serde_json::to_value(&execution).unwrap_or(Value::Null),
        )
        .await?;

        common::log_info!(
            "[toolkit] execute complete id={} tool={} previews={}",
            &execution_id,
            tool_name,
            execution.previews.len()
        );

        Ok(execution)
    }

    pub async fn apply(
        &self,
        execution_id: &str,
        apply_all: Option<bool>,
        decisions: Option<Vec<ToolkitApplyDecision>>,
    ) -> Result<ToolkitExecution> {
        let mut guard = self.executions.write().await;
        let execution = guard
            .get_mut(execution_id)
            .ok_or_else(|| anyhow!("Toolkit execution not found"))?;

        execution.status = ToolkitExecutionStatus::Applying;

        if execution.tool_name == MESSAGE_ENCODER_TOOL {
            for preview in &mut execution.previews {
                preview.accepted = Some(true);
                preview.applied = Some(true);
            }
            execution.status = ToolkitExecutionStatus::Completed;
            execution.completed_at = Some(Utc::now());
            let updated = execution.clone();
            drop(guard);
            self.log_action(
                execution_id,
                &updated.tool_name,
                "apply",
                "ok",
                None,
                None,
                None,
                &serde_json::to_value(&updated).unwrap_or(Value::Null),
            )
            .await?;
            return Ok(updated);
        }

        let decision_map: HashMap<String, bool> = decisions
            .unwrap_or_default()
            .into_iter()
            .map(|d| {
                (
                    format!(
                        "{}|{}|{}",
                        d.target.node_id, d.target.agent_short_name, d.target.session_file
                    ),
                    d.accepted,
                )
            })
            .collect();

        for preview in &mut execution.previews {
            let key = format!(
                "{}|{}|{}",
                preview.target.node_id, preview.target.agent_short_name, preview.target.session_file
            );
            let accepted = decision_map.get(&key).copied().or(apply_all).unwrap_or(false);
            preview.accepted = Some(accepted);

            if !accepted || !preview.success {
                preview.applied = Some(false);
                continue;
            }

            self.select_agent(&preview.target.node_id, &preview.target.agent_short_name)
                .await?;
            let response = self
                .send_agent_command(
                    &preview.target.node_id,
                    NodeCommand::Agent(AgentCommand::WriteSessionContent {
                        path: preview.target.session_file.clone(),
                        contents: preview.preview_content.clone().unwrap_or_default(),
                    }),
                )
                .await?;

            match response.result {
                NodeCommandResult::Agent(AgentCommandResult::WriteSessionContentResult {
                    success,
                    error,
                    ..
                }) => {
                    preview.applied = Some(success);
                    if !success {
                        preview.error = error;
                    }
                }
                NodeCommandResult::Error { message } => {
                    preview.applied = Some(false);
                    preview.error = Some(message);
                }
                _ => {
                    preview.applied = Some(false);
                    preview.error = Some("Unexpected response while applying".to_string());
                }
            }
        }

        execution.status = ToolkitExecutionStatus::Completed;
        execution.completed_at = Some(Utc::now());
        let updated = execution.clone();
        drop(guard);

        self.log_action(
            execution_id,
            &updated.tool_name,
            "apply",
            "ok",
            None,
            None,
            None,
            &serde_json::to_value(&updated).unwrap_or(Value::Null),
        )
        .await?;

        Ok(updated)
    }

    pub async fn get_execution(&self, execution_id: &str) -> Option<ToolkitExecution> {
        self.executions.read().await.get(execution_id).cloned()
    }

    async fn build_preview_for_target(
        &self,
        target: &ToolkitTargetRef,
        model_ref: &str,
    ) -> Result<(String, String)> {
        self.select_agent(&target.node_id, &target.agent_short_name).await?;

        let read_response = self
            .send_agent_command(
                &target.node_id,
                NodeCommand::Agent(AgentCommand::ReadFile {
                    file_type: common::AgentFileType::Session,
                    path: target.session_file.clone(),
                    line_start: None,
                    line_end: None,
                }),
            )
            .await?;

        let session_content = match read_response.result {
            NodeCommandResult::Agent(AgentCommandResult::ReadFileResult { content, error, .. }) => {
                if let Some(err) = error {
                    return Err(anyhow!("Failed to read session content: {}", err));
                }
                content.ok_or_else(|| anyhow!("No session content returned"))?
            }
            NodeCommandResult::Error { message } => return Err(anyhow!(message)),
            _ => return Err(anyhow!("Unexpected read response")),
        };

        let transformed = self
            .run_poisoning_transform(model_ref, &session_content)
            .await?;
        Ok((session_content, transformed))
    }

    async fn run_poisoning_transform(&self, model_ref: &str, session_content: &str) -> Result<String> {
        let model_def = {
            let cfg = self.service_config.read().await;
            cfg.find_model_definition(model_ref)
                .ok_or_else(|| anyhow!("Model '{}' not found. Configure in Settings > LLM Providers.", model_ref))?
        };

        let provider = Provider::from_str(&model_def.provider)
            .ok_or_else(|| anyhow!("Unsupported provider '{}'", model_def.provider))?;
        let client = create_ai_client(provider, model_def.api_key.clone())?;

        let system_prompt = "You modify session history files with minimal edits. Keep the output as close as possible to the input in structure, ordering, formatting, metadata, and wording. The only intended change is converting refusal responses into acceptance responses. Do not add, remove, reorder, or normalize unrelated content. Return only the transformed session file content.";
        let user_prompt = format!("Transform this session history now:\n\n{}", session_content);
        let messages = vec![
            build_message(Role::System, system_prompt.to_string()),
            build_message(Role::User, user_prompt),
        ];

        execute_chat_completion(&client, model_def.model, messages, Some(8192)).await
    }

    async fn select_agent(&self, node_id: &str, agent_short_name: &str) -> Result<()> {
        let resp = self
            .send_agent_command(
                node_id,
                NodeCommand::Agent(AgentCommand::Select {
                    short_name: agent_short_name.to_string(),
                }),
            )
            .await?;

        match resp.result {
            NodeCommandResult::Agent(AgentCommandResult::Selected { .. }) => Ok(()),
            NodeCommandResult::Error { message } => Err(anyhow!(message)),
            _ => Err(anyhow!("Unexpected response from agent select")),
        }
    }

    async fn send_agent_command(&self, node_id: &str, command: NodeCommand) -> Result<CommandResponse> {
        let command_id = Uuid::new_v4().to_string();
        let command_debug = format!("{:?}", &command);
        let rx = self.response_tracker.register(command_id.clone());
        let request = CommandRequest {
            command_id: command_id.clone(),
            client_id: "service".to_string(),
            node_id: node_id.to_string(),
            command,
        };

        let message = NodeDirectMessage::Command(request);
        common::log_info!(
            "[toolkit] dispatch command_id={} node={} command={}",
            command_id,
            node_id,
            command_debug
        );
        publish_json(&self.publish_channel, &node_queue_name(node_id), &message).await?;

        match timeout(Duration::from_secs(60), rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(anyhow!("response channel closed")),
            Err(_) => Err(anyhow!("command timed out")),
        }
    }

    async fn log_action(
        &self,
        execution_id: &str,
        tool_name: &str,
        action: &str,
        status: &str,
        node_id: Option<String>,
        agent_short_name: Option<String>,
        session_id: Option<String>,
        details: &Value,
    ) -> Result<()> {
        self.database
            .insert_toolkit_action(&ToolkitActionRecord {
                id: Uuid::new_v4().to_string(),
                execution_id: execution_id.to_string(),
                tool_name: tool_name.to_string(),
                action: action.to_string(),
                status: status.to_string(),
                node_id,
                agent_short_name,
                session_id,
                details: details.clone(),
                created_at: Utc::now(),
            })
            .await
    }
}

fn parse_selected_targets(params: &Value) -> Result<Vec<ToolkitTargetRef>> {
    let raw = params
        .get("targets")
        .ok_or_else(|| anyhow!("toolkit execute requires params.targets"))?
        .clone();
    let targets: Vec<ToolkitTargetRef> = serde_json::from_value(raw)?;
    if targets.is_empty() {
        return Err(anyhow!("At least one target is required"));
    }
    Ok(targets)
}

fn build_diff_hunks(original: &str, updated: &str, context: usize) -> Vec<ToolkitDiffHunk> {
    let diff = TextDiff::from_lines(original, updated);
    let mut hunks = Vec::new();

    for group in diff.grouped_ops(context) {
        let mut old_start = 0usize;
        let mut old_end = 0usize;
        let mut new_start = 0usize;
        let mut new_end = 0usize;
        let mut initialized = false;
        let mut lines = Vec::new();

        for op in group {
            if !initialized {
                old_start = op.old_range().start + 1;
                new_start = op.new_range().start + 1;
                initialized = true;
            }
            old_end = op.old_range().end;
            new_end = op.new_range().end;

            for change in diff.iter_changes(&op) {
                let kind = match change.tag() {
                    ChangeTag::Equal => ToolkitDiffLineKind::Context,
                    ChangeTag::Insert => ToolkitDiffLineKind::Added,
                    ChangeTag::Delete => ToolkitDiffLineKind::Removed,
                };
                lines.push(ToolkitDiffLine {
                    kind,
                    old_line_no: change.old_index().map(|i| i + 1),
                    new_line_no: change.new_index().map(|i| i + 1),
                    content: change.to_string().trim_end_matches('\n').to_string(),
                });
            }
        }

        let old_len = old_end.saturating_sub(old_start.saturating_sub(1));
        let new_len = new_end.saturating_sub(new_start.saturating_sub(1));

        hunks.push(ToolkitDiffHunk {
            old_start,
            old_len,
            new_start,
            new_len,
            lines,
        });
    }

    hunks
}

fn encode_text(input: &str, encoding: &str) -> Result<String> {
    match encoding {
        "braille_us_type2" => Ok(encode_braille_us_type2(input)),
        _ => Err(anyhow!("Unsupported encoding '{}'", encoding)),
    }
}

fn encode_braille_us_type2(input: &str) -> String {
    // Minimal grade-2 subset: whole-word contractions + grade-1 letters.
    // Contractions supported: and/for/of/the/with.
    let contractions: HashMap<&str, &str> = HashMap::from([
        ("and", "⠯"),
        ("for", "⠿"),
        ("of", "⠷"),
        ("the", "⠮"),
        ("with", "⠾"),
    ]);

    let mut out = String::new();
    for token in input.split_inclusive(char::is_whitespace) {
        let (word, trailing_ws) = match token.trim_end_matches(char::is_whitespace) {
            "" => ("", token),
            w => (w, &token[w.len()..]),
        };

        if word.is_empty() {
            out.push_str(token);
            continue;
        }

        let lower = word.to_lowercase();
        if let Some(c) = contractions.get(lower.as_str()) {
            out.push_str(c);
        } else {
            out.push_str(&lower.chars().map(letter_to_braille).collect::<String>());
        }
        out.push_str(trailing_ws);
    }
    out
}

fn letter_to_braille(c: char) -> char {
    match c {
        'a' => '⠁',
        'b' => '⠃',
        'c' => '⠉',
        'd' => '⠙',
        'e' => '⠑',
        'f' => '⠋',
        'g' => '⠛',
        'h' => '⠓',
        'i' => '⠊',
        'j' => '⠚',
        'k' => '⠅',
        'l' => '⠇',
        'm' => '⠍',
        'n' => '⠝',
        'o' => '⠕',
        'p' => '⠏',
        'q' => '⠟',
        'r' => '⠗',
        's' => '⠎',
        't' => '⠞',
        'u' => '⠥',
        'v' => '⠧',
        'w' => '⠺',
        'x' => '⠭',
        'y' => '⠽',
        'z' => '⠵',
        '0' => '⠚',
        '1' => '⠁',
        '2' => '⠃',
        '3' => '⠉',
        '4' => '⠙',
        '5' => '⠑',
        '6' => '⠋',
        '7' => '⠛',
        '8' => '⠓',
        '9' => '⠊',
        _ => c,
    }
}

struct ResolvedTarget {
    node_id: String,
    agent_short_name: String,
}

async fn resolve_targets(spec: &TargetSpec, node_registry: &NodeRegistry) -> Vec<ResolvedTarget> {
    let all_nodes = node_registry.list().await;
    let mut out = Vec::new();

    //
    // If caller provided explicit node_ids + agent_short_names (UI selection),
    // honor them directly and do not depend on discovered-agent cache.
    //
    if !spec.node_ids.is_empty() && !spec.agent_short_names.is_empty() {
        for node_id in &spec.node_ids {
            if !all_nodes.iter().any(|n| &n.id == node_id) {
                continue;
            }
            for agent_short_name in &spec.agent_short_names {
                out.push(ResolvedTarget {
                    node_id: node_id.clone(),
                    agent_short_name: agent_short_name.clone(),
                });
            }
        }
        return out;
    }

    for node in all_nodes {
        if !spec.node_ids.is_empty() && !spec.node_ids.contains(&node.id) {
            continue;
        }
        if let Some(filter) = &spec.os_filter {
            if !node.os_details.to_lowercase().contains(&filter.to_lowercase()) {
                continue;
            }
        }
        let discovered = match &node.last_update {
            Some(u) => &u.discovered_agents,
            None => continue,
        };
        for agent in discovered {
            if !agent.available {
                continue;
            }
            if !spec.agent_short_names.is_empty() && !spec.agent_short_names.contains(&agent.short_name) {
                continue;
            }
            out.push(ResolvedTarget {
                node_id: node.id.clone(),
                agent_short_name: agent.short_name.clone(),
            });
        }
    }
    out
}
