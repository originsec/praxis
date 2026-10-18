use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde_json::{Value, json};

use common::ai::{Message, Role, Tool, get_system_prompt_with_tools};

use super::engine::{LoopOutcome, Scope, SessionCtx, run_loop};

//
// Sub-orchestrators. `delegate` runs a child orchestrator loop on a scoped
// sub-task with its own fresh context, the same tools (minus delegation and
// plan reporting), a bounded number of model turns, and the parent's
// cancel flag. Several delegate calls in one response run concurrently like
// any other tool batch. Only a concise result summary returns to the
// parent, keeping its context small.
//

pub(super) const DELEGATE_TOOL: &str = "delegate";

const DEFAULT_TURNS: usize = 15;
const MAX_TURNS: usize = 30;
const LABEL_MAX_CHARS: usize = 32;
const RESULT_MAX_CHARS: usize = 6_000;

//
// Tools a sub-orchestrator does not get: no recursion, and the plan pane
// belongs to the top-level orchestrator.
//
const CHILD_EXCLUDED_TOOLS: &[&str] = &[DELEGATE_TOOL, "report_plan"];

const SUB_ORCHESTRATOR_PROMPT: &str = "## Sub-orchestrator mode\n\n\
You are running as a sub-orchestrator: the main Orchestrator delegated one scoped sub-task to you. \
You cannot talk to the Operator, ask questions or see the main conversation; work autonomously \
with your tools using only the task description. Stay within the task. When you are done (or \
cannot make further progress), reply without calling tools: a concise summary (at most ~300 \
words) of what you did, the results and exact identifiers found, and anything left undone or \
failed. That reply is returned to the main Orchestrator verbatim.";

pub(super) fn tool_definition() -> Tool {
    Tool::new(DELEGATE_TOOL)
        .with_description(format!(
            "Hand a self-contained sub-task to a sub-orchestrator that runs its own tool loop with a fresh context and returns a concise summary. Use it for independent, tool-heavy work (e.g. investigating one node or agent) to keep your own context small; call it several times in one response to run sub-tasks in parallel. The sub-orchestrator cannot see this conversation, so put everything it needs (node ids, agent short names, goals, constraints) in `task`. It gets up to max_turns model turns (default {}, max {}).",
            DEFAULT_TURNS, MAX_TURNS
        ))
        .with_parameters(json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Complete, self-contained description of the sub-task and the expected result"
                },
                "label": {
                    "type": "string",
                    "description": "Short label shown to the Operator for this sub-task (e.g. 'recon web-01')"
                },
                "max_turns": {
                    "type": "integer",
                    "description": "Maximum model turns for the sub-orchestrator (1-30, default 15)"
                }
            },
            "required": ["task"]
        }))
}

//
// Build the sub-orchestrator scope from the session's tools.
//
fn child_scope(
    session_tools: &[Tool],
    base_prompt: &str,
    label: String,
    max_turns: usize,
) -> Scope {
    let tools: Vec<Tool> = session_tools
        .iter()
        .filter(|t| !CHILD_EXCLUDED_TOOLS.contains(&t.name.as_str()))
        .cloned()
        .collect();
    let manual_system = get_system_prompt_with_tools(&child_prompt(base_prompt), &tools);
    Scope {
        label: Some(label),
        tools,
        manual_system,
        max_turns: Some(max_turns),
    }
}

fn child_prompt(base: &str) -> String {
    format!("{}\n\n{}", base, SUB_ORCHESTRATOR_PROMPT)
}

fn sanitize_label(raw: Option<&str>) -> String {
    let label: String = raw
        .map(|l| l.replace(['\n', '\r'], " "))
        .unwrap_or_default()
        .trim()
        .chars()
        .take(LABEL_MAX_CHARS)
        .collect();
    if label.is_empty() {
        "sub-task".to_string()
    } else {
        label
    }
}

fn turn_budget(args: &Value) -> usize {
    args.get("max_turns")
        .and_then(Value::as_u64)
        .map(|n| (n as usize).clamp(1, MAX_TURNS))
        .unwrap_or(DEFAULT_TURNS)
}

//
// Last visible assistant text in a child transcript (its summary).
//
fn last_assistant_text(history: &[Message]) -> String {
    history
        .iter()
        .rev()
        .filter(|m| m.role == Role::Assistant)
        .map(|m| m.joined_text())
        .find(|t| !t.trim().is_empty())
        .unwrap_or_default()
}

