use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde_json::json;

use common::ai::{ChatCompletionRequest, Content, ContentPart, Message, Role};

use super::engine::{SessionCtx, to_manual_message};
use crate::acp_server::{session_update_tool_call, session_update_tool_result};

//
// Token-budget-aware context compaction. When the conversation grows past
// the configured budget, older turns are summarized by the orchestrator
// model into a single summary message and the most recent turns are kept
// verbatim. Cuts only happen at operator-turn boundaries, so a tool call is
// never separated from its result.
//

//
// Share of the budget kept verbatim after compaction; the rest of the room
// is left for the summary and new work.
//
const KEEP_FRACTION: f64 = 0.4;

//
// Bounds on what is sent to the summarizer: each message is clipped and the
// whole transcript is capped so the summarization request itself fits.
//
const TRANSCRIPT_MESSAGE_CHARS: usize = 4_000;
const SUMMARY_MAX_TOKENS: u32 = 4_096;

const SUMMARY_HEADER: &str = "[Summary of the earlier conversation, compacted to save context]";

const SUMMARIZER_PROMPT: &str = "You compress the history of a conversation between a security \
Operator and the Praxis Orchestrator (an AI that coordinates nodes, agents, sessions, operations, \
chains and triggers through tools). Write a concise but complete summary that lets the \
Orchestrator continue the work without the original messages. Preserve: the Operator's goals and \
standing instructions; decisions made; exact identifiers (node ids, agent short names, session \
ids, operation/chain/trigger/execution ids, file paths); important tool results and findings; \
what is finished and what is still outstanding; anything the Operator asked to remember. Use \
short bullet points grouped under headings. Do not invent details.";

pub(super) enum CompactionOutcome {
    NotNeeded,
    Compacted,
    Interrupted,
}

//
// Rough token estimate (~4 characters per token) over everything that is
// sent to the model, including tool inputs and results.
//

pub(super) fn estimate_tokens(messages: &[Message]) -> usize {
    messages.iter().map(message_chars).sum::<usize>() / 4
}

fn message_chars(message: &Message) -> usize {
    match &message.content {
        Content::Text(t) => t.len(),
        Content::Parts(parts) => parts
            .iter()
            .map(|p| match p {
                ContentPart::Text { text } => text.len(),
                ContentPart::Thinking { thinking, .. } => thinking.len(),
                ContentPart::RedactedThinking { data } => data.len(),
                ContentPart::ToolUse { name, input, .. } => name.len() + input.to_string().len(),
                ContentPart::ToolResult { content, .. } => content.len(),
            })
            .sum(),
    }
}

//
// An operator turn starts at a plain-text user message. Native tool results
// are structured, and manual-mode tool results use the engine's
// "Tool '<name>' result:" text form, so neither counts.
//

fn is_turn_start(message: &Message) -> bool {
    message.role == Role::User
        && matches!(&message.content, Content::Text(t) if !(t.starts_with("Tool '") && t.contains("' result:")))
}

//
// Decide where to cut. Returns the index of the first message to keep
// verbatim (everything in 1..index is summarized), or None when no
// compaction is needed or possible. The kept tail is the longest run of
// whole turns that fits in `keep_tokens`; if even the latest turn is
// larger, everything before the latest turn is summarized.
//

pub(super) fn compaction_split(
    history: &[Message],
    budget_tokens: usize,
    observed_tokens: usize,
) -> Option<usize> {
    let total = estimate_tokens(history).max(observed_tokens);
    if total <= budget_tokens || history.len() < 3 {
        return None;
    }
    let keep_tokens = (budget_tokens as f64 * KEEP_FRACTION) as usize;

    let starts: Vec<usize> = (2..history.len())
        .filter(|&i| is_turn_start(&history[i]))
        .collect();
    let latest = *starts.last()?;

    starts
        .iter()
        .copied()
        .find(|&i| estimate_tokens(&history[i..]) <= keep_tokens)
        .or(Some(latest))
}

//
// Kept turns lose their thinking blocks: providers bind a thinking block to
// the exact preceding conversation, which compaction changes.
//

fn strip_thinking(message: Message) -> Message {
    let Content::Parts(parts) = message.content else {
        return message;
    };
    let parts: Vec<ContentPart> = parts
        .into_iter()
        .filter(|p| {
            !matches!(
                p,
                ContentPart::Thinking { .. } | ContentPart::RedactedThinking { .. }
            )
        })
        .collect();
    if parts.is_empty() {
        return Message::new(
            message.role,
            Content::Text("(reasoning omitted)".to_string()),
        );
    }
    Message::with_parts(message.role, parts)
}

//
// Assemble the compacted history: system prompt, the summary as a user
// message with a short assistant acknowledgement (keeps roles alternating),
// then the kept turns.
//

