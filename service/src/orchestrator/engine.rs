use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use futures_util::StreamExt;
use lapin::Channel;
use serde_json::{Value, json};

use common::ClientDirectMessage;
use common::OrchestratorPlan;
use common::ai::{
    AiClient, ChatCompletionRequest, Content, ContentPart, INVALID_TOOL_ARGUMENTS_KEY, Message,
    ReasoningConfig, Role, StreamEvent, Tool, Usage, parse_manual_tool_calls,
};

use super::approval::{ApprovalGate, Decision, PermissionRegistry, denied_result};
use super::delegate::{self, DELEGATE_TOOL};
use super::markers::ManualTextFilter;
use super::memory;
use super::playbooks::{LOAD_PLAYBOOK_TOOL, load_playbook};
use super::tools::{
    execute_local_tool, execute_mcp_tool, execute_tool_calls_concurrent, result_is_error,
};
use crate::acp_server::{
    session_update_plan, session_update_text, session_update_thought, session_update_tool_call,
    session_update_tool_result, session_update_usage,
};
use crate::database::Database;
use crate::messaging::send_to_client;

//
// Model settings for a session. `native` starts from the provider default
// (or the configured override) and flips to manual for the rest of the
// session if the endpoint rejects native tools.
//

pub(super) struct AiSettings {
    pub client: AiClient,
    pub model: String,
    pub max_tokens: u32,
    pub reasoning: Option<ReasoningConfig>,
    pub context_budget: usize,
    pub native: AtomicBool,
    //
    // Base instructions (orchestrator prompt + playbook list) and the
    // top-level system prompts derived from it for each tool mode.
    //
    pub base_prompt: String,
    pub native_system: String,
    pub manual_system: String,
}

impl AiSettings {
    pub(super) fn system_prompt(&self) -> &str {
        if self.native.load(Ordering::SeqCst) {
            &self.native_system
        } else {
            &self.manual_system
        }
    }
}

//
// Everything a running loop needs. Shared (Arc) so concurrently executing
// tool calls can hold it in 'static tasks.
//

pub(super) struct SessionCtx {
    pub session_id: String,
    pub client_id: String,
    pub channel: Channel,
    pub peer: rmcp::service::Peer<rmcp::RoleClient>,
    pub ai: AiSettings,
    pub tools: Vec<Tool>,
    pub cancel: Arc<AtomicBool>,
    pub stop: Arc<AtomicBool>,
    //
    // Prompt size reported by the provider for the latest request; feeds
    // the compaction trigger alongside the local estimate.
    //
    pub last_prompt_tokens: AtomicU64,
    pub gate: ApprovalGate,
    pub permissions: Arc<PermissionRegistry>,
    pub db: Arc<Database>,
    //
    // Engagement that scopes the memory tools.
    //
    pub engagement: String,
}