fn error_result(message: String, display: &str) -> (String, bool) {
    (
        json!({"status": "error", "message": message, "display": display}).to_string(),
        true,
    )
}

//
// Run one delegated sub-task. Boxed with an explicit type because it is
// reached recursively from the engine's tool execution.
//
pub(super) fn run<'a>(
    ctx: &'a Arc<SessionCtx>,
    args: &'a Value,
) -> Pin<Box<dyn Future<Output = (String, bool)> + Send + 'a>> {
    Box::pin(async move {
        let Some(task) = args
            .get("task")
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
        else {
            return error_result("task is required".to_string(), "Error: task is required");
        };
        let label = sanitize_label(args.get("label").and_then(Value::as_str));
        let max_turns = turn_budget(args);
        let scope = child_scope(&ctx.tools, &ctx.ai.base_prompt, label.clone(), max_turns);

        let system = if ctx.ai.native.load(Ordering::SeqCst) {
            child_prompt(&ctx.ai.base_prompt)
        } else {
            scope.manual_system.clone()
        };
        let mut history = vec![Message::system(system), Message::user(task)];

        common::log_info!(
            "Orchestrator {} delegating '{}' (max {} turns)",
            common::short_id(&ctx.session_id),
            label,
            max_turns
        );

        let outcome = run_loop(ctx, &mut history, &scope).await;
        let summary = last_assistant_text(&history);
        let summary = common::truncate_str(&summary, RESULT_MAX_CHARS);

        match outcome {
            LoopOutcome::Completed => (
                json!({
                    "status": "success",
                    "label": label,
                    "summary": summary,
                    "display": format!("{}: done", label)
                })
                .to_string(),
                false,
            ),
            LoopOutcome::TurnLimit => (
                json!({
                    "status": "partial",
                    "label": label,
                    "message": format!(
                        "The sub-orchestrator used all {} turns before finishing. Its last report is below; continue the remaining work yourself or delegate a narrower task.",
                        max_turns
                    ),
                    "summary": summary,
                    "display": format!("{}: stopped after {} turns", label, max_turns)
                })
                .to_string(),
                false,
            ),
            LoopOutcome::Interrupted => error_result("Cancelled".to_string(), "Cancelled"),
            LoopOutcome::Failed(e) => {
                error_result(format!("Sub-orchestrator failed: {}", e), "Sub-task failed")
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_scope_drops_delegate_and_plan_tools() {
        let tools = vec![
            Tool::new("node_list"),
            Tool::new(DELEGATE_TOOL),
            Tool::new("report_plan"),
            Tool::new("op_run"),
        ];
        let scope = child_scope(&tools, "BASE", "web".into(), 4);
        let names: Vec<&str> = scope.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["node_list", "op_run"]);
        assert_eq!(scope.max_turns, Some(4));
        assert_eq!(scope.label.as_deref(), Some("web"));
        assert!(scope.manual_system.starts_with("BASE"));
        assert!(scope.manual_system.contains("Sub-orchestrator mode"));
        assert!(!scope.manual_system.contains("### delegate"));
    }

    #[test]
    fn labels_are_sanitized() {
        assert_eq!(sanitize_label(None), "sub-task");
        assert_eq!(sanitize_label(Some("  ")), "sub-task");
        assert_eq!(sanitize_label(Some("recon\nweb-01")), "recon web-01");
        assert_eq!(sanitize_label(Some(&"x".repeat(50))).len(), LABEL_MAX_CHARS);
    }

    #[test]
    fn turn_budget_is_clamped() {
        assert_eq!(turn_budget(&json!({})), DEFAULT_TURNS);
        assert_eq!(turn_budget(&json!({"max_turns": 0})), 1);
        assert_eq!(turn_budget(&json!({"max_turns": 99})), MAX_TURNS);
        assert_eq!(turn_budget(&json!({"max_turns": 5})), 5);
    }

    #[test]
    fn summary_is_last_non_empty_assistant_text() {
        let history = vec![
            Message::system("s"),
            Message::user("task"),
            Message::assistant("first"),
            Message::user("Tool 'x' result:\n{}"),
            Message::assistant("final summary"),
        ];
        assert_eq!(last_assistant_text(&history), "final summary");
        assert_eq!(last_assistant_text(&history[..2]), "");
    }
}
