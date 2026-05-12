use anyhow::{Context, Result, anyhow};
use common::SemanticOperationSpec;
use lapin::Channel;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock as TokioRwLock, oneshot};
use uuid::Uuid;

use common::ai::{
    ChatCompletionRequest, Message, Provider, Tool, create_ai_client, fmt_agent_start,
    fmt_complete, fmt_error, fmt_incoming, fmt_iteration, fmt_outgoing,
    get_system_prompt_with_tools_and_completion, parse_completion_signal, parse_manual_tool_call,
};

//
// Semantic ops agent prompt embedded at build time.
//

const SEMANTIC_OP_AGENT_PROMPT: &str = include_str!("../prompts/semantic_op_agent.prompt");

use crate::acp_node_proxy::AcpNodeProxy;
use crate::config::ServiceConfig;
use crate::database::Database;

//
// Create a session on the target node/connector over ACP. Returns the ACP
// session_id. The connector to select is folded into _meta.praxis.connector
// (ACP session/new handles "select" implicitly).
//

pub async fn create_session(
    node_id: &str,
    agent_short_name: &str,
    yolo_mode: bool,
    working_dir: Option<String>,
    prompt_timeout_secs: Option<u64>,
    channel: &Channel,
    proxy: &Arc<AcpNodeProxy>,
) -> Result<String> {
    common::log_info!(
        "Creating ACP session on node {} (connector: {}, yolo: {}, working_dir: {:?}, prompt_timeout: {:?})",
        common::short_id(node_id),
        agent_short_name,
        yolo_mode,
        working_dir,
        prompt_timeout_secs
    );

    let cwd = working_dir.unwrap_or_else(|| "/".to_string());
    let mut praxis = json!({
        "nodeId": node_id,
        "connector": agent_short_name,
        "yolo": yolo_mode,
        "interactive": false,
    });
    if let Some(secs) = prompt_timeout_secs {
        praxis["promptTimeoutSecs"] = json!(secs);
    }

    let params = json!({
        "cwd": cwd,
        "mcpServers": [],
        "_meta": { "praxis": praxis },
    });

    let result = proxy
        .request(channel, node_id, "session/new", params)
        .await
        .context("session/new failed")?;

    let session_id = result
        .get("sessionId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("session/new response missing sessionId"))?
        .to_string();

    common::log_info!(
        "Created ACP session {} on node {}",
        common::short_id(&session_id),
        common::short_id(node_id)
    );

    Ok(session_id)
}

//
// Close an ACP session on the node. Fire and forget — errors are logged but
// not propagated.
//

pub async fn close_session(
    node_id: &str,
    session_id: &str,
    channel: &Channel,
    proxy: &Arc<AcpNodeProxy>,
) -> Result<()> {
    let params = json!({
        "sessionId": session_id,
        "_meta": { "praxis": { "nodeId": node_id } },
    });

    let _ = proxy
        .request(channel, node_id, "session/close", params)
        .await
        .map_err(|e| {
            common::log_warn!(
                "session/close for {} on {} failed: {}",
                common::short_id(&session_id),
                common::short_id(node_id),
                e
            );
            e
        });

    Ok(())
}

//
// Cancel an in-flight session/prompt on the node. This is an ACP
// notification (no response expected), so we hand-roll the JSON-RPC frame
// and forward it directly via the proxy.
//

pub async fn cancel_session_prompt(
    node_id: &str,
    session_id: &str,
    channel: &Channel,
    proxy: &Arc<AcpNodeProxy>,
) -> Result<()> {
    let client_id = format!("svc_{}", Uuid::new_v4());
    let frame = json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": {
            "sessionId": session_id,
            "_meta": { "praxis": { "nodeId": node_id } },
        },
    });
    let json_rpc = serde_json::to_string(&frame)?;
    proxy
        .forward_to_node(channel, node_id, &client_id, &json_rpc)
        .await
}

//
// Send a session/prompt and collect the streamed reply body. Wraps
// proxy.request_collecting_text and returns (stop_reason, reply_text).
//

async fn send_session_prompt(
    node_id: &str,
    session_id: &str,
    prompt_text: &str,
    channel: &Channel,
    proxy: &Arc<AcpNodeProxy>,
) -> Result<(String, String)> {
    let params = json!({
        "sessionId": session_id,
        "prompt": [{ "type": "text", "text": prompt_text }],
        "_meta": { "praxis": { "nodeId": node_id } },
    });

    let (result, text) = proxy
        .request_collecting_text(channel, node_id, "session/prompt", params)
        .await?;

    let stop_reason = result
        .get("stopReason")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok((stop_reason, text))
}