pub(super) fn rebuild_history(history: &mut Vec<Message>, split: usize, summary: &str) {
    let kept: Vec<Message> = history.drain(split..).map(strip_thinking).collect();
    history.truncate(1);
    history.push(Message::user(format!(
        "{}\n\n{}",
        SUMMARY_HEADER,
        summary.trim()
    )));
    history.push(Message::assistant(
        "Understood. I will continue from this summary.",
    ));
    history.extend(kept);
}

//
// Plain-text transcript of the messages being summarized. Long messages
// are clipped and, if the whole thing is still over `max_chars`, the oldest
// content is dropped first.
//

pub(super) fn render_transcript(messages: &[Message], max_chars: usize) -> String {
    let mut blocks: Vec<String> = messages
        .iter()
        .map(|m| {
            let text = to_manual_message(m).joined_text();
            let label = match m.role {
                Role::User if is_turn_start(m) => "Operator",
                Role::User => "Tool results",
                Role::Assistant => "Orchestrator",
                Role::System => "System",
            };
            format!(
                "### {}\n{}",
                label,
                common::truncate_str(&text, TRANSCRIPT_MESSAGE_CHARS)
            )
        })
        .collect();

    let mut total: usize = blocks.iter().map(|b| b.len() + 2).sum();
    let mut dropped = false;
    while total > max_chars && blocks.len() > 1 {
        total -= blocks.remove(0).len() + 2;
        dropped = true;
    }
    let body = blocks.join("\n\n");
    if dropped {
        format!("[earliest messages omitted]\n\n{}", body)
    } else {
        body
    }
}

//
// Compact `history` in place if it is over the session's budget. Runs at
// the start of an operator turn (so it also applies to resumed sessions on
// their first prompt). The summarization call is cancellable; if it fails
// the older turns are dropped with a note instead.
//

pub(super) async fn maybe_compact(
    ctx: &Arc<SessionCtx>,
    history: &mut Vec<Message>,
) -> CompactionOutcome {
    let observed = ctx.last_prompt_tokens.load(Ordering::SeqCst) as usize;
    let Some(split) = compaction_split(history, ctx.ai.context_budget, observed) else {
        return CompactionOutcome::NotNeeded;
    };

    let compacted_count = split - 1;
    let before_tokens = estimate_tokens(history).max(observed);
    let display_id = uuid::Uuid::new_v4().to_string();
    ctx.send(session_update_tool_call(
        &ctx.session_id,
        &display_id,
        "compact_context",
        Some(json!({
            "messages": compacted_count,
            "estimated_tokens": before_tokens,
            "budget_tokens": ctx.ai.context_budget,
        })),
    ))
    .await;

    let transcript = render_transcript(&history[1..split], ctx.ai.context_budget * 3);
    let request = ChatCompletionRequest::new(
        ctx.ai.model.clone(),
        vec![
            Message::system(SUMMARIZER_PROMPT),
            Message::user(format!(
                "Summarize this conversation history:\n\n{}",
                transcript
            )),
        ],
    )
    .with_max_tokens(SUMMARY_MAX_TOKENS);

    let response = tokio::select! {
        _ = ctx.interrupted_signal() => {
            ctx.send(session_update_tool_result(
                &ctx.session_id,
                &display_id,
                r#"{"status":"error","display":"Cancelled"}"#,
                true,
            ))
            .await;
            return CompactionOutcome::Interrupted;
        }
        r = ctx.ai.client.chat_completion(request) => r,
    };

    let (summary, ok) = match response {
        Ok(resp) => {
            let text = resp
                .choices
                .first()
                .map(|c| c.message.joined_text())
                .unwrap_or_default();
            if text.trim().is_empty() {
                (dropped_note("the summarizer returned no text"), false)
            } else {
                (text, true)
            }
        }
        Err(e) => {
            common::log_warn!("Orchestrator compaction summary failed: {}", e);
            (dropped_note(&e.to_string()), false)
        }
    };

    rebuild_history(history, split, &summary);
    ctx.last_prompt_tokens.store(0, Ordering::SeqCst);

    let after_tokens = estimate_tokens(history);
    common::log_info!(
        "Orchestrator compacted {} messages for {} (~{} -> ~{} tokens, summarized: {})",
        compacted_count,
        common::short_id(&ctx.session_id),
        before_tokens,
        after_tokens,
        ok
    );

    let result = json!({
        "status": if ok { "success" } else { "error" },
        "compacted_messages": compacted_count,
        "estimated_tokens_before": before_tokens,
        "estimated_tokens_after": after_tokens,
        "display": if ok {
            format!("Compacted {} earlier messages", compacted_count)
        } else {
            format!("Dropped {} earlier messages (summary failed)", compacted_count)
        },
    });
    ctx.send(session_update_tool_result(
        &ctx.session_id,
        &display_id,
        &result.to_string(),
        !ok,
    ))
    .await;
    CompactionOutcome::Compacted
}

