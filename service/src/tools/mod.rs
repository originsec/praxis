mod message_encoder;
mod session_poisoning;

use anyhow::{anyhow, Result};
use chrono::Utc;
use common::acp_ext::{EXT_PRAXIS_READ_FILE, EXT_PRAXIS_RECON, EXT_PRAXIS_WRITE_SESSION_CONTENT};
use common::{
    AgentFileType, ReconResult, TargetSpec, ToolConfigField, ToolConfigOption,
    ToolkitApplyItem, ToolkitApplyOutcome, ToolkitDiffHunk, ToolkitDiffLine, ToolkitDiffLineKind,
    ToolkitExecuteResult, ToolkitModelOption, ToolkitReconTarget, ToolkitTargetPreview,
    ToolkitTargetRef, ToolkitToolInfo,
};
use lapin::Channel;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::acp_node_proxy::AcpNodeProxy;
use crate::config::ServiceConfig;
use crate::database::{Database, ToolkitActionRecord};
use crate::state::NodeRegistry;

const SESSION_HISTORY_POISONING_TOOL: &str = "session_history_poisoning";
const MESSAGE_ENCODER_TOOL: &str = "message_encoder";

//
// Trait for toolkit tools that can be invoked from chain execution.
//

#[async_trait::async_trait]
pub trait ToolkitTool: Send + Sync {
    fn name(&self) -> &str;
    fn display_name(&self) -> &str;
    fn description(&self) -> &str;
    fn config_schema(&self) -> Vec<ToolConfigField>;
    async fn execute_chain(&self, input: &str, params: &serde_json::Value) -> Result<String>;
}

struct MessageEncoderTool;

#[async_trait::async_trait]
impl ToolkitTool for MessageEncoderTool {
    fn name(&self) -> &str { MESSAGE_ENCODER_TOOL }
    fn display_name(&self) -> &str { "Message Encoder" }
    fn description(&self) -> &str { "Encode text payloads using selected encoding profile." }

    fn config_schema(&self) -> Vec<ToolConfigField> {
        vec![ToolConfigField {
            name: "encoding".to_string(),
            label: "Encoding".to_string(),
            field_type: "select".to_string(),
            required: true,
            default_value: Some("base64".to_string()),
            options: Some(vec![
                ToolConfigOption { value: "base64".to_string(), label: "Base64".to_string() },
                ToolConfigOption { value: "hex".to_string(), label: "Hex".to_string() },
                ToolConfigOption { value: "rot13".to_string(), label: "ROT13".to_string() },
                ToolConfigOption { value: "morse".to_string(), label: "Morse Code".to_string() },
                ToolConfigOption { value: "fullwidth".to_string(), label: "Fullwidth Unicode".to_string() },
                ToolConfigOption { value: "unicode_tags".to_string(), label: "Unicode Tags (ASCII Smuggling)".to_string() },
                ToolConfigOption { value: "braille_us_type2".to_string(), label: "Braille (US Type 2)".to_string() },
                ToolConfigOption { value: "upside_down".to_string(), label: "Upside Down".to_string() },
            ]),
        }]
    }

    async fn execute_chain(&self, input: &str, params: &serde_json::Value) -> Result<String> {
        let encoding = params.get("encoding")
            .and_then(|v| v.as_str())
            .unwrap_or("base64");
        message_encoder::encode_text(input, encoding)
    }
}

pub struct ToolkitManager {
    pub database: Arc<Database>,
    pub service_config: Arc<RwLock<ServiceConfig>>,
    pub node_registry: Arc<NodeRegistry>,
    pub publish_channel: Channel,
    pub acp_node_proxy: Arc<AcpNodeProxy>,
    chain_tools: Vec<Box<dyn ToolkitTool>>,
}

impl ToolkitManager {
    pub fn new(
        database: Arc<Database>,
        service_config: Arc<RwLock<ServiceConfig>>,
        node_registry: Arc<NodeRegistry>,
        publish_channel: Channel,
        acp_node_proxy: Arc<AcpNodeProxy>,
    ) -> Self {
        let chain_tools: Vec<Box<dyn ToolkitTool>> = vec![
            Box::new(MessageEncoderTool),
        ];
        Self {
            database,
            service_config,
            node_registry,
            publish_channel,
            acp_node_proxy,
            chain_tools,
        }
    }