//
// Execute a one-shot operation. The caller owns the session lifecycle: if
// `existing_session_id` is Some we use it, otherwise we create a short-lived
// session and close it on the way out. Returns (reply_text, result_status).
//

//
// Dispatch to `execute_agent_mode` or `execute_one_shot` based on
// `spec.mode` and normalize both return shapes to
// `(summary, result, semantic_success)`. `semantic_success` is always
// `None` for the one-shot path. Callers can use this to avoid
// duplicating the mode switch.
//

#[allow(clippy::too_many_arguments)]
pub async fn execute_by_mode(
    operation_id: &str,
    node_id: &str,
    agent_short_name: &str,
    spec: &SemanticOperationSpec,
    working_dir: Option<String>,
    prompt_timeout_secs: Option<u64>,
    existing_session_id: Option<String>,
    config: &Arc<TokioRwLock<ServiceConfig>>,
    channel: &Channel,
    proxy: &Arc<AcpNodeProxy>,
    database: Arc<Database>,
    cancel_rx: oneshot::Receiver<()>,
) -> Result<(String, String, Option<bool>)> {
    if spec.mode == "agent" {
        execute_agent_mode(
            operation_id,
            node_id,
            agent_short_name,
            spec,
            working_dir,
            prompt_timeout_secs,
            existing_session_id,
            config,
            channel,
            proxy,
            database,
            cancel_rx,
        )
        .await
    } else {
        execute_one_shot(
            operation_id,
            node_id,
            agent_short_name,
            spec,
            working_dir,
            prompt_timeout_secs,
            existing_session_id,
            channel,
            proxy,
            database,
            cancel_rx,
        )
        .await
        .map(|(summary, result)| (summary, result, None))
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_one_shot(
    operation_id: &str,
    node_id: &str,
    agent_short_name: &str,
    spec: &SemanticOperationSpec,
    working_dir: Option<String>,
    prompt_timeout_secs: Option<u64>,
    existing_session_id: Option<String>,
    channel: &Channel,
    proxy: &Arc<AcpNodeProxy>,
    database: Arc<Database>,
    mut cancel_rx: oneshot::Receiver<()>,
) -> Result<(String, String)> {
    common::log_debug!(
        "[semop {}] START one-shot '{}' | input: {} bytes",
        common::short_id(&operation_id),
        spec.name,
        spec.operation_prompt.len()
    );

    //
    // Establish session.
    //

    let (session_id, owns_session) = match existing_session_id {
        Some(sid) => (sid, false),
        None => {
            let sid = create_session(
                node_id,
                agent_short_name,
                spec.yolo_mode,
                working_dir,
                prompt_timeout_secs,
                channel,
                proxy,
            )
            .await
            .context("Failed to create session for one-shot operation")?;
            (sid, true)
        }
    };

    let _ = database
        .append_output(
            operation_id,
            &fmt_outgoing("Sending prompt to agent", &spec.operation_prompt),
        )
        .await;

    let prompt_preview: String = spec.operation_prompt.chars().take(100).collect();
    common::log_info!(
        "SemanticPromptSent: op={} node={} prompt={}",
        common::short_id(&operation_id),
        common::short_id(node_id),
        prompt_preview
    );

    //
    // Wait for the prompt response with timeout + cancellation. On
    // timeout/cancel we fire session/cancel to the node to abort the
    // in-flight prompt, then close the session if we own it.
    //

    let timeout_duration = Duration::from_secs(spec.timeout);
    let prompt_fut =
        send_session_prompt(node_id, &session_id, &spec.operation_prompt, channel, proxy);

    let outcome: Result<(String, String)> = tokio::select! {
        result = tokio::time::timeout(timeout_duration, prompt_fut) => {
            match result {
                Ok(Ok((_stop_reason, text))) => {
                    let _ = database
                        .append_output(operation_id, &fmt_incoming("Agent response", &text))
                        .await;
                    let preview: String = text.chars().take(100).collect();
                    common::log_info!(
                        "SemanticResponseReceived: op={} len={} response={}",
                        common::short_id(&operation_id),
                        text.len(),
                        preview
                    );
                    Ok((text, "success".to_string()))
                }
                Ok(Err(e)) => {
                    let _ = database
                        .append_output(operation_id, &fmt_error(&format!("Error: {}", e)))
                        .await;
                    common::log_error!(
                        "SemanticResponseError: op={} error={}",
                        common::short_id(&operation_id),
                        e
                    );
                    Err(e)
                }
                Err(_) => {
                    let _ = database
                        .append_output(
                            operation_id,
                            &fmt_error(&format!(
                                "Operation timed out after {} seconds",
                                spec.timeout
                            )),
                        )
                        .await;
                    common::log_error!(
                        "SemanticResponseError: op={} error=timeout",
                        common::short_id(&operation_id)
                    );
                    if let Err(e) = cancel_session_prompt(node_id, &session_id, channel, proxy).await {
                        common::log_error!("Failed to cancel session prompt on timeout: {}", e);
                    }
                    Err(anyhow!(
                        "Operation timed out after {} seconds",
                        spec.timeout
                    ))
                }
            }
        }
        _ = &mut cancel_rx => {
            let _ = database.append_output(operation_id, &fmt_error("Operation cancelled")).await;
            common::log_error!(
                "SemanticResponseError: op={} error=cancelled",
                common::short_id(&operation_id)
            );
            if let Err(e) = cancel_session_prompt(node_id, &session_id, channel, proxy).await {
                common::log_error!("Failed to cancel session prompt: {}", e);
            }
            Err(anyhow::Error::new(crate::semantic_ops::Cancelled))
        }
    };

    if owns_session {
        let _ = close_session(node_id, &session_id, channel, proxy).await;
    }

    let response = outcome?;

    common::log_debug!(
        "[semop {}] END   one-shot '{}' | output: {} bytes",
        common::short_id(&operation_id),
        spec.name,
        response.0.len()
    );

    Ok(response)
}

//
// Execute an operation in agent mode. Uses an LLM-driven loop where the
// model can call a single `session_prompt` tool that dispatches prompts to
// the remote agent session. LLM configuration is pulled from ServiceConfig.
// Returns (summary, result, semantic_success).
//

#[allow(clippy::too_many_arguments)]
pub async fn execute_agent_mode(
    operation_id: &str,
    node_id: &str,
    agent_short_name: &str,
    spec: &SemanticOperationSpec,
    working_dir: Option<String>,
    prompt_timeout_secs: Option<u64>,
    existing_session_id: Option<String>,
    config: &Arc<TokioRwLock<ServiceConfig>>,
    channel: &Channel,
    proxy: &Arc<AcpNodeProxy>,
    database: Arc<Database>,
    mut cancel_rx: oneshot::Receiver<()>,
) -> Result<(String, String, Option<bool>)> {
    common::log_debug!(
        "[semop {}] START agent '{}' | iterations: {} | input: {} bytes",
        common::short_id(&operation_id),
        spec.name,
        spec.agent_iterations,
        spec.operation_prompt.len()
    );

    //
    // Establish session.
    //

    let (session_id, owns_session) = match existing_session_id {
        Some(sid) => (sid, false),
        None => {
            let sid = create_session(
                node_id,
                agent_short_name,
                spec.yolo_mode,
                working_dir.clone(),
                prompt_timeout_secs,
                channel,
                proxy,
            )
            .await
            .context("Failed to create session for agent mode operation")?;
            (sid, true)
        }
    };

    //
    // Reload config from database to ensure fresh values.
    //

    {
        let mut cfg_w = config.write().await;
        let _ = cfg_w.reload().await;
    }

    let (provider_str, model, api_key, base_url) = {
        let cfg = config.read().await;
        let model_def = if let Some(ref mref) = spec.model_ref {
            cfg.find_model_definition(mref).ok_or_else(|| {
                anyhow!(
                    "Model '{}' not found. Configure in Settings > LLM Providers.",
                    mref
                )
            })?
        } else {
            cfg.get_semantic_ops_model_def().ok_or_else(|| {
                anyhow!(
                    "No LLM configured for Semantic Ops. Configure in Settings > LLM Providers."
                )
            })?
        };
        (
            model_def.provider,
            model_def.model,
            model_def.api_key,
            model_def.base_url,
        )
    };

    let provider = Provider::from_str(&provider_str)
        .ok_or_else(|| anyhow!("Unknown provider: {}", provider_str))?;

    let client = create_ai_client(provider, api_key, base_url.as_deref())?;

    //
    // Define the single tool available to the agent.
    //

    let tools = vec![Tool {
        name: "session_prompt".to_string(),
        description: Some(
            "Send a prompt/instruction to the remote agent session and receive their response. Use this to instruct the remote agent to perform tasks, gather information, or execute commands.".to_string(),
        ),
        parameters: Some(json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The prompt/instruction to send to the remote agent"
                }
            },
            "required": ["text"]
        })),
    }];

    let system_prompt =
        get_system_prompt_with_tools_and_completion(SEMANTIC_OP_AGENT_PROMPT, &tools);

    let mut conversation_history: Vec<Message> = vec![
        Message::system(&system_prompt),
        Message::user(format!(
            "START TASK>>>\n\n{}\n\n<<<END TASK",
            spec.operation_prompt
        )),
    ];

    let mut final_summary = String::new();
    let mut final_result = String::new();
    let mut semantic_success: Option<bool> = None;
    let start_time = std::time::Instant::now();
    let timeout_duration = Duration::from_secs(spec.timeout);

    let _ = database
        .append_output(
            operation_id,
            &fmt_agent_start(&provider_str, &model, spec.agent_iterations as usize),
        )
        .await;
    let _ = database
        .append_output(operation_id, &fmt_outgoing("Task", &spec.operation_prompt))
        .await;

    //
    // Helper closure to tear down the session if we own it. Called before
    // each early return from the loop.
    //

    let cleanup_on_exit = |owns: bool, sid: &str| {
        let channel = channel.clone();
        let proxy = proxy.clone();
        let sid = sid.to_string();
        let node_id = node_id.to_string();
        async move {
            if owns {
                let _ = close_session(&node_id, &sid, &channel, &proxy).await;
            }
        }
    };

    for iteration in 1..=spec.agent_iterations {
        if start_time.elapsed() > timeout_duration {
            let _ = database
                .append_output(
                    operation_id,
                    &fmt_error(&format!(
                        "Operation timed out after {} seconds",
                        spec.timeout
                    )),
                )
                .await;
            cleanup_on_exit(owns_session, &session_id).await;
            return Err(anyhow!(
                "Operation timed out after {} seconds",
                spec.timeout
            ));
        }

        if cancel_rx.try_recv().is_ok() {
            let _ = database
                .append_output(operation_id, &fmt_error("Operation cancelled"))
                .await;
            cleanup_on_exit(owns_session, &session_id).await;
            return Err(anyhow::Error::new(crate::semantic_ops::Cancelled));
        }

        common::log_debug!(
            "[semop {}] iteration {}/{}",
            common::short_id(&operation_id),
            iteration,
            spec.agent_iterations
        );
        let _ = database
            .append_output(
                operation_id,
                &fmt_iteration(iteration as usize, spec.agent_iterations as usize),
            )
            .await;

        let request = ChatCompletionRequest::new(model.clone(), conversation_history.clone())
            .with_max_tokens(4096);

        let response = tokio::select! {
            result = client.chat_completion(request) => {
                result.context("Failed to complete AI request")?
            }
            _ = &mut cancel_rx => {
                let _ = database.append_output(operation_id, &fmt_error("Operation cancelled")).await;
                cleanup_on_exit(owns_session, &session_id).await;
                return Err(anyhow::Error::new(crate::semantic_ops::Cancelled));
            }
        };

        let text_content = response.text().unwrap_or_default().to_string();
        common::log_debug!(
            "[semop {}] AI response: {} bytes\n{}",
            common::short_id(&operation_id),
            text_content.len(),
            text_content
        );
        let _ = database
            .append_output(operation_id, &fmt_incoming("AI Response", &text_content))
            .await;

        //
        // Tool calls take precedence over completion signals — if the model
        // emits both, the completion is hallucinated.
        //

        if let Some((tool_name, tool_args, remaining_text)) = parse_manual_tool_call(&text_content)
        {
            if !remaining_text.is_empty() {
                final_summary = remaining_text.clone();
            }
            conversation_history.push(Message::assistant(&text_content));

            let tool_result = if tool_name == "session_prompt" {
                let prompt_text = tool_args.get("text").and_then(|t| t.as_str()).unwrap_or("");
                common::log_debug!(
                    "[semop {}] tool call session_prompt: {} bytes\n{}",
                    common::short_id(&operation_id),
                    prompt_text.len(),
                    prompt_text
                );
                let _ = database
                    .append_output(
                        operation_id,
                        &fmt_outgoing("Tool call: session_prompt", prompt_text),
                    )
                    .await;

                tokio::select! {
                    res = send_remote_prompt(
                        operation_id,
                        node_id,
                        &session_id,
                        prompt_text,
                        channel,
                        proxy,
                        spec.timeout,
                    ) => {
                        match res {
                            Ok(response_text) => {
                                common::log_debug!(
                                    "[semop {}] tool result: {} bytes\n{}",
                                    common::short_id(&operation_id),
                                    response_text.len(),
                                    response_text
                                );
                                let _ = database.append_output(operation_id, &fmt_incoming("Tool result", &response_text)).await;
                                response_text
                            }
                            Err(e) => {
                                let error_msg = format!("Tool error: {}", e);
                                common::log_debug!(
                                    "[semop {}] tool error: {}",
                                    common::short_id(&operation_id),
                                    error_msg
                                );
                                let _ = database.append_output(operation_id, &fmt_error(&error_msg)).await;
                                error_msg
                            }
                        }
                    }
                    _ = &mut cancel_rx => {
                        let _ = database.append_output(operation_id, &fmt_error("Operation cancelled")).await;
                        cleanup_on_exit(owns_session, &session_id).await;
                        return Err(anyhow::Error::new(crate::semantic_ops::Cancelled));
                    }
                }
            } else {
                let error_msg = format!("Unknown tool: {}", tool_name);
                let _ = database
                    .append_output(operation_id, &fmt_error(&error_msg))
                    .await;
                error_msg
            };

            conversation_history.push(Message::user(format!(
                "Tool '{}' result: {}",
                tool_name, tool_result
            )));
            continue;
        }

        if let Some((is_complete, summary, result, _remaining_text, success)) =
            parse_completion_signal(&text_content)
        {
            if is_complete {
                final_summary = if !summary.is_empty() {
                    summary
                } else {
                    text_content.clone()
                };
                final_result = result;
                semantic_success = success;
                common::log_info!(
                    "SemanticOpComplete: op={} result={} summary={}",
                    common::short_id(&operation_id),
                    final_result,
                    final_summary
                );
                let _ = database
                    .append_output(operation_id, &fmt_complete(&final_result, &final_summary))
                    .await;
                conversation_history.push(Message::assistant(&text_content));
                break;
            }
        }

        //
        // No tool call and no completion — log and exit the loop.
        //

        if !text_content.is_empty() {
            final_summary = text_content.clone();
        }
        conversation_history.push(Message::assistant(&text_content));
        break;
    }

    if semantic_success.is_none() {
        semantic_success = Some(false);
        final_result = "failure".to_string();
        if final_summary.is_empty() {
            final_summary =
                "Agent did not complete: iteration limit reached without a result".to_string();
        }
    }

    common::log_debug!(
        "[semop {}] END   agent '{}' | success: {:?} | summary: {} bytes | result: {}",
        common::short_id(&operation_id),
        spec.name,
        semantic_success,
        final_summary.len(),
        final_result
    );

    if owns_session {
        let _ = close_session(node_id, &session_id, channel, proxy).await;
    }

    Ok((final_summary, final_result, semantic_success))
}