fn dropped_note(reason: &str) -> String {
    format!(
        "Earlier conversation was removed to fit the context budget; a summary could not be produced ({}). Ask the Operator to restate anything important.",
        common::truncate_str(reason, 200)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_turn(id: &str, result_len: usize) -> Vec<Message> {
        vec![
            Message::with_parts(
                Role::Assistant,
                vec![
                    ContentPart::Thinking {
                        thinking: "t".into(),
                        signature: Some("s".into()),
                    },
                    ContentPart::ToolUse {
                        id: id.into(),
                        name: "node_list".into(),
                        input: json!({}),
                        signature: None,
                    },
                ],
            ),
            Message::with_parts(
                Role::User,
                vec![ContentPart::ToolResult {
                    tool_use_id: id.into(),
                    name: "node_list".into(),
                    content: "x".repeat(result_len),
                    is_error: false,
                }],
            ),
        ]
    }

    //
    // system, then three operator turns each with a tool exchange of
    // ~1000 tokens and a final answer.
    //
    fn sample_history() -> Vec<Message> {
        let mut h = vec![Message::system("sys")];
        for turn in 0..3 {
            h.push(Message::user(format!("prompt {turn}")));
            h.extend(tool_turn(&format!("t{turn}"), 4_000));
            h.push(Message::assistant(format!("answer {turn}")));
        }
        h
    }

    #[test]
    fn no_compaction_under_budget() {
        let h = sample_history();
        assert_eq!(compaction_split(&h, 1_000_000, 0), None);
    }

    #[test]
    fn split_lands_on_turn_start_and_never_between_tool_use_and_result() {
        let h = sample_history();
        let total = estimate_tokens(&h);
        //
        // Budget below total; keep room (40%) fits roughly one turn.
        //
        let split = compaction_split(&h, total * 2 / 3, 0).unwrap();
        assert!(is_turn_start(&h[split]));
        assert!(split > 1);
        //
        // The message before the cut must not be an assistant tool_use
        // awaiting its result.
        //
        let prev = &h[split - 1];
        assert!(
            !prev
                .parts()
                .iter()
                .any(|p| matches!(p, ContentPart::ToolUse { .. }))
        );
    }

    #[test]
    fn oversized_latest_turn_summarizes_everything_before_it() {
        let mut h = sample_history();
        h.push(Message::user("latest prompt"));
        h.extend(tool_turn("big", 200_000));
        let latest = h
            .iter()
            .rposition(|m| matches!(&m.content, Content::Text(t) if t == "latest prompt"))
            .unwrap();
        assert_eq!(compaction_split(&h, 10_000, 0), Some(latest));
    }

    #[test]
    fn observed_tokens_trigger_compaction() {
        let h = sample_history();
        assert!(compaction_split(&h, 50_000, 0).is_none());
        assert!(compaction_split(&h, 50_000, 60_000).is_some());
    }

    #[test]
    fn single_turn_cannot_be_compacted() {
        let mut h = vec![Message::system("sys"), Message::user("only")];
        h.extend(tool_turn("t", 400_000));
        assert_eq!(compaction_split(&h, 1_000, 0), None);
    }

    #[test]
    fn manual_tool_results_are_not_turn_starts() {
        assert!(!is_turn_start(&Message::user(
            "Tool 'node_list' result:\n[]"
        )));
        assert!(is_turn_start(&Message::user("list the nodes")));
        assert!(!is_turn_start(&Message::assistant("hi")));
    }

    #[test]
    fn rebuild_keeps_tail_strips_thinking_and_alternates_roles() {
        let mut h = sample_history();
        let split = 5;
        assert!(is_turn_start(&h[split]));
        let kept_len = h.len() - split;
        rebuild_history(&mut h, split, "the summary");

        assert_eq!(h.len(), 3 + kept_len);
        assert_eq!(h[0].role, Role::System);
        assert_eq!(h[1].role, Role::User);
        assert!(h[1].text().contains("the summary"));
        assert_eq!(h[2].role, Role::Assistant);
        assert_eq!(h[3].role, Role::User);
        for m in &h {
            assert!(
                !m.parts()
                    .iter()
                    .any(|p| matches!(p, ContentPart::Thinking { .. }))
            );
        }
        //
        // Tool use and its result are still adjacent.
        //
        let use_idx = h
            .iter()
            .position(|m| {
                m.parts()
                    .iter()
                    .any(|p| matches!(p, ContentPart::ToolUse { .. }))
            })
            .unwrap();
        assert!(
            h[use_idx + 1]
                .parts()
                .iter()
                .any(|p| matches!(p, ContentPart::ToolResult { .. }))
        );
    }

    #[test]
    fn transcript_clips_and_drops_oldest() {
        let h = sample_history();
        let full = render_transcript(&h[1..], usize::MAX);
        assert!(full.starts_with("### Operator\nprompt 0"));
        assert!(full.contains("### Tool results"));

        let capped = render_transcript(&h[1..], 200);
        assert!(capped.starts_with("[earliest messages omitted]"));
        assert!(capped.contains("answer 2"));
    }
}
