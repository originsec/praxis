use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow};
use common::ai::{AiClient, ChatCompletionRequest, Message, Role, Tool, ToolCall, Usage};
use futures_util::StreamExt;

pub const CONTEXT_TOKENS: usize = 24_000;

pub fn legacy_history(history: &[Message]) -> Vec<Message> {
    history.iter().map(|message| {
        if message.role == Role::Tool {
            return Message::user(format!("Tool {} ({}) result:\n{}",message.tool_name.as_deref().unwrap_or("unknown"),message.tool_call_id.as_deref().unwrap_or("unknown"),message.text()));
        }
        let mut text=message.text().to_string();
        if !message.tool_calls.is_empty() && common::ai::parse_manual_tool_calls(&text).0.is_empty() {
            for call in &message.tool_calls {text.push_str(&format!("\n{}",serde_json::json!({"tool":call.name,"args":call.arguments})));}
        }
        Message::new(message.role,text)
    }).collect()
}

pub fn estimated_tokens(messages: &[Message]) -> usize {
    messages.iter().map(|m| serde_json::to_vec(m).map(|s| s.len().div_ceil(3) + 8).unwrap_or(0)).sum()
}

//
// Keep the objective and complete recent user turns. Older material becomes
// bounded extracts, never a fabricated summary. Durable missions separately
// pin their structured checkpoint and retain the original events in storage.
//

pub fn compact_history(history: &mut Vec<Message>, limit: usize) {
    if estimated_tokens(history) <= limit || history.len() < 4 { return; }
    let mut prefix = 1;
    if history.get(1).is_some_and(|m| m.role == Role::User) { prefix = 2; }
    if history.get(prefix).is_some_and(|m| m.text().starts_with("Earlier context extracts:")) {
        history.remove(prefix);
    }
    let mut cut = prefix;
    let recent_budget = limit.saturating_sub(estimated_tokens(&history[..prefix]) + 2200);
    for index in prefix..history.len() {
        if history[index].role == Role::User && estimated_tokens(&history[index..]) <= recent_budget {
            cut = index;
            break;
        }
    }
    if cut == prefix {
        //
        // Long autonomous turns have no new user boundary. Cut only before
        // an assistant message so tool requests and results stay together.
        //
        for index in prefix..history.len() {
            if history[index].role == Role::Assistant && estimated_tokens(&history[index..]) <= recent_budget {
                cut = index;
                break;
            }
        }
    }
    if cut > prefix {
        let extracts = history[prefix..cut].iter().rev().take(16).rev().map(|m| {
            format!("{}: {}",m.role.as_str(),common::truncate_str(m.text(),300))
        }).collect::<Vec<_>>().join("\n");
        history.splice(prefix..cut,[Message::user(format!("Earlier context extracts: (incomplete; consult original evidence when needed)\n{extracts}"))]);
    }
    let result_count=history.iter().filter(|m|m.role==Role::Tool).count().max(1);
    let fixed_tokens=history.iter().filter(|m|m.role!=Role::Tool).map(|m|estimated_tokens(std::slice::from_ref(m))).sum::<usize>();
    let result_bytes=limit.saturating_sub(fixed_tokens+result_count*160).saturating_mul(3)/result_count;
    let result_bytes=result_bytes.min(12_000);
    for message in history.iter_mut().filter(|m| m.role == Role::Tool) {
        if message.text().len() > result_bytes {
            message.content = common::ai::Content::Text(format!("{}\n[Result truncated in model context; consult original evidence.]",common::truncate_str(message.text(),result_bytes)));
        }
    }
}

