use std::sync::atomic::{AtomicBool, Ordering};

use common::ai::Tool;
use common::{PlanStep, PlanStepStatus};
use rmcp::model::{CallToolRequestParams, RawContent};
use serde_json::{Value, json};

pub(super) fn get_local_tool_definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "wait".to_string(),
            description: Some("Wait/sleep for a specified number of seconds before continuing. Use incremental waits: start with 1-2 seconds, check status, then increase if needed.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "seconds": {
                        "type": "integer",
                        "description": "Number of seconds to wait (1-15)"
                    }
                },
                "required": ["seconds"]
            })),
        },
        Tool {
            name: "report_plan".to_string(),
            description: Some("Report/update the current execution plan. Use this to show your plan to the user and update step statuses as you progress.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "steps": {
                        "type": "array",
                        "description": "The list of plan steps",
                        "items": {
                            "type": "object",
                            "properties": {
                                "description": {
                                    "type": "string",
                                    "description": "Description of what this step does"
                                },
                                "status": {
                                    "type": "string",
                                    "enum": ["not_started", "in_progress", "done"],
                                    "description": "Current status of the step"
                                }
                            },
                            "required": ["description", "status"]
                        }
                    },
                    "current_step_description": {
                        "type": "string",
                        "description": "Brief description of what you're currently doing"
                    },
                    "summary": {
                        "type": "string",
                        "description": "Optional summary or notes about the plan"
                    }
                },
                "required": ["steps"]
            })),
        },
    ]
}

pub(super) async fn execute_local_tool(tool_name: &str, tool_input: &Value) -> Option<String> {
    match tool_name {
        "wait" => {
            let seconds = tool_input
                .get("seconds")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if seconds < 1 {
                return Some(json!({"status": "error", "message": "seconds must be at least 1", "display": "Error: seconds >= 1"}).to_string());
            }
            if seconds > 15 {
                return Some(json!({"status": "error", "message": "seconds cannot exceed 15", "display": "Error: seconds <= 15"}).to_string());
            }

            tokio::time::sleep(std::time::Duration::from_secs(seconds as u64)).await;

            Some(
                json!({
                    "status": "success",
                    "message": format!("Waited for {} seconds", seconds),
                    "seconds": seconds,
                    "display": format!("Waited {}s", seconds)
                })
                .to_string(),
            )
        }
        "report_plan" => {
            let steps_value = tool_input.get("steps").cloned().unwrap_or(json!([]));
            let steps: Vec<PlanStep> = serde_json::from_value(steps_value).unwrap_or_default();
            let summary = tool_input
                .get("summary")
                .and_then(|v| v.as_str())
                .map(String::from);
            let current_step_description = tool_input
                .get("current_step_description")
                .and_then(|v| v.as_str())
                .map(String::from);

            let done_count = steps
                .iter()
                .filter(|s| s.status == PlanStepStatus::Done)
                .count();
            let total_count = steps.len();

            let display = if total_count == 0 {
                "Plan cleared".to_string()
            } else {
                format!("Plan updated: {}/{} done", done_count, total_count)
            };

            Some(
                json!({
                    "status": "success",
                    "message": "Plan updated",
                    "display": display,
                    "plan": {
                        "steps": steps,
                        "summary": summary,
                        "current_step_description": current_step_description,
                        "done_count": done_count,
                        "total_count": total_count
                    }
                })
                .to_string(),
            )
        }
        _ => None,
    }
}

