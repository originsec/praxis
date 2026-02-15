use anyhow::{Context, Result};
use common::{
    publish_json, node_queue_name, AgentCommand, CommandRequest, CommandResponse, NodeCommand,
    NodeDirectMessage, SemanticOperationSpec, SessionCommand, SessionContext,
};
use lapin::Channel;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock as StdRwLock};
use std::time::Duration;
use tokio::sync::{oneshot, RwLock as TokioRwLock};
use tokio::time::timeout;
use uuid::Uuid;

use common::ai::{
    Provider, create_ai_client, parse_manual_tool_call, parse_completion_signal,
    get_system_prompt_with_tools_and_completion, fmt_outgoing, fmt_incoming, fmt_error,
    fmt_iteration, fmt_agent_start, fmt_complete,
    ChatCompletionRequest, Message, Tool,
};

//
// Semantic ops agent prompt embedded at build time.
//
const SEMANTIC_OP_AGENT_PROMPT: &str = include_str!("../prompts/semantic_op_agent.prompt");

use crate::config::ServiceConfig;
use crate::database::Database;

/// Response tracker for waiting for command responses
pub struct ResponseTracker {
    pending: Arc<StdRwLock<HashMap<String, oneshot::Sender<CommandResponse>>>>,
}

impl ResponseTracker {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(StdRwLock::new(HashMap::new())),
        }
    }

    /// Register a pending command and return a receiver for the response
    pub fn register(&self, command_id: String) -> oneshot::Receiver<CommandResponse> {
        let (tx, rx) = oneshot::channel();
        self.pending.write().unwrap().insert(command_id, tx);
        rx
    }

    /// Complete a pending command with its response
    pub fn complete(&self, command_id: &str, response: CommandResponse) {
        if let Some(tx) = self.pending.write().unwrap().remove(command_id) {
            let _ = tx.send(response);
        }
    }
}

/// Select an agent on a node
pub async fn select_agent(
    node_id: &str,
    agent_short_name: &str,
    rabbitmq_channel: &Channel,
    response_tracker: Arc<ResponseTracker>,
) -> Result<()> {
    let service_id = "service";
    let cmd_id = Uuid::new_v4().to_string();
    let rx = response_tracker.register(cmd_id.clone());

    common::log_info!(
        "Selecting agent '{}' on node {} (cmd_id: {})",
        agent_short_name, &node_id[..8], &cmd_id[..8]
    );

    let request = CommandRequest {
        command_id: cmd_id.clone(),
        client_id: service_id.to_string(),
        node_id: node_id.to_string(),
        command: NodeCommand::Agent(AgentCommand::Select {
            short_name: agent_short_name.to_string(),
        }),
    };

    let message = NodeDirectMessage::Command(request);
    publish_json(rabbitmq_channel, &node_queue_name(node_id), &message).await?;

    common::log_info!("Sent agent select command, waiting for response...");

    //
    // Wait for response with timeout (30 seconds).
    //
    let timeout_duration = Duration::from_secs(30);
    match timeout(timeout_duration, rx).await {
        Ok(Ok(cmd_response)) => match cmd_response.result {
            common::NodeCommandResult::Agent(common::AgentCommandResult::Selected { .. }) => {
                common::log_info!("Agent '{}' selected successfully", agent_short_name);
                Ok(())
            }
            common::NodeCommandResult::Error { message } => {
                Err(anyhow::anyhow!("Agent select failed: {}", message))
            }
            _ => Err(anyhow::anyhow!("Unexpected response type for agent select")),
        },
        Ok(Err(_)) => Err(anyhow::anyhow!("Response channel closed")),
        Err(_) => Err(anyhow::anyhow!("Agent select timed out after 30s")),
    }
}

