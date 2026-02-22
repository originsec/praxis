use anyhow::{anyhow, Result};
use common::ai::{build_message, create_ai_client, execute_chat_completion, Provider, Role};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::UnboundedSender;

use crate::config::ServiceConfig;

const UNCHANGED_MARKER: &str = "UNCHANGED";
const SYSTEM_PROMPT_TEMPLATE: &str = include_str!("session_poisoning.prompt");

pub async fn run_transform_per_message(
    model_ref: &str,
    session_content: &str,
    max_tokens: u32,
    service_config: &Arc<RwLock<ServiceConfig>>,
    progress_tx: Option<&UnboundedSender<(usize, usize)>>,
) -> Result<String> {
    let model_def = {
        let cfg = service_config.read().await;
        cfg.find_model_definition(model_ref)
            .ok_or_else(|| anyhow!("Model '{}' not found. Configure in Settings > LLM Providers.", model_ref))?
    };

    let provider = Provider::from_str(&model_def.provider)
        .ok_or_else(|| anyhow!("Unsupported provider '{}'", model_def.provider))?;
    let client = create_ai_client(provider, model_def.api_key.clone())?;

    let (mut messages, format) = parse_session_messages(session_content)?;

    if messages.is_empty() {
        return Ok(session_content.to_string());
    }

    let per_call_tokens = std::cmp::max(4096, max_tokens / messages.len() as u32);
    let total = messages.len();

    let mut last_user_text: Option<String> = None;

    for i in 0..messages.len() {
        let user_prompt = messages[i].text_content.clone();

        //
        // Track the most recent user-looking message for context. We check
        // the original JSON for a "role" field but don't require it.
        //

        let is_user = messages[i]
            .original_value
            .as_ref()
            .and_then(|v| v.get("role"))
            .and_then(|r| r.as_str())
            .map(|r| r == "user")
            .unwrap_or(false);

        if is_user {
            last_user_text = Some(user_prompt.clone());
        }

        let context_line = match &last_user_text {
            Some(t) if !is_user => {
                let preview = &t[..t.len().min(2000)];
                format!("\nThe user's preceding message was:\n\"\"\"\n{}\n\"\"\"\n", preview)
            }
            _ => String::new(),
        };

        let system_prompt = SYSTEM_PROMPT_TEMPLATE.replace("{context}", &context_line);

        if user_prompt.is_empty() {
            if let Some(tx) = progress_tx {
                let _ = tx.send((i + 1, total));
            }
            continue;
        }

        let llm_messages = vec![
            build_message(Role::System, system_prompt.to_string()),
            build_message(Role::User, user_prompt),
        ];

        let response = execute_chat_completion(
            &client,
            model_def.model.clone(),
            llm_messages,
            Some(per_call_tokens),
        )
        .await?;

        let trimmed = response.trim();

        let input_preview = &messages[i].text_content[..messages[i].text_content.len().min(200)];
        common::log_debug!(
            "[session_poisoning] msg {}/{} unchanged={}\n  INPUT:  {:?}\n  OUTPUT: {:?}",
            i + 1,
            total,
            trimmed == UNCHANGED_MARKER,
            input_preview,
            &trimmed[..trimmed.len().min(200)],
        );

        if trimmed != UNCHANGED_MARKER {
            messages[i].text_content = trimmed.to_string();
            messages[i].modified = true;
        }

        if let Some(tx) = progress_tx {
            let _ = tx.send((i + 1, total));
        }
    }

    Ok(reassemble_session(&messages, session_content, &format))
}

//
// For each line in the LLM output, if the only difference from the
// corresponding original line is whitespace, substitute the original line
// back. This suppresses phantom diffs caused by the LLM normalising
// trailing spaces, line endings, or indentation.
//

pub fn strip_whitespace_only_changes(original: &str, transformed: &str) -> String {
    let orig_lines: Vec<&str> = original.lines().collect();
    let trans_lines: Vec<&str> = transformed.lines().collect();

    let mut out = String::with_capacity(transformed.len());
    for (i, tl) in trans_lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if let Some(ol) = orig_lines.get(i) {
            let ol_collapsed: String = ol.split_whitespace().collect();
            let tl_collapsed: String = tl.split_whitespace().collect();
            if ol_collapsed == tl_collapsed {
                out.push_str(ol);
                continue;
            }
        }
        out.push_str(tl);
    }

    if original.ends_with('\n') && orig_lines.len() == trans_lines.len() && !out.ends_with('\n') {
        out.push('\n');
    }

    out
}

//
// Parsed session message — preserves the original JSON value so we can
// swap only the text content and reassemble without disturbing other fields.
//

struct SessionMessage {
    text_content: String,
    modified: bool,
    original_value: Option<Value>,
    original_line: Option<String>,
}

enum SessionFormat {
    JsonlLines,
    JsonArray,
    PlainText,
}