pub(super) async fn execute_mcp_tool(
    peer: &rmcp::service::Peer<rmcp::RoleClient>,
    tool_name: &str,
    tool_input: &Value,
) -> (String, bool) {
    let arguments = if let Some(obj) = tool_input.as_object() {
        if obj.is_empty() {
            None
        } else {
            Some(obj.clone())
        }
    } else {
        None
    };

    let mut request = CallToolRequestParams::new(tool_name.to_string());
    request.arguments = arguments;

    match peer.call_tool(request).await {
        Ok(result) => {
            let text = result
                .content
                .iter()
                .find_map(|c| match &c.raw {
                    RawContent::Text(t) => Some(t.text.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "{}".to_string());
            let is_error = result.is_error.unwrap_or(false) || result_is_error(&text);
            (text, is_error)
        }
        Err(e) => {
            let payload = json!({
                "status": "error",
                "message": format!("MCP tool call failed: {}", e),
                "display": format!("Error: {}", e)
            })
            .to_string();
            (payload, true)
        }
    }
}

//
// Inspect a tool-result payload for a `status: "error"` field — this is
// the convention used by both local and MCP-side tool wrappers to signal
// a logical failure even when the transport call itself succeeded.
//

pub(super) fn result_is_error(result: &str) -> bool {
    serde_json::from_str::<Value>(result)
        .ok()
        .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(str::to_string))
        .map(|s| s == "error")
        .unwrap_or(false)
}

pub(super) fn convert_mcp_tools(mcp_tools: Vec<rmcp::model::Tool>) -> Vec<Tool> {
    mcp_tools
        .into_iter()
        .map(|t| {
            let parameters = if t.input_schema.is_empty() {
                None
            } else {
                Some(Value::Object((*t.input_schema).clone()))
            };

            Tool {
                name: t.name.to_string(),
                description: t.description.map(|d| d.to_string()),
                parameters,
            }
        })
        .collect()
}

//
// Run every tool call in `calls` concurrently. Results are returned in the
// same order as `calls` / `tool_ids` (not completion order). Returns `None`
// if cancel/stop is observed before the full batch finishes — in-flight
// tasks are aborted so the session does not sit waiting on every tool.
//
pub(super) async fn execute_tool_calls_concurrent<F, Fut>(
    calls: Vec<(String, Value)>,
    tool_ids: Vec<String>,
    cancel: &AtomicBool,
    stop: &AtomicBool,
    mut execute_one: F,
) -> Option<Vec<(String, String, String, bool)>>
where
    F: FnMut(String, Value) -> Fut,
    Fut: std::future::Future<Output = (String, bool)> + Send + 'static,
{
    debug_assert_eq!(calls.len(), tool_ids.len());

    if cancel.load(Ordering::SeqCst) || stop.load(Ordering::SeqCst) {
        return None;
    }

    let n = calls.len();
    if n == 0 {
        return Some(Vec::new());
    }

    let names: Vec<String> = calls.iter().map(|(n, _)| n.clone()).collect();
    let mut join_set = tokio::task::JoinSet::new();

    for (i, ((name, args), tool_id)) in calls.into_iter().zip(tool_ids.iter()).enumerate() {
        if cancel.load(Ordering::SeqCst) || stop.load(Ordering::SeqCst) {
            join_set.abort_all();
            while join_set.join_next().await.is_some() {}
            return None;
        }
        let tool_id = tool_id.clone();
        let fut = execute_one(name, args);
        join_set.spawn(async move {
            let (result, is_error) = fut.await;
            (i, tool_id, result, is_error)
        });
    }

    let mut slots: Vec<Option<(String, String, bool)>> = (0..n).map(|_| None).collect();

    let cancel_watch = async {
        loop {
            if cancel.load(Ordering::SeqCst) || stop.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    };
    tokio::pin!(cancel_watch);

    loop {
        tokio::select! {
            _ = &mut cancel_watch => {
                join_set.abort_all();
                while join_set.join_next().await.is_some() {}
                return None;
            }
            joined = join_set.join_next() => {
                match joined {
                    Some(Ok((i, tool_id, result, is_error))) => {
                        slots[i] = Some((tool_id, result, is_error));
                    }
                    Some(Err(_)) => {
                        join_set.abort_all();
                        while join_set.join_next().await.is_some() {}
                        return None;
                    }
                    None => break,
                }
            }
        }
    }

    let mut ordered = Vec::with_capacity(n);
    for (name, slot) in names.into_iter().zip(slots) {
        let (tool_id, result, is_error) = slot?;
        ordered.push((tool_id, name, result, is_error));
    }
    Some(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn concurrent_tools_overlap_in_time() {
        let cancel = AtomicBool::new(false);
        let stop = AtomicBool::new(false);
        let calls = vec![
            ("wait_a".to_string(), json!({})),
            ("wait_b".to_string(), json!({})),
        ];
        let ids = vec!["id-a".to_string(), "id-b".to_string()];

        let start = Instant::now();
        let results =
            execute_tool_calls_concurrent(calls, ids, &cancel, &stop, |name, _| async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                (format!("ok-{name}"), false)
            })
            .await
            .expect("batch should complete");

        let elapsed = start.elapsed();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "id-a");
        assert_eq!(results[0].1, "wait_a");
        assert_eq!(results[0].2, "ok-wait_a");
        assert_eq!(results[1].0, "id-b");
        assert_eq!(results[1].1, "wait_b");
        assert_eq!(results[1].2, "ok-wait_b");
        //
        // Sequential would be ~400ms; concurrent should finish near 200ms.
        // Allow headroom for scheduler noise but stay clearly under sum.
        //
        assert!(
            elapsed < Duration::from_millis(350),
            "expected concurrent overlap, elapsed={elapsed:?}"
        );
    }

    #[tokio::test]
    async fn concurrent_tools_preserve_input_order() {
        let cancel = AtomicBool::new(false);
        let stop = AtomicBool::new(false);
        let calls = vec![
            ("slow".to_string(), json!({})),
            ("fast".to_string(), json!({})),
        ];
        let ids = vec!["id-slow".to_string(), "id-fast".to_string()];

        let results =
            execute_tool_calls_concurrent(calls, ids, &cancel, &stop, |name, _| async move {
                if name == "slow" {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                } else {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                (format!("result-{name}"), false)
            })
            .await
            .expect("batch should complete");

        assert_eq!(results[0].1, "slow");
        assert_eq!(results[1].1, "fast");
        assert_eq!(results[0].0, "id-slow");
        assert_eq!(results[1].0, "id-fast");
    }

    #[tokio::test]
    async fn concurrent_tools_cancel_aborts_batch() {
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = AtomicBool::new(false);
        let cancel_flag = Arc::clone(&cancel);

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_flag.store(true, Ordering::SeqCst);
        });

        let calls = vec![
            ("long_a".to_string(), json!({})),
            ("long_b".to_string(), json!({})),
        ];
        let ids = vec!["id-a".to_string(), "id-b".to_string()];

        let start = Instant::now();
        let results =
            execute_tool_calls_concurrent(calls, ids, &cancel, &stop, |_name, _| async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                ("done".to_string(), false)
            })
            .await;

        assert!(results.is_none(), "cancel should abort the batch");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "cancel should not wait for full tool duration"
        );
    }

    #[tokio::test]
    async fn concurrent_same_name_tools_keep_distinct_ids() {
        let cancel = AtomicBool::new(false);
        let stop = AtomicBool::new(false);
        let calls = vec![
            ("node_list".to_string(), json!({"a": 1})),
            ("node_list".to_string(), json!({"a": 2})),
        ];
        let ids = vec!["uuid-1".to_string(), "uuid-2".to_string()];

        let results =
            execute_tool_calls_concurrent(calls, ids, &cancel, &stop, |name, args| async move {
                let tag = args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                (format!("{name}-{tag}"), false)
            })
            .await
            .expect("batch should complete");

        assert_eq!(results[0].0, "uuid-1");
        assert_eq!(results[1].0, "uuid-2");
        assert_eq!(results[0].2, "node_list-1");
        assert_eq!(results[1].2, "node_list-2");
    }
}