/// Create a session on the currently selected agent
pub async fn create_session(
    node_id: &str,
    yolo_mode: bool,
    working_dir: Option<String>,
    rabbitmq_channel: &Channel,
    response_tracker: Arc<ResponseTracker>,
) -> Result<String> {
    let service_id = "service";
    let cmd_id = Uuid::new_v4().to_string();
    let rx = response_tracker.register(cmd_id.clone());

    common::log_info!(
        "Creating session on node {} (yolo_mode: {}, working_dir: {:?}, cmd_id: {})",
        &node_id[..8], yolo_mode, working_dir, &cmd_id[..8]
    );

    let context = SessionContext {
        working_dir,
        yolo_mode,
    };

    let request = CommandRequest {
        command_id: cmd_id.clone(),
        client_id: service_id.to_string(),
        node_id: node_id.to_string(),
        command: NodeCommand::Session(SessionCommand::Create { context }),
    };

    let message = NodeDirectMessage::Command(request);
    publish_json(rabbitmq_channel, &node_queue_name(node_id), &message).await?;

    common::log_info!("Sent session create command, waiting for response...");

    //
    // Wait for response with timeout (60 seconds - session creation can take a
    // while).
    //
    let timeout_duration = Duration::from_secs(60);
    match timeout(timeout_duration, rx).await {
        Ok(Ok(cmd_response)) => match cmd_response.result {
            common::NodeCommandResult::Session(common::SessionCommandResult::Created {
                session_id,
            }) => {
                common::log_info!("Session created successfully: {}", &session_id[..8]);
                Ok(session_id)
            }
            common::NodeCommandResult::Error { message } => {
                Err(anyhow::anyhow!("Session create failed: {}", message))
            }
            _ => Err(anyhow::anyhow!(
                "Unexpected response type for session create"
            )),
        },
        Ok(Err(_)) => Err(anyhow::anyhow!("Response channel closed")),
        Err(_) => Err(anyhow::anyhow!("Session create timed out after 60s")),
    }
}

/// Cancel a running transaction on the node (fire and forget)
pub async fn cancel_transaction(
    node_id: &str,
    transaction_id: &str,
    rabbitmq_channel: &Channel,
) -> Result<()> {
    let service_id = "service";
    let cmd_id = Uuid::new_v4().to_string();

    let request = CommandRequest {
        command_id: cmd_id,
        client_id: service_id.to_string(),
        node_id: node_id.to_string(),
        command: NodeCommand::Session(SessionCommand::CancelTransaction {
            transaction_id: transaction_id.to_string(),
            force: true,
        }),
    };

    let message = NodeDirectMessage::Command(request);
    publish_json(rabbitmq_channel, &node_queue_name(node_id), &message).await?;

    Ok(())
}

/// Close the current session (fire and forget)
pub async fn close_session(node_id: &str, rabbitmq_channel: &Channel) -> Result<()> {
    let service_id = "service";
    let cmd_id = Uuid::new_v4().to_string();

    let request = CommandRequest {
        command_id: cmd_id,
        client_id: service_id.to_string(),
        node_id: node_id.to_string(),
        command: NodeCommand::Session(SessionCommand::Close),
    };

    let message = NodeDirectMessage::Command(request);
    publish_json(rabbitmq_channel, &node_queue_name(node_id), &message).await?;

    Ok(())
}