impl SessionCtx {
    pub(super) fn interrupted(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) || self.stop.load(Ordering::SeqCst)
    }

    pub(super) async fn send(&self, msg: ClientDirectMessage) {
        let _ = send_to_client(&self.channel, &self.client_id, msg).await;
    }

    //
    // Resolves once cancel or stop is raised. Used in select! so a model
    // that thinks silently for minutes can still be interrupted promptly.
    //
    pub(super) async fn interrupted_signal(&self) {
        while !self.interrupted() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

pub(super) enum LoopOutcome {
    Completed,
    Interrupted,
    Failed(String),
    //
    // A bounded (sub-orchestrator) loop used all its model turns while the
    // model still wanted to call tools.
    //
    TurnLimit,
}

//
// What a loop may do: the top-level orchestrator, or a sub-orchestrator
// started by `delegate` with its own label, a reduced tool set and a turn
// bound. Sub-orchestrators do not stream text to the operator; only their
// tool calls are shown, prefixed with the label.
//

pub(super) struct Scope {
    pub label: Option<String>,
    pub tools: Vec<Tool>,
    pub manual_system: String,
    pub max_turns: Option<usize>,
}

impl Scope {
    pub(super) fn top_level(ctx: &SessionCtx) -> Self {
        Self {
            label: None,
            tools: ctx.tools.clone(),
            manual_system: ctx.ai.manual_system.clone(),
            max_turns: None,
        }
    }
}

//
// A tool call requested by the model, normalized across native and manual
// modes. `model_id` pairs the result with the call in the next request;
// `display_id` is a fresh id for the client (model ids are not guaranteed
// to be unique across turns).
//

struct PendingCall {
    model_id: String,
    display_id: String,
    name: String,
    args: Value,
}

//
// Run model turns until the model answers without calling tools. History
// is extended in place: each assistant turn that calls tools is appended
// together with its results, so a tool_use is never stored without its
// tool_result.
//

pub(super) async fn run_loop(
    ctx: &Arc<SessionCtx>,
    history: &mut Vec<Message>,
    scope: &Scope,
) -> LoopOutcome {
    let mut turns = 0usize;
    loop {
        if ctx.interrupted() {
            return LoopOutcome::Interrupted;
        }
        if scope.max_turns.is_some_and(|max| turns >= max) {
            return LoopOutcome::TurnLimit;
        }
        turns += 1;

        let native = ctx.ai.native.load(Ordering::SeqCst);
        let mut request = ChatCompletionRequest::new(ctx.ai.model.clone(), history.clone())
            .with_max_tokens(ctx.ai.max_tokens);
        if native {
            request = request.with_tools(scope.tools.clone());
        }
        if let Some(reasoning) = &ctx.ai.reasoning {
            request = request.with_reasoning(reasoning.clone());
        }

        let mut stream = ctx.ai.client.chat_completion_events(request);
        let mut display = Display::new(native, scope.label.is_some());
        let mut full_text = String::new();
        let mut completed = None;
        let mut received_any = false;

        loop {
            let next = tokio::select! {
                _ = ctx.interrupted_signal() => return LoopOutcome::Interrupted,
                next = stream.next() => next,
            };
            let Some(event) = next else { break };
            match event {
                Ok(StreamEvent::TextDelta(text)) => {
                    received_any = true;
                    full_text.push_str(&text);
                    display.text(ctx, &text).await;
                }
                Ok(StreamEvent::ReasoningDelta(text)) => {
                    received_any = true;
                    display.thought(ctx, &text).await;
                }
                Ok(StreamEvent::Completed(response)) => {
                    completed = Some(response);
                }
                Err(e) => {
                    let message = e.to_string();
                    if native && !received_any && looks_like_tools_unsupported(&message) {
                        drop(stream);
                        fall_back_to_manual(ctx, history, &scope.manual_system);
                        turns -= 1;
                        break;
                    }
                    let err_msg = format!("AI request failed: {}", message);
                    common::log_error!("{}", err_msg);
                    return LoopOutcome::Failed(err_msg);
                }
            }
        }

        let Some(response) = completed else {
            //
            // Either we just switched to manual mode (retry the turn) or the
            // stream ended without a final message.
            //
            if native && !ctx.ai.native.load(Ordering::SeqCst) {
                continue;
            }
            return LoopOutcome::Failed("AI response stream ended unexpectedly".to_string());
        };

        if let Some(usage) = &response.usage {
            ctx.last_prompt_tokens
                .store(usage.prompt_tokens as u64, Ordering::SeqCst);
            send_usage(ctx, usage).await;
        }

        let (assistant, finish_reason) = response
            .choices
            .into_iter()
            .next()
            .map(|c| (c.message, c.finish_reason))
            .unwrap_or_else(|| (Message::assistant(""), None));

        let calls: Vec<PendingCall> = if native {
            assistant
                .parts()
                .into_iter()
                .filter_map(|p| match p {
                    ContentPart::ToolUse {
                        id, name, input, ..
                    } => Some(PendingCall {
                        model_id: id,
                        display_id: uuid::Uuid::new_v4().to_string(),
                        name,
                        args: input,
                    }),
                    _ => None,
                })
                .collect()
        } else {
            parse_manual_tool_calls(&full_text)
                .0
                .into_iter()
                .map(|(name, args)| {
                    let id = uuid::Uuid::new_v4().to_string();
                    PendingCall {
                        model_id: id.clone(),
                        display_id: id,
                        name,
                        args,
                    }
                })
                .collect()
        };

        display.finish(ctx, &full_text, !calls.is_empty()).await;

        let assistant_message = if native {
            assistant
        } else {
            Message::assistant(&full_text)
        };

        if calls.is_empty() {
            //
            // A final answer with no visible text (a refusal, the output
            // limit hit while reasoning, ...) would otherwise show nothing,
            // and an empty assistant turn is rejected by some providers on
            // the next request. Store and show an explanatory note instead.
            //
            if assistant_message.joined_text().trim().is_empty() {
                let note = empty_response_note(finish_reason.as_deref());
                if scope.label.is_none() {
                    ctx.send(session_update_text(&ctx.session_id, note.clone()))
                        .await;
                }
                history.push(Message::assistant(note));
            } else {
                history.push(assistant_message);
            }
            return LoopOutcome::Completed;
        }

        if ctx.interrupted() {
            return LoopOutcome::Interrupted;
        }

        let Some(results) = run_tool_batch(ctx, &calls, scope.label.as_deref()).await else {
            //
            // Cancelled or stopped mid-batch — do not feed partial tool
            // results back into the model.
            //
            return LoopOutcome::Interrupted;
        };

        history.push(assistant_message);
        history.push(results_message(native, &calls, &results));
    }
}

//
// Announce, execute (concurrently) and report a batch of tool calls.
// Results are returned in call order as (result, is_error).
//

async fn run_tool_batch(
    ctx: &Arc<SessionCtx>,
    calls: &[PendingCall],
    label: Option<&str>,
) -> Option<Vec<(String, bool)>> {
    let display_name = |name: &str| match label {
        Some(label) => format!("{} \u{203a} {}", label, name),
        None => name.to_string(),
    };
    for call in calls {
        common::log_info!(
            "Orchestrator queuing tool {} ({})",
            display_name(&call.name),
            common::short_id(&call.display_id)
        );
        ctx.send(session_update_tool_call(
            &ctx.session_id,
            &call.display_id,
            &display_name(&call.name),
            Some(call.args.clone()),
        ))
        .await;
    }
    let child = label.is_some();

    let batch = execute_tool_calls_concurrent(
        calls
            .iter()
            .map(|c| (c.name.clone(), c.args.clone()))
            .collect(),
        calls.iter().map(|c| c.display_id.clone()).collect(),
        &ctx.cancel,
        &ctx.stop,
        |display_id, name, args| {
            let ctx = Arc::clone(ctx);
            async move { execute_tool(&ctx, &display_id, &name, &args, child).await }
        },
    )
    .await?;

    let mut results = Vec::with_capacity(batch.len());
    for (display_id, name, result, is_error) in batch {
        common::log_info!(
            "Tool {} result: {}",
            name,
            common::truncate_str(&result, 100)
        );
        if name == "report_plan" && !child {
            send_plan(ctx, &result).await;
        }
        ctx.send(session_update_tool_result(
            &ctx.session_id,
            &display_id,
            &result,
            is_error,
        ))
        .await;
        results.push((result, is_error));
    }
    Some(results)
}

async fn execute_tool(
    ctx: &Arc<SessionCtx>,
    display_id: &str,
    name: &str,
    args: &Value,
    child: bool,
) -> (String, bool) {
    if let Some(raw) = args.get(INVALID_TOOL_ARGUMENTS_KEY) {
        let payload = json!({
            "status": "error",
            "message": format!(
                "The arguments for '{}' were not a valid JSON object (possibly truncated). Retry the call with complete, valid JSON arguments.",
                name
            ),
            "raw_arguments": raw,
            "display": "Error: invalid tool arguments"
        });
        return (payload.to_string(), true);
    }

    if ctx.gate.needs_approval(name, args) {
        match ctx
            .gate
            .request(ctx, &ctx.permissions, display_id, name, args)
            .await
        {
            Decision::Allow => {}
            Decision::Deny => {
                common::log_info!("Operator denied orchestrator tool {}", name);
                return (denied_result(name), true);
            }
            Decision::Interrupted => {
                return (
                    json!({"status": "error", "message": "Cancelled", "display": "Cancelled"})
                        .to_string(),
                    true,
                );
            }
        }
    }

    if let Some(result) = memory::execute(&ctx.db, &ctx.engagement, name, args).await {
        return result;
    }

    if name == LOAD_PLAYBOOK_TOOL {
        return load_playbook(&ctx.db, args).await;
    }

    if name == DELEGATE_TOOL {
        if child {
            return (
                json!({
                    "status": "error",
                    "message": "Sub-orchestrators cannot delegate further; do the work directly.",
                    "display": "Error: nested delegation"
                })
                .to_string(),
                true,
            );
        }
        return delegate::run(ctx, args).await;
    }

    if let Some(local_result) = execute_local_tool(name, args).await {
        let err = result_is_error(&local_result);
        return (local_result, err);
    }
    let (result, is_error) = execute_mcp_tool(&ctx.peer, name, args).await;
    ctx.gate.note_result(name, args, &result, is_error);
    (result, is_error)
}

fn results_message(native: bool, calls: &[PendingCall], results: &[(String, bool)]) -> Message {
    if native {
        Message::with_parts(
            Role::User,
            calls
                .iter()
                .zip(results)
                .map(|(call, (result, is_error))| ContentPart::ToolResult {
                    tool_use_id: call.model_id.clone(),
                    name: call.name.clone(),
                    content: result.clone(),
                    is_error: *is_error,
                })
                .collect(),
        )
    } else {
        Message::user(
            calls
                .iter()
                .zip(results)
                .map(|(call, (result, _))| format!("Tool '{}' result:\n{}", call.name, result))
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    }
}

async fn send_plan(ctx: &SessionCtx, result: &str) {
    let Some(plan_obj) = serde_json::from_str::<Value>(result)
        .ok()
        .and_then(|v| v.get("plan").cloned())
    else {
        return;
    };
    if let Ok(plan) = serde_json::from_value::<OrchestratorPlan>(plan_obj) {
        let plan_json = serde_json::to_value(&plan).unwrap_or(Value::Null);
        ctx.send(session_update_plan(&ctx.session_id, &plan_json))
            .await;
    }
}

async fn send_usage(ctx: &SessionCtx, usage: &Usage) {
    ctx.send(session_update_usage(
        &ctx.session_id,
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.total_tokens,
    ))
    .await;
}

fn empty_response_note(finish_reason: Option<&str>) -> String {
    match finish_reason.map(str::to_ascii_lowercase).as_deref() {
        Some("refusal" | "content_filter" | "safety" | "prohibited_content" | "blocklist") => {
            format!(
                "_The model declined to respond (stop reason: {})._",
                finish_reason.unwrap_or_default()
            )
        }
        Some("max_tokens" | "length") => "_The response hit the output token limit before any \
            text was produced. Raise llm_orchestrator_max_tokens or lower the reasoning effort._"
            .to_string(),
        Some(other) => format!("_The model returned no text (stop reason: {})._", other),
        None => "_The model returned no text._".to_string(),
    }
}

//
// Endpoints that cannot do native function calling reject the request with
// an error naming tool support (Ollama: "does not support tools",
// OpenRouter: "No endpoints found that support tool use", vLLM: tool
// parser not enabled).
//

fn looks_like_tools_unsupported(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("tool")
        && (m.contains("support") || m.contains("tool-call-parser") || m.contains("tool choice"))
}

//
// Switch the session to prompt-based tool calling: swap the system prompt
// for the one documenting tools inline and rewrite any structured turns
// into the textual form the manual parser understands.
//

fn fall_back_to_manual(ctx: &SessionCtx, history: &mut [Message], manual_system: &str) {
    common::log_warn!(
        "Orchestrator: model {} rejected native tool calling; falling back to prompt-based tool calls for this session",
        ctx.ai.model
    );
    ctx.ai.native.store(false, Ordering::SeqCst);
    for message in history.iter_mut() {
        *message = if message.role == Role::System {
            Message::system(manual_system)
        } else {
            to_manual_message(message)
        };
    }
}

//
// Render a structured message as plain text: tool calls in the manual
// `{"tool": ..., "args": ...}` form and tool results as "Tool 'x' result".
// Thinking is dropped. Also used to build transcripts for summarization.
//

pub(super) fn to_manual_message(message: &Message) -> Message {
    let Content::Parts(parts) = &message.content else {
        return message.clone();
    };
    let mut chunks: Vec<String> = Vec::new();
    for part in parts {
        match part {
            ContentPart::Text { text } if !text.is_empty() => chunks.push(text.clone()),
            //
            // Built by hand: the manual parser keys on a leading `{"tool":`.
            //
            ContentPart::ToolUse { name, input, .. } => chunks.push(format!(
                "{{\"tool\": {}, \"args\": {}}}",
                Value::String(name.clone()),
                input
            )),
            ContentPart::ToolResult { name, content, .. } => {
                chunks.push(format!("Tool '{}' result:\n{}", name, content))
            }
            _ => {}
        }
    }
    Message::new(message.role, Content::Text(chunks.join("\n\n")))
}

//
// Operator-facing streaming. Native mode forwards text as it arrives (in
// small batches); manual mode runs it through the tool-marker filter.
//

struct Display {
    manual: Option<ManualTextFilter>,
    pending: String,
    pending_thought: String,
    quiet: bool,
}

impl Display {
    fn new(native: bool, quiet: bool) -> Self {
        Self {
            manual: (!native).then(ManualTextFilter::default),
            pending: String::new(),
            pending_thought: String::new(),
            quiet,
        }
    }

    //
    // Reasoning is streamed as thought chunks. Text and thought buffers are
    // flushed before switching between them so ordering is preserved.
    //
    async fn thought(&mut self, ctx: &SessionCtx, delta: &str) {
        if self.quiet {
            return;
        }
        self.flush_text(ctx).await;
        self.pending_thought.push_str(delta);
        if self.pending_thought.len() >= 80 || delta.contains('\n') {
            self.flush_thought(ctx).await;
        }
    }

    async fn flush_thought(&mut self, ctx: &SessionCtx) {
        if !self.pending_thought.is_empty() {
            let chunk = std::mem::take(&mut self.pending_thought);
            ctx.send(session_update_thought(&ctx.session_id, chunk))
                .await;
        }
    }

    async fn flush_text(&mut self, ctx: &SessionCtx) {
        if !self.pending.is_empty() {
            let chunk = std::mem::take(&mut self.pending);
            ctx.send(session_update_text(&ctx.session_id, chunk)).await;
        }
    }

    async fn text(&mut self, ctx: &SessionCtx, delta: &str) {
        if self.quiet {
            return;
        }
        self.flush_thought(ctx).await;
        if let Some(filter) = self.manual.as_mut() {
            for chunk in filter.push(delta) {
                ctx.send(session_update_text(&ctx.session_id, chunk)).await;
            }
            return;
        }
        self.pending.push_str(delta);
        if self.pending.len() >= 50 || delta.contains('\n') {
            self.flush_text(ctx).await;
        }
    }

    async fn finish(&mut self, ctx: &SessionCtx, full_text: &str, has_calls: bool) {
        if self.quiet {
            return;
        }
        self.flush_thought(ctx).await;
        if let Some(filter) = self.manual.as_mut() {
            if let Some(chunk) = filter.finish() {
                ctx.send(session_update_text(&ctx.session_id, chunk)).await;
            }
            if !has_calls && let Some(tail) = filter.unsent_tail(full_text) {
                ctx.send(session_update_text(&ctx.session_id, tail)).await;
            }
            return;
        }
        self.flush_text(ctx).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_responses_get_explanatory_notes() {
        assert!(empty_response_note(Some("refusal")).contains("declined"));
        assert!(empty_response_note(Some("SAFETY")).contains("declined"));
        assert!(empty_response_note(Some("max_tokens")).contains("output token limit"));
        assert!(empty_response_note(Some("end_turn")).contains("no text"));
        assert!(empty_response_note(None).contains("no text"));
    }

    #[test]
    fn detects_tool_unsupported_errors() {
        assert!(looks_like_tools_unsupported(
            "OpenAI API error 400 Bad Request: {\"error\":\"registry.ollama.ai/library/gemma:2b does not support tools\"}"
        ));
        assert!(looks_like_tools_unsupported(
            "OpenAI API error 404: No endpoints found that support tool use."
        ));
        assert!(!looks_like_tools_unsupported(
            "Anthropic API error 529: Overloaded"
        ));
    }

    #[test]
    fn manual_rendering_of_structured_turns() {
        let assistant = Message::with_parts(
            Role::Assistant,
            vec![
                ContentPart::Thinking {
                    thinking: "x".into(),
                    signature: None,
                },
                ContentPart::Text {
                    text: "Checking.".into(),
                },
                ContentPart::ToolUse {
                    id: "t1".into(),
                    name: "node_list".into(),
                    input: json!({}),
                    signature: None,
                },
            ],
        );
        let rendered = to_manual_message(&assistant);
        assert_eq!(
            rendered.text(),
            "Checking.\n\n{\"tool\": \"node_list\", \"args\": {}}"
        );
        let (calls, _) = parse_manual_tool_calls(rendered.text());
        assert_eq!(calls[0].0, "node_list");

        let results = Message::with_parts(
            Role::User,
            vec![ContentPart::ToolResult {
                tool_use_id: "t1".into(),
                name: "node_list".into(),
                content: "[]".into(),
                is_error: false,
            }],
        );
        assert_eq!(
            to_manual_message(&results).text(),
            "Tool 'node_list' result:\n[]"
        );
    }

    #[test]
    fn native_results_pair_with_model_ids() {
        let calls = vec![
            PendingCall {
                model_id: "toolu_a".into(),
                display_id: "d1".into(),
                name: "node_list".into(),
                args: json!({}),
            },
            PendingCall {
                model_id: "toolu_b".into(),
                display_id: "d2".into(),
                name: "op_available".into(),
                args: json!({}),
            },
        ];
        let results = vec![("ok".to_string(), false), ("bad".to_string(), true)];
        let msg = results_message(true, &calls, &results);
        assert_eq!(msg.role, Role::User);
        let parts = msg.parts();
        assert_eq!(
            parts[1],
            ContentPart::ToolResult {
                tool_use_id: "toolu_b".into(),
                name: "op_available".into(),
                content: "bad".into(),
                is_error: true
            }
        );
    }
}