pub async fn cancelled(cancel: &AtomicBool, stop: &AtomicBool) {
    loop {
        if cancel.load(Ordering::SeqCst) || stop.load(Ordering::SeqCst) { return; }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub async fn infer(client: &AiClient, request: ChatCompletionRequest, cancel: &AtomicBool, stop: &AtomicBool) -> Result<(Message,Usage)> {
    let estimated_prompt = estimated_tokens(&request.messages) + serde_json::to_vec(&request.tools)?.len().div_ceil(3);
    let mut stream = client.chat_completion_stream(request);
    let mut message = Message::assistant("");
    let mut text = String::new();
    let mut usage = None;
    let mut complete = false;
    loop {
        let delta = tokio::select! {
            biased;
            _ = cancelled(cancel,stop) => return Err(anyhow!("Cancelled")),
            delta = stream.next() => delta,
        };
        let Some(delta) = delta else { break; };
        let delta = delta?;
        text.push_str(&delta.content);
        message.tool_calls.extend(delta.tool_calls);
        if delta.provider_data.is_some() { message.provider_data = delta.provider_data; }
        if delta.usage.is_some() { usage = delta.usage; }
        if let Some(reason) = delta.finish_reason {
            if matches!(reason.as_str(),"length"|"max_tokens"|"MAX_TOKENS") { return Err(anyhow!("Model response was truncated; no tools executed")); }
            complete = true;
        }
    }
    if !complete { return Err(anyhow!("Model stream ended without a completion marker; no tools executed")); }
    let completion = text.len().div_ceil(3) + serde_json::to_vec(&message.tool_calls)?.len().div_ceil(3);
    message.content = common::ai::Content::Text(text);
    let usage = usage.unwrap_or(Usage { prompt_tokens:estimated_prompt as u32, completion_tokens:completion as u32, total_tokens:(estimated_prompt+completion) as u32 });
    Ok((message,usage))
}

pub fn validate_call(call: &ToolCall, tools: &[Tool]) -> Result<()> {
    let tool = tools.iter().find(|t| t.name == call.name).ok_or_else(|| anyhow!("Unknown tool: {}",call.name))?;
    let args = call.arguments.as_object().ok_or_else(|| anyhow!("Tool arguments must be an object"))?;
    if let Some(required) = tool.parameters.as_ref().and_then(|s| s.get("required")).and_then(|v| v.as_array()) {
        for key in required.iter().filter_map(|v| v.as_str()) {
            if !args.contains_key(key) { return Err(anyhow!("Missing tool argument: {key}")); }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_keeps_objective_and_tool_pairs() {
        let mut history = vec![Message::system("system"),Message::user("pinned objective")];
        for n in 0..40 {
            let call = ToolCall {id:n.to_string(),name:"read".into(),arguments:serde_json::json!({})};
            history.push(Message::assistant_with_tools("",vec![call.clone()]));
            history.push(Message::tool_result(&call,"x".repeat(1000)));
        }
        compact_history(&mut history,5000);
        assert!(estimated_tokens(&history) < 5000);
        assert_eq!(history[1].text(),"pinned objective");
        for (index,message) in history.iter().enumerate().filter(|(_,m)| m.role==Role::Tool) {
            assert!(history[..index].iter().any(|m| m.tool_calls.iter().any(|c| Some(&c.id)==message.tool_call_id.as_ref())));
        }
    }

    #[test]
    fn compaction_bounds_a_single_large_batch_without_losing_tool_pairs() {
        let calls = (0..32).map(|index| ToolCall {
            id: format!("call-{index}"), name: "read".into(),
            arguments: serde_json::json!({"path":format!("fixture-{index}")}),
        }).collect::<Vec<_>>();
        let mut history = vec![Message::system("system"), Message::user("pinned objective")];
        history.push(Message::assistant_with_tools("Inspect these fixtures", calls.clone()));
        for call in &calls {
            history.push(Message::tool_result(call, "λ".repeat(30_000)));
        }
        assert!(estimated_tokens(&history) > CONTEXT_TOKENS * 10);
        compact_history(&mut history, CONTEXT_TOKENS);
        assert!(estimated_tokens(&history) <= CONTEXT_TOKENS);
        assert_eq!(history[1].text(), "pinned objective");
        assert_eq!(history[2].tool_calls.len(), calls.len());
        let results = history.iter().filter(|message| message.role == Role::Tool).collect::<Vec<_>>();
        assert_eq!(results.len(), calls.len());
        for (call, result) in calls.iter().zip(results) {
            assert_eq!(result.tool_call_id.as_deref(), Some(call.id.as_str()));
            assert!(result.text().contains("Result truncated"));
        }
    }

    #[tokio::test]
    async fn cancellation_does_not_wait_for_a_model_chunk() {
        let cancel = AtomicBool::new(false);
        let stop = AtomicBool::new(false);
        let work = async { tokio::select! { _ = cancelled(&cancel,&stop) => (), _ = std::future::pending::<()>() => unreachable!() } };
        let signal = async { tokio::time::sleep(Duration::from_millis(10)).await; cancel.store(true,Ordering::SeqCst); };
        tokio::time::timeout(Duration::from_millis(250),async { tokio::join!(work,signal); }).await.unwrap();
    }
}