/// Execute an operation in one-shot mode
/// Sends the operation prompt directly to the node session and waits for response
/// If use_existing_session is false, creates and closes a session for this operation
/// Execute an operation in one-shot mode
/// Returns (summary, result) - for one-shot mode, summary is empty and result contains the response
pub async fn execute_one_shot(
    operation_id: &str,
    node_id: &str,
    spec: &SemanticOperationSpec,
    working_dir: Option<String>,
    rabbitmq_channel: &Channel,
    response_tracker: Arc<ResponseTracker>,
    database: Arc<Database>,
    mut cancel_rx: oneshot::Receiver<()>,
    use_existing_session: bool,
) -> Result<(String, String)> {
    //
    // Create session if needed.
    //
    if !use_existing_session {
        create_session(node_id, spec.yolo_mode, working_dir.clone(), rabbitmq_channel, response_tracker.clone())
            .await
            .context("Failed to create session for one-shot operation")?;
    }

    //
    // Generate a unique command ID for tracking.
    //
    let service_id = "service";

    //
    // Log the prompt being sent.
    //
    let _ = database.append_output(operation_id, &fmt_outgoing("Sending prompt to agent", &spec.operation_prompt)).await;

    //
    // Log to event log.
    //
    let prompt_preview: String = spec.operation_prompt.chars().take(100).collect();
    common::log_info!("SemanticPromptSent: op={} node={} prompt={}", &operation_id[..8], &node_id[..8], prompt_preview);

    //
    // Send the prompt.
    //
    let prompt_cmd_id = Uuid::new_v4().to_string();
    let transaction_id = Uuid::new_v4().to_string();
    let prompt_rx = response_tracker.register(prompt_cmd_id.clone());

    let prompt_request = CommandRequest {
        command_id: prompt_cmd_id.clone(),
        client_id: service_id.to_string(),
        node_id: node_id.to_string(),
        command: NodeCommand::Session(SessionCommand::Prompt {
            text: spec.operation_prompt.clone(),
            transaction_id: transaction_id.clone(),
        }),
    };

    let message = NodeDirectMessage::Command(prompt_request);
    publish_json(rabbitmq_channel, &node_queue_name(node_id), &message).await?;

    //
    // Wait for prompt response with timeout and cancellation support.
    //
    let timeout_duration = Duration::from_secs(spec.timeout);

    let response = tokio::select! {
        result = timeout(timeout_duration, prompt_rx) => {
            match result {
                Ok(Ok(cmd_response)) => {
                    //
                    // Extract response from
                    // SessionCommandResult::PromptResponse.
                    //
                    match cmd_response.result {
                        common::NodeCommandResult::Session(common::SessionCommandResult::PromptResponse { response, .. }) => {
                            //
                            // Log the response.
                            //
                            let _ = database.append_output(operation_id, &fmt_incoming("Agent response", &response)).await;
                            let response_preview: String = response.chars().take(100).collect();
                            common::log_info!("SemanticResponseReceived: op={} len={} response={}", &operation_id[..8], response.len(), response_preview);
                            //
                            // One-shot mode: summary is empty, result is the response.
                            //
                            Ok((String::new(), response))
                        }
                        common::NodeCommandResult::Session(common::SessionCommandResult::TransactionCancelled { .. }) => {
                            let _ = database.append_output(operation_id, &fmt_error("Transaction cancelled")).await;
                            common::log_error!("SemanticResponseError: op={} error=cancelled", &operation_id[..8]);
                            Err(anyhow::anyhow!("Transaction cancelled"))
                        }
                        common::NodeCommandResult::Error { message } => {
                            let _ = database.append_output(operation_id, &fmt_error(&format!("Error: {}", message))).await;
                            common::log_error!("SemanticResponseError: op={} error={}", &operation_id[..8], message);
                            Err(anyhow::anyhow!("Node error: {}", message))
                        }
                        _ => Err(anyhow::anyhow!("Unexpected response type")),
                    }
                }
                Ok(Err(_)) => Err(anyhow::anyhow!("Response channel closed")),
                Err(_) => {
                    let _ = database.append_output(operation_id, &fmt_error(&format!("Operation timed out after {} seconds", spec.timeout))).await;
                    common::log_error!("SemanticResponseError: op={} error=timeout", &operation_id[..8]);

                    //
                    // Cancel the in-flight prompt on the node, then close session.
                    //

                    let _ = cancel_transaction(node_id, &transaction_id, rabbitmq_channel).await;
                    let _ = close_session(node_id, rabbitmq_channel).await;

                    Err(anyhow::anyhow!("Operation timed out after {} seconds", spec.timeout))
                }
            }
        }
        _ = &mut cancel_rx => {
            let _ = database.append_output(operation_id, &fmt_error("Operation cancelled")).await;
            common::log_error!("SemanticResponseError: op={} error=cancelled", &operation_id[..8]);

            //
            // Cancel the in-flight prompt on the node, then close session.
            //

            let _ = cancel_transaction(node_id, &transaction_id, rabbitmq_channel).await;
            let _ = close_session(node_id, rabbitmq_channel).await;

            Err(anyhow::anyhow!("Operation cancelled"))
        }
    }?;

    //
    // Close session if we created it.
    //
    if !use_existing_session {
        let _ = close_session(node_id, rabbitmq_channel).await;
    }

    //
    // Response is already a (String, String) tuple from the match arm.
    //
    Ok(response)
}