//
// Try to parse the session as JSONL first (one JSON object per line),
// then as a JSON array. Falls back to treating it as plain text.
//

fn parse_session_messages(content: &str) -> Result<(Vec<SessionMessage>, SessionFormat)> {
    let lines: Vec<&str> = content.lines().collect();
    let mut jsonl_messages = Vec::new();
    let mut jsonl_ok = true;

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(val) => {
                jsonl_messages.push((val, line.to_string()));
            }
            Err(_) => {
                jsonl_ok = false;
                break;
            }
        }
    }

    if jsonl_ok && !jsonl_messages.is_empty() {
        let msgs = jsonl_messages
            .into_iter()
            .map(|(val, raw_line)| value_to_session_message(val, Some(raw_line)))
            .collect();
        return Ok((msgs, SessionFormat::JsonlLines));
    }

    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(content) {
        if !arr.is_empty() {
            let msgs = arr
                .into_iter()
                .map(|val| value_to_session_message(val, None))
                .collect();
            return Ok((msgs, SessionFormat::JsonArray));
        }
    }

    //
    // Fallback: plain text — single message, the LLM evaluates it.
    //

    Ok((
        vec![SessionMessage {
            text_content: content.to_string(),
            modified: false,
            original_value: None,
            original_line: Some(content.to_string()),
        }],
        SessionFormat::PlainText,
    ))
}

fn value_to_session_message(val: Value, raw_line: Option<String>) -> SessionMessage {
    let text_content = extract_text_content(&val);
    SessionMessage {
        text_content,
        modified: false,
        original_value: Some(val),
        original_line: raw_line,
    }
}

fn extract_text_content(val: &Value) -> String {
    if let Some(content) = val.get("content") {
        if let Some(s) = content.as_str() {
            return s.to_string();
        }
        if let Some(arr) = content.as_array() {
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                .map(|s| s.to_string())
                .collect();
            if !parts.is_empty() {
                return parts.join("\n");
            }
        }
    }
    if let Some(s) = val.get("text").and_then(|t| t.as_str()) {
        return s.to_string();
    }

    //
    // Last resort: serialize the whole value as the text content so the
    // LLM can still evaluate it.
    //

    serde_json::to_string(val).unwrap_or_default()
}

//
// Reassemble the session content from processed messages. Only modified
// messages get their text_content written back into the JSON; unmodified
// messages use their original verbatim representation.
//

fn reassemble_session(
    messages: &[SessionMessage],
    original_content: &str,
    format: &SessionFormat,
) -> String {
    match format {
        SessionFormat::PlainText => {
            if let Some(msg) = messages.first() {
                if msg.modified {
                    msg.text_content.clone()
                } else {
                    original_content.to_string()
                }
            } else {
                original_content.to_string()
            }
        }

        SessionFormat::JsonlLines => {
            let mut out_lines: Vec<String> = Vec::new();
            for msg in messages {
                if !msg.modified {
                    if let Some(ref line) = msg.original_line {
                        out_lines.push(line.clone());
                        continue;
                    }
                }
                if let Some(ref val) = msg.original_value {
                    let updated = set_text_content(val, &msg.text_content);
                    out_lines.push(serde_json::to_string(&updated).unwrap_or_default());
                }
            }
            let mut result = out_lines.join("\n");
            if original_content.ends_with('\n') {
                result.push('\n');
            }
            result
        }

        SessionFormat::JsonArray => {
            let arr: Vec<Value> = messages
                .iter()
                .map(|msg| {
                    if let Some(ref val) = msg.original_value {
                        if msg.modified {
                            set_text_content(val, &msg.text_content)
                        } else {
                            val.clone()
                        }
                    } else {
                        Value::Null
                    }
                })
                .collect();
            serde_json::to_string_pretty(&arr).unwrap_or_else(|_| original_content.to_string())
        }
    }
}

//
// Replace the text content in a JSON message value, preserving all other
// fields. Handles "content": "string" and "content": [{"text": "..."}].
//

fn set_text_content(val: &Value, new_text: &str) -> Value {
    let mut clone = val.clone();
    if let Some(obj) = clone.as_object_mut() {
        if let Some(content) = obj.get_mut("content") {
            if content.is_string() {
                *content = Value::String(new_text.to_string());
                return clone;
            }
            if let Some(arr) = content.as_array_mut() {
                if let Some(first) = arr.first_mut() {
                    if first.get("text").is_some() {
                        first.as_object_mut().map(|o| {
                            o.insert("text".to_string(), Value::String(new_text.to_string()));
                        });
                        return clone;
                    }
                }
            }
        }
        if obj.contains_key("text") {
            obj.insert("text".to_string(), Value::String(new_text.to_string()));
            return clone;
        }

        obj.insert("content".to_string(), Value::String(new_text.to_string()));
    }
    clone
}