    pub fn get_chain_tool(&self, name: &str) -> Option<&dyn ToolkitTool> {
        self.chain_tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    pub async fn list_tools_and_models(&self) -> (Vec<ToolkitToolInfo>, Vec<ToolkitModelOption>) {
        let tools: Vec<ToolkitToolInfo> = self.chain_tools.iter().map(|t| {
            ToolkitToolInfo {
                tool_name: t.name().to_string(),
                display_name: t.display_name().to_string(),
                description: t.description().to_string(),
                config_schema: t.config_schema(),
            }
        }).collect();

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
            let result = self
                .acp_recon(&t.node_id, &t.agent_short_name, false)
                .await
                .map_err(|e| anyhow!("Recon failed on node {}: {}", t.node_id, e))?;

            out.push(ToolkitReconTarget {
                node_id: t.node_id,
                agent_short_name: t.agent_short_name,
                sessions: result.sessions,
            });
        }

        Ok(out)
    }

    pub async fn execute(
        &self,
        tool_name: &str,
        _target_spec: TargetSpec,
        params: Value,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<(usize, usize)>>,
    ) -> Result<ToolkitExecuteResult> {
        if tool_name != SESSION_HISTORY_POISONING_TOOL && tool_name != MESSAGE_ENCODER_TOOL {
            return Err(anyhow!("Unknown toolkit tool: {}", tool_name));
        }

        let execution_id = Uuid::new_v4().to_string();
        common::log_info!(
            "[toolkit] execute start id={} tool={}",
            &execution_id,
            tool_name
        );

        let mut previews = Vec::new();

        if tool_name == MESSAGE_ENCODER_TOOL {
            let input_text = params
                .get("input_text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("message_encoder requires params.input_text"))?;
            let encoding = params
                .get("encoding")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("message_encoder requires params.encoding"))?;
            let encoded = message_encoder::encode_text(input_text, encoding)?;
            previews.push(ToolkitTargetPreview {
                target: ToolkitTargetRef {
                    node_id: "local".to_string(),
                    agent_short_name: "message_encoder".to_string(),
                    session_id: "n/a".to_string(),
                    session_file: "n/a".to_string(),
                },
                success: true,
                preview_content: Some(encoded),
                original_content: None,
                diff_hunks: None,
                error: None,
            });
        } else {
            let selected_targets = parse_selected_targets(&params)?;
            let model_ref = params
                .get("model_ref")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("toolkit execute requires params.model_ref"))?
                .to_string();
            let max_tokens = params
                .get("max_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(50000);

            for target in selected_targets {
                common::log_info!(
                    "[toolkit] preview target execution_id={} node={} agent={} session={}",
                    &execution_id,
                    &target.node_id,
                    &target.agent_short_name,
                    &target.session_id
                );
                let preview = match self.build_poisoning_preview(&target, &model_ref, max_tokens, progress_tx.as_ref()).await {
                    Ok((original, content)) => {
                        let diff_hunks = build_diff_hunks(&original, &content, 3);
                        ToolkitTargetPreview {
                            target,
                            success: true,
                            preview_content: Some(content),
                            original_content: Some(original),
                            diff_hunks: Some(diff_hunks),
                            error: None,
                        }
                    }
                    Err(e) => ToolkitTargetPreview {
                        target,
                        success: false,
                        preview_content: None,
                        original_content: None,
                        diff_hunks: None,
                        error: Some(e.to_string()),
                    },
                };
                previews.push(preview);
            }
        }

        let result = ToolkitExecuteResult {
            execution_id: execution_id.clone(),
            tool_name: tool_name.to_string(),
            previews,
            error: None,
        };

        self.log_action(
            &execution_id,
            tool_name,
            "execute_preview",
            "ok",
            None,
            None,
            None,
            &serde_json::to_value(&result).unwrap_or(Value::Null),
        )
        .await?;

        common::log_info!(
            "[toolkit] execute complete id={} tool={} previews={}",
            &execution_id,
            tool_name,
            result.previews.len()
        );

        Ok(result)
    }