/// Execute an operation in agent mode
/// Uses an AI model to orchestrate the operation with session_prompt tool calls
/// LLM configuration (API key, provider, model, system prompt) comes from ServiceConfig
/// If use_existing_session is false, creates and closes a session for this operation
/// Returns (summary, result) where summary is a brief description and result contains findings
pub async fn execute_agent_mode(
    operation_id: &str,
    node_id: &str,
    spec: &SemanticOperationSpec,
    working_dir: Option<String>,
    config: &Arc<TokioRwLock<ServiceConfig>>,
    rabbitmq_channel: &Channel,
    response_tracker: Arc<ResponseTracker>,
    database: Arc<Database>,
    mut cancel_rx: oneshot::Receiver<()>,
    use_existing_session: bool,
) -> Result<(String, String)> {
    //
    // Create session if needed.
    //
    if !use_existing_session {
        create_session(node_id, spec.yolo_mode, working_dir.clone(), rabbitmq_channel, response_tracker.clone())
            .await
            .context("Failed to create session for agent mode operation")?;
    }

    //
    // Reload config from database to ensure fresh values.
    //
    {
        let mut config_write = config.write().await;
        let _ = config_write.reload().await;
    }

    //
    // Acquire read lock on config.
    //
    let config = config.read().await;

    //
    // Load AI configuration from model definitions.
    //
    let model_def = if let Some(ref model_ref) = spec.model_ref {
        config.find_model_definition(model_ref)
            .ok_or_else(|| anyhow::anyhow!("Model '{}' not found. Configure in Settings > LLM Providers.", model_ref))?
    } else {
        config.get_semantic_ops_model_def()
            .ok_or_else(|| anyhow::anyhow!("No LLM configured for Semantic Ops. Configure in Settings > LLM Providers."))?
    };

    let (provider_str, model, api_key) = (model_def.provider, model_def.model, model_def.api_key);

    //
    // Use the built-in semantic ops agent prompt.
    //
    let agent_prompt = SEMANTIC_OP_AGENT_PROMPT;

    //
    // Parse provider string.
    //
    let provider = Provider::from_str(&provider_str)
        .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", provider_str))?;

    //
    // Create AI client.
    //
    let client = create_ai_client(provider, api_key.clone())?;

    //
    // Define the session_prompt tool.
    //
    let tools = vec![Tool {
        name: "session_prompt".to_string(),
        description: Some("Send a prompt/instruction to the remote agent session and receive their response. Use this to instruct the remote agent to perform tasks, gather information, or execute commands.".to_string()),
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

    //
    // Create extended system prompt with manual tool calling and completion
    // instructions.
    //
    let system_prompt = get_system_prompt_with_tools_and_completion(&agent_prompt, &tools);

    //
    // Initialize conversation.
    //
    let mut conversation_history: Vec<Message> = vec![
        Message::system(&system_prompt),
        Message::user(format!(
            "START TASK>>>\n\n{}\n\n<<<END TASK",
            spec.operation_prompt
        )),
    ];

    let mut final_summary = String::new();
    let mut final_result = String::new();
    let start_time = std::time::Instant::now();
    let timeout_duration = std::time::Duration::from_secs(spec.timeout);

    //
    // Log the start of agent mode.
    //
    let _ = database.append_output(operation_id, &fmt_agent_start(&provider_str, &model, spec.agent_iterations as usize)).await;

    let _ = database.append_output(operation_id, &fmt_outgoing("Task", &spec.operation_prompt)).await;

    //
    // Agent iteration loop.
    //
    for iteration in 1..=spec.agent_iterations {
        //
        // Check timeout.
        //
        if start_time.elapsed() > timeout_duration {
            let _ = close_session(node_id, rabbitmq_channel).await;
            return Err(anyhow::anyhow!(
                "Operation timed out after {} seconds",
                spec.timeout
            ));
        }

        //
        // Check for cancellation.
        //
        if cancel_rx.try_recv().is_ok() {
            let _ = database.append_output(operation_id, &fmt_error("Operation cancelled")).await;
            let _ = close_session(node_id, rabbitmq_channel).await;
            return Err(anyhow::anyhow!("Operation cancelled"));
        }

        //
        // Log iteration start.
        //
        let _ = database.append_output(operation_id, &fmt_iteration(iteration as usize, spec.agent_iterations as usize)).await;

        //
        // Build and execute AI request.
        //
        let request = ChatCompletionRequest::new(model.clone(), conversation_history.clone())
            .with_max_tokens(4096);

        //
        // Execute non-streaming request.
        //
        let response = tokio::select! {
            result = client.chat_completion(request) => {
                result.context("Failed to complete AI request")?
            }
            _ = &mut cancel_rx => {
                let _ = database.append_output(operation_id, &fmt_error("Operation cancelled")).await;
                let _ = close_session(node_id, rabbitmq_channel).await;
                return Err(anyhow::anyhow!("Operation cancelled"));
            }
        };

        //
        // Extract response text.
        //
        let text_content = response.text().unwrap_or_default().to_string();

        //
        // Log AI response.
        //
        let _ = database.append_output(operation_id, &fmt_incoming("AI Response", &text_content)).await;

        //
        // IMPORTANT: Check for tool calls FIRST before completion signals.
        // If a model outputs both a tool call and completion in the same message,
        // the completion is hallucinated (it can't know the result before the
        // tool executes). We must execute the tool and ignore the fake completion.
        //
        if let Some((tool_name, tool_args, remaining_text)) = parse_manual_tool_call(&text_content)
        {
            if !remaining_text.is_empty() {
                final_summary = remaining_text.clone();
            }

            //
            // Add assistant message to history.
            //
            conversation_history.push(Message::assistant(&text_content));

            let result = if tool_name == "session_prompt" {
                //
                // Extract the prompt text.
                //
                let prompt_text = tool_args
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");

                //
                // Log tool call.
                //
                let _ = database.append_output(operation_id, &fmt_outgoing("Tool call: session_prompt", prompt_text)).await;

                //
                // Send prompt to the remote agent.
                //
                tokio::select! {
                    result = send_remote_prompt(
                        operation_id,
                        node_id,
                        prompt_text,
                        rabbitmq_channel,
                        response_tracker.clone(),
                        spec.timeout,
                    ) => {
                        match result {
                            Ok(response) => {
                                let _ = database.append_output(operation_id, &fmt_incoming("Tool result", &response)).await;
                                response
                            }
                            Err(e) => {
                                let error_msg = format!("Tool error: {}", e);
                                let _ = database.append_output(operation_id, &fmt_error(&error_msg)).await;
                                error_msg
                            }
                        }
                    }
                    _ = &mut cancel_rx => {
                        let _ = database.append_output(operation_id, &fmt_error("Operation cancelled")).await;
                        let _ = close_session(node_id, rabbitmq_channel).await;
                        return Err(anyhow::anyhow!("Operation cancelled"));
                    }
                }
            } else {
                let error_msg = format!("Unknown tool: {}", tool_name);
                let _ = database.append_output(operation_id, &fmt_error(&error_msg)).await;
                error_msg
            };

            //
            // Add tool result as user message.
            //
            conversation_history.push(Message::user(format!("Tool '{}' result: {}", tool_name, result)));

            //
            // Continue to next iteration.
            //
            continue;
        }

        //
        // No tool call found - check for completion signal.
        //
        if let Some((is_complete, summary, result, _remaining_text)) =
            parse_completion_signal(&text_content)
        {
            if is_complete {
                final_summary = if !summary.is_empty() {
                    summary
                } else {
                    text_content.clone()
                };
                final_result = result;

                //
                // Log completion.
                //
                let _ = database.append_output(operation_id, &fmt_complete(&final_summary)).await;

                //
                // Add final assistant message to history.
                //
                conversation_history.push(Message::assistant(&text_content));

                break;
            }
        }

        //
        // No tool call and no completion - log the response and exit.
        //
        if !text_content.is_empty() {
            final_summary = text_content.clone();
        }

        //
        // Add assistant message to history.
        //
        conversation_history.push(Message::assistant(&text_content));

        break;
    }

    //
    // Close session if we created it.
    //
    if !use_existing_session {
        let _ = close_session(node_id, rabbitmq_channel).await;
    }

    Ok((final_summary, final_result))
}

/// Send a prompt to a remote node and wait for response
async fn send_remote_prompt(
    operation_id: &str,
    node_id: &str,
    prompt: &str,
    rabbitmq_channel: &Channel,
    response_tracker: Arc<ResponseTracker>,
    timeout_secs: u64,
) -> Result<String> {
    let service_id = "service";
    let prompt_cmd_id = Uuid::new_v4().to_string();
    let transaction_id = Uuid::new_v4().to_string();
    let prompt_rx = response_tracker.register(prompt_cmd_id.clone());

    //
    // Log the tool prompt to event log.
    //
    let prompt_preview: String = prompt.chars().take(100).collect();
    common::log_info!("SemanticToolPrompt: op={} node={} prompt={}", &operation_id[..8], &node_id[..8], prompt_preview);

    let prompt_request = CommandRequest {
        command_id: prompt_cmd_id.clone(),
        client_id: service_id.to_string(),
        node_id: node_id.to_string(),
        command: NodeCommand::Session(SessionCommand::Prompt {
            text: prompt.to_string(),
            transaction_id,
        }),
    };

    let message = NodeDirectMessage::Command(prompt_request);
    publish_json(rabbitmq_channel, &node_queue_name(node_id), &message).await?;

    //
    // Wait for response with timeout.
    //
    let timeout_duration = Duration::from_secs(timeout_secs);

    let response = match timeout(timeout_duration, prompt_rx).await {
        Ok(Ok(cmd_response)) => match cmd_response.result {
            common::NodeCommandResult::Session(common::SessionCommandResult::PromptResponse {
                response,
                ..
            }) => {
                let response_preview: String = response.chars().take(100).collect();
                common::log_info!("SemanticToolResponse: op={} len={} response={}", &operation_id[..8], response.len(), response_preview);
                Ok(response)
            }
            common::NodeCommandResult::Session(common::SessionCommandResult::TransactionCancelled { .. }) => {
                common::log_error!("SemanticToolError: op={} error=cancelled", &operation_id[..8]);
                Err(anyhow::anyhow!("Transaction cancelled"))
            }
            common::NodeCommandResult::Error { message } => {
                common::log_error!("SemanticToolError: op={} error={}", &operation_id[..8], message);
                Err(anyhow::anyhow!("Node error: {}", message))
            }
            _ => Err(anyhow::anyhow!("Unexpected response type")),
        },
        Ok(Err(_)) => Err(anyhow::anyhow!("Response channel closed")),
        Err(_) => {
            common::log_error!("SemanticToolError: op={} error=timeout", &operation_id[..8]);
            Err(anyhow::anyhow!(
                "Prompt timed out after {} seconds",
                timeout_secs
            ))
        }
    }?;

    Ok(response)
}