//
// Send a prompt to the remote agent session and await the streamed reply
// body. On timeout, fires session/cancel to abort the in-flight prompt but
// leaves the session itself alive (the caller owns its lifecycle).
//

async fn send_remote_prompt(
    operation_id: &str,
    node_id: &str,
    session_id: &str,
    prompt: &str,
    channel: &Channel,
    proxy: &Arc<AcpNodeProxy>,
    timeout_secs: u64,
) -> Result<String> {
    let preview: String = prompt.chars().take(100).collect();
    common::log_info!(
        "SemanticToolPrompt: op={} node={} prompt={}",
        common::short_id(&operation_id),
        common::short_id(node_id),
        preview
    );

    let fut = send_session_prompt(node_id, session_id, prompt, channel, proxy);
    let timeout = Duration::from_secs(timeout_secs);

    match tokio::time::timeout(timeout, fut).await {
        Ok(Ok((_stop_reason, text))) => {
            let response_preview: String = text.chars().take(100).collect();
            common::log_info!(
                "SemanticToolResponse: op={} len={} response={}",
                common::short_id(&operation_id),
                text.len(),
                response_preview
            );
            Ok(text)
        }
        Ok(Err(e)) => {
            common::log_error!(
                "SemanticToolError: op={} error={}",
                common::short_id(&operation_id),
                e
            );
            Err(e)
        }
        Err(_) => {
            common::log_error!(
                "SemanticToolError: op={} error=timeout",
                common::short_id(&operation_id)
            );
            if let Err(e) = cancel_session_prompt(node_id, session_id, channel, proxy).await {
                common::log_error!("Failed to cancel session prompt on timeout: {}", e);
            }
            Err(anyhow!("Prompt timed out after {} seconds", timeout_secs))
        }
    }
}