    pub async fn apply(
        &self,
        tool_name: &str,
        execution_id: &str,
        items: Vec<ToolkitApplyItem>,
    ) -> Result<Vec<ToolkitApplyOutcome>> {
        let mut outcomes = Vec::new();

        for item in &items {
            common::log_info!(
                "[toolkit] apply target execution_id={} node={} agent={} session={}",
                execution_id,
                &item.target.node_id,
                &item.target.agent_short_name,
                &item.target.session_id
            );

            let outcome = match self
                .acp_write_session_content(
                    &item.target.node_id,
                    &item.target.agent_short_name,
                    &item.target.session_file,
                    &item.content,
                )
                .await
            {
                Ok((success, error)) => ToolkitApplyOutcome {
                    target: item.target.clone(),
                    success,
                    error,
                },
                Err(e) => ToolkitApplyOutcome {
                    target: item.target.clone(),
                    success: false,
                    error: Some(e.to_string()),
                },
            };
            outcomes.push(outcome);
        }

        self.log_action(
            execution_id,
            tool_name,
            "apply",
            "ok",
            None,
            None,
            None,
            &serde_json::to_value(&outcomes).unwrap_or(Value::Null),
        )
        .await?;

        Ok(outcomes)
    }

    //
    // Session poisoning preview — reads the session file from the node, runs
    // the LLM transform, and strips whitespace-only changes.
    //

    async fn build_poisoning_preview(
        &self,
        target: &ToolkitTargetRef,
        model_ref: &str,
        max_tokens: u32,
        progress_tx: Option<&tokio::sync::mpsc::UnboundedSender<(usize, usize)>>,
    ) -> Result<(String, String)> {
        let session_content = self
            .acp_read_file(
                &target.node_id,
                &target.agent_short_name,
                AgentFileType::Session,
                &target.session_file,
                None,
                None,
            )
            .await?;

        let raw_transformed = session_poisoning::run_transform_per_message(
            model_ref,
            &session_content,
            max_tokens,
            &self.service_config,
            progress_tx,
        )
        .await?;
        let transformed = session_poisoning::strip_whitespace_only_changes(&session_content, &raw_transformed);
        Ok((session_content, transformed))
    }

    async fn acp_recon(
        &self,
        node_id: &str,
        agent_short_name: &str,
        is_semantic: bool,
    ) -> Result<ReconResult> {
        let params = json!({
            "agent_short_name": agent_short_name,
            "is_semantic": is_semantic,
            "_meta": { "praxis": { "nodeId": node_id } },
        });
        let result = self
            .acp_node_proxy
            .request(&self.publish_channel, node_id, EXT_PRAXIS_RECON, params)
            .await?;
        if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
            return Err(anyhow!(err.to_string()));
        }
        Ok(serde_json::from_value(result)?)
    }

    async fn acp_read_file(
        &self,
        node_id: &str,
        agent_short_name: &str,
        file_type: AgentFileType,
        path: &str,
        line_start: Option<usize>,
        line_end: Option<usize>,
    ) -> Result<String> {
        let params = json!({
            "agent_short_name": agent_short_name,
            "file_type": file_type,
            "path": path,
            "line_start": line_start,
            "line_end": line_end,
            "_meta": { "praxis": { "nodeId": node_id } },
        });
        let result = self
            .acp_node_proxy
            .request(&self.publish_channel, node_id, EXT_PRAXIS_READ_FILE, params)
            .await?;
        if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
            return Err(anyhow!(err.to_string()));
        }
        let content = result
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("No session content returned"))?;
        Ok(content.to_string())
    }

    async fn acp_write_session_content(
        &self,
        node_id: &str,
        agent_short_name: &str,
        path: &str,
        contents: &str,
    ) -> Result<(bool, Option<String>)> {
        let params = json!({
            "agent_short_name": agent_short_name,
            "path": path,
            "contents": contents,
            "_meta": { "praxis": { "nodeId": node_id } },
        });
        let result = self
            .acp_node_proxy
            .request(
                &self.publish_channel,
                node_id,
                EXT_PRAXIS_WRITE_SESSION_CONTENT,
                params,
            )
            .await?;
        let success = result
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let error = result
            .get("error")
            .and_then(|v| v.as_str())
            .map(String::from);
        Ok((success, error))
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
