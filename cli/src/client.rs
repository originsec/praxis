use anyhow::{Result, anyhow};
use common::ClientTransport;
use common::{
    CLIENT_SIGNAL_QUEUE, ChainDefinitionFull, ChainDefinitionInfo, ChainDefinitionInput,
    ChainExecutionUpdate, ChainTriggerInfo, ClientBroadcastMessage, ClientDirectMessage,
    ClientRegistration, ClientSignalMessage, InterceptMethod, InterceptRule, InterceptStatus,
    InterceptedTrafficEntry, LuaAgentScriptInfo, OperationDefinitionInfo, RuleScope,
    SemanticOpUpdate, SystemState, TargetDirection, TargetSpec, TerminalOutput, TrafficLogFilters,
    TrafficMatchWithDetails, TrafficSearchFilters, TriggerConfig,
    mcp::{build_notification_frame, build_request_frame},
    publish_json, publish_terminal_command,
};
use lapin::Channel;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, oneshot};

use crate::intercept_live::{try_push_bounded, LivePushResult, INTERCEPT_LIVE_CAPACITY};

pub struct Client {
    channel: Channel,
    client_id: String,
    timeout: Duration,
    state: Arc<Mutex<ClientState>>,
    consumer_handle: Option<tokio::task::JoinHandle<()>>,
    register_cmd_tx: Option<mpsc::UnboundedSender<RegisterCmd>>,
    register_worker: Option<tokio::task::JoinHandle<()>>,
}

/// Serialized registration / ServiceOnline re-register commands.
enum RegisterCmd {
    Register {
        expected_instance: Option<String>,
        resp: oneshot::Sender<Result<(), String>>,
    },
    Shutdown,
}

struct PendingRegistration {
    nonce: String,
    expected_instance: Option<String>,
    resp: oneshot::Sender<Result<(), String>>,
}

struct PendingClear {
    expected_instance: String,
    /// (service_instance_id, deleted_count, generation)
    resp: oneshot::Sender<Result<(String, usize, u64), String>>,
}

//
// Outcome of an intercept-rule create/update/delete signal. Folded into a
// single enum because the service emits a different ClientDirectMessage
// variant per outcome and callers just need the result.
//

#[derive(Debug, Clone)]
pub enum RuleOpOutcome {
    Created(InterceptRule),
    Updated(InterceptRule),
    Deleted { success: bool },
    Error(String),
}

//
// In-flight ACP request. When `text_buf` is Some, streamed
// `agent_message_chunk` text for the tracked session_id is appended.
//

struct PendingAcp {
    response_tx: Option<oneshot::Sender<Result<Value, String>>>,
    text_buf: Option<String>,
    session_id: Option<String>,
}

#[derive(Default)]
struct ClientState {
    system_state: Option<SystemState>,
    pending_initial_state: Option<oneshot::Sender<()>>,
    acp_event_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    terminal_output_tx: Option<tokio::sync::mpsc::UnboundedSender<TerminalOutput>>,
    pending_config: Option<oneshot::Sender<HashMap<String, String>>>,
    pending_config_save: Option<oneshot::Sender<Result<(), String>>>,
    pending_acp: HashMap<String, PendingAcp>,
    pending_terminal_creates: HashMap<String, oneshot::Sender<Result<String, String>>>,
    cached_project_paths: Vec<String>,
    recon_cache: HashMap<(String, String), common::ReconResult>,
    operations: Vec<SemanticOpUpdate>,
    operation_definitions: Vec<OperationDefinitionInfo>,
    chain_definitions: Vec<ChainDefinitionInfo>,
    chain_executions: Vec<ChainExecutionUpdate>,
    chain_triggers: Vec<ChainTriggerInfo>,
    current_chain: Option<ChainDefinitionFull>,
    pending_semantic_op: Option<oneshot::Sender<String>>,

    //
    // Awaitable list refreshes: when set, the matching list response fires
    // the sender with the fresh data in addition to updating the cache.
    //
    pending_op_def_list: Option<oneshot::Sender<Vec<OperationDefinitionInfo>>>,
    pending_semantic_op_list: Option<oneshot::Sender<Vec<SemanticOpUpdate>>>,
    pending_chain_list: Option<oneshot::Sender<Vec<ChainDefinitionInfo>>>,
    pending_chain_execution_list: Option<oneshot::Sender<Vec<ChainExecutionUpdate>>>,
    pending_chain_trigger_list: Option<oneshot::Sender<Vec<ChainTriggerInfo>>>,
    lua_agent_scripts: Vec<LuaAgentScriptInfo>,
    intercept_targets_text: String,
    intercept_targets_parsed: Vec<common::InterceptTargetConfig>,
    intercept_targets_error: Option<String>,

    //
    // Intercept traffic: one-shot senders keyed by client-generated
    // request_id so concurrent same-kind queries cannot be swapped.
    //
    pending_traffic_log:
        HashMap<String, oneshot::Sender<Result<(Vec<InterceptedTrafficEntry>, usize), String>>>,
    pending_traffic_search:
        HashMap<String, oneshot::Sender<Result<(Vec<InterceptedTrafficEntry>, usize), String>>>,
    pending_traffic_matches:
        HashMap<String, oneshot::Sender<Result<(Vec<TrafficMatchWithDetails>, usize), String>>>,
    pending_traffic_clear: HashMap<String, PendingClear>,
    pending_rules_list: Option<oneshot::Sender<Vec<InterceptRule>>>,
    pending_rule_op: Option<oneshot::Sender<RuleOpOutcome>>,
    pending_traffic_get:
        HashMap<String, oneshot::Sender<Result<Option<InterceptedTrafficEntry>, String>>>,
    pending_intercept_toggles: HashMap<String, oneshot::Sender<Result<(), String>>>,
    //
    // Bounded live intercept delivery (drop-when-full). Status uses a
    // watch channel (last-value-wins) so it does not compete with traffic.
    //
    intercept_entries_tx:
        Option<mpsc::Sender<(String, u64, Vec<InterceptedTrafficEntry>)>>,
    intercept_matches_tx:
        Option<mpsc::Sender<(String, u64, Vec<TrafficMatchWithDetails>)>>,
    intercept_status_tx: Option<tokio::sync::watch::Sender<Option<InterceptStatus>>>,
    /// Notifies TUI when service_instance_id rebinds (registration / restart).
    service_instance_tx: Option<tokio::sync::watch::Sender<Option<String>>>,
    /// Last-value-wins clear boundary so late successes still reconcile the TUI.
    clear_boundary_tx: Option<tokio::sync::watch::Sender<Option<(String, u64)>>>,
    /// Serialized register / ServiceOnline re-register worker.
    register_cmd_tx: Option<mpsc::UnboundedSender<RegisterCmd>>,
    /// In-flight registration correlated by nonce (not StateUpdate).
    pending_registration: Option<PendingRegistration>,
    intercept_entries_drops: AtomicU64,
    intercept_matches_drops: AtomicU64,
    /// Clear-epoch from last successful TrafficCleared (ignore older live batches).
    clear_epoch: AtomicU64,
    /// Service process identity for generation scoping across restarts.
    service_instance_id: Option<String>,

    //
    // LogQuery: single in-flight request; the Err side carries the service
    // error message so the TUI can show it verbatim.
    //
    pending_log_query: Option<oneshot::Sender<Result<LogQueryResults, String>>>,

    //
    // Documentation helper agent: streamed responses are forwarded to this
    // subscriber (set by the app's event loop). Correlated by request_id in
    // the event payload, so overlapping requests remain distinguishable.
    //
    doc_helper_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::event::DocHelperEvent>>,
}

#[derive(Clone, Debug)]
pub struct LogQueryResults {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total_count: usize,
}

impl Client {
    pub async fn connect(url: &str, timeout_secs: u64, client_id: String) -> Result<Self> {
        let transport = ClientTransport::connect(url, &client_id).await?;

        let state = Arc::new(Mutex::new(ClientState::default()));

        let direct_state = Arc::clone(&state);
        let broadcast_state = Arc::clone(&state);
        let consumer_handle = transport
            .start_consuming(
                "tui",
                move |data| {
                    let state = Arc::clone(&direct_state);
                    async move { Self::handle_direct_message(&state, &data).await }
                },
                move |data| {
                    let state = Arc::clone(&broadcast_state);
                    async move { Self::handle_broadcast_message(&state, &data).await }
                },
            )
            .await?;

        let mut client = Self {
            channel: transport.channel().clone(),
            client_id,
            timeout: Duration::from_secs(timeout_secs),
            state,
            consumer_handle: Some(consumer_handle),
            register_cmd_tx: None,
            register_worker: None,
        };

        //
        // Single serial registration worker for initial connect and
        // ServiceOnline re-register (correlated nonce + expected instance).
        //
        client.spawn_register_worker().await;
        client.register().await?;

        Ok(client)
    }

    async fn spawn_register_worker(&mut self) {
        let (tx, mut rx) = mpsc::unbounded_channel::<RegisterCmd>();
        self.state.lock().await.register_cmd_tx = Some(tx.clone());
        self.register_cmd_tx = Some(tx);
        let state = self.state.clone();
        let channel = self.channel.clone();
        let client_id = self.client_id.clone();
        let timeout = self.timeout;
        self.register_worker = Some(tokio::spawn(async move {
            //
            // Single serial worker. Newer ServiceOnline / Register cmds
            // supersede an in-flight attempt (coalesce FIFO backlog) so a
            // dead announced instance cannot block re-registration for the
            // full timeout window.
            //
            enum PollCmd {
                None,
                Shutdown,
                Supersede,
            }

            /// Drain queued cmds; Shutdown wins; otherwise keep newest Register.
            fn drain_register_cmds(
                rx: &mut mpsc::UnboundedReceiver<RegisterCmd>,
                expected_instance: &mut Option<String>,
                resp: &mut oneshot::Sender<Result<(), String>>,
                deadline: &mut tokio::time::Instant,
                timeout: Duration,
            ) -> PollCmd {
                let mut saw = PollCmd::None;
                loop {
                    match rx.try_recv() {
                        Ok(RegisterCmd::Shutdown) => return PollCmd::Shutdown,
                        Ok(RegisterCmd::Register {
                            expected_instance: next_expected,
                            resp: next_resp,
                        }) => {
                            let prev = std::mem::replace(resp, next_resp);
                            let _ = prev.send(Err(
                                "registration superseded by a newer ServiceOnline".into(),
                            ));
                            *expected_instance = next_expected;
                            *deadline = tokio::time::Instant::now() + timeout;
                            saw = PollCmd::Supersede;
                        }
                        Err(_) => return saw,
                    }
                }
            }

            let mut shutdown = false;
            while !shutdown {
                let Some(cmd) = rx.recv().await else {
                    break;
                };
                let (mut expected_instance, mut resp) = match cmd {
                    RegisterCmd::Shutdown => break,
                    RegisterCmd::Register {
                        expected_instance,
                        resp,
                    } => (expected_instance, resp),
                };

                let mut deadline = tokio::time::Instant::now() + timeout;
                match drain_register_cmds(
                    &mut rx,
                    &mut expected_instance,
                    &mut resp,
                    &mut deadline,
                    timeout,
                ) {
                    PollCmd::Shutdown => {
                        let _ = resp
                            .send(Err("registration aborted: client shutting down".into()));
                        break;
                    }
                    PollCmd::None | PollCmd::Supersede => {}
                }

                //
                // Retry until deadline when a shared-queue consumer acks the
                // wrong instance (nonce matched, expected not).
                //
                let mut last_err = String::from("registration failed");
                let outcome = loop {
                    match drain_register_cmds(
                        &mut rx,
                        &mut expected_instance,
                        &mut resp,
                        &mut deadline,
                        timeout,
                    ) {
                        PollCmd::Shutdown => {
                            let mut s = state.lock().await;
                            s.pending_registration = None;
                            shutdown = true;
                            break Err("registration aborted: client shutting down".into());
                        }
                        PollCmd::Supersede => {
                            last_err = String::from("registration superseded; retrying");
                        }
                        PollCmd::None => {}
                    }

                    if tokio::time::Instant::now() >= deadline {
                        let mut s = state.lock().await;
                        s.pending_registration = None;
                        break Err(format!(
                            "registration timed out after {}s: {}",
                            timeout.as_secs(),
                            last_err
                        ));
                    }
                    let nonce = uuid::Uuid::new_v4().to_string();
                    let (ack_tx, mut ack_rx) = oneshot::channel();
                    {
                        let mut s = state.lock().await;
                        if let Some(prev) = s.pending_registration.take() {
                            let _ = prev
                                .resp
                                .send(Err("registration superseded by a newer attempt".into()));
                        }
                        s.pending_registration = Some(PendingRegistration {
                            nonce: nonce.clone(),
                            expected_instance: expected_instance.clone(),
                            resp: ack_tx,
                        });
                    }
                    let registration = ClientRegistration {
                        client_id: client_id.clone(),
                        registration_nonce: nonce,
                        expected_service_instance_id: expected_instance
                            .clone()
                            .unwrap_or_default(),
                    };
                    let message = ClientSignalMessage::Registration(registration);
                    if let Err(e) = publish_json(&channel, CLIENT_SIGNAL_QUEUE, &message).await {
                        let mut s = state.lock().await;
                        s.pending_registration = None;
                        last_err = format!("register publish failed: {}", e);
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        continue;
                    }

                    //
                    // Wait for ack in short slices so a newer ServiceOnline can
                    // supersede without blocking for the full remaining timeout.
                    //
                    let ack_wait = loop {
                        let left =
                            deadline.saturating_duration_since(tokio::time::Instant::now());
                        if left.is_zero() {
                            break Err(());
                        }
                        let poll = left.min(std::time::Duration::from_millis(200));
                        match tokio::time::timeout(poll, &mut ack_rx).await {
                            Ok(Ok(r)) => break Ok(r),
                            Ok(Err(_)) => {
                                break Ok(Err(
                                    "registration response channel closed; retrying".into(),
                                ));
                            }
                            Err(_) => {
                                match drain_register_cmds(
                                    &mut rx,
                                    &mut expected_instance,
                                    &mut resp,
                                    &mut deadline,
                                    timeout,
                                ) {
                                    PollCmd::Shutdown => {
                                        let mut s = state.lock().await;
                                        s.pending_registration = None;
                                        shutdown = true;
                                        break Ok(Err(
                                            "registration aborted: client shutting down".into(),
                                        ));
                                    }
                                    PollCmd::Supersede => {
                                        let mut s = state.lock().await;
                                        s.pending_registration = None;
                                        last_err =
                                            String::from("registration superseded; retrying");
                                        break Ok(Err(
                                            common::clear_epoch::REGISTRATION_RETRY_MARKER.into(),
                                        ));
                                    }
                                    PollCmd::None => {
                                        if tokio::time::Instant::now() >= deadline {
                                            break Err(());
                                        }
                                    }
                                }
                            }
                        }
                    };

                    match ack_wait {
                        Ok(Ok(())) => {
                            //
                            // Connect contract: successful registration implies
                            // system_state is available for get_state() /
                            // non-interactive commands. Service publishes
                            // StateUpdate before ack; wait until registration
                            // deadline, then fail.
                            //
                            let mut got_state = false;
                            let mut state_superseded = false;
                            loop {
                                if state.lock().await.system_state.is_some() {
                                    got_state = true;
                                    break;
                                }
                                if tokio::time::Instant::now() >= deadline {
                                    break;
                                }
                                match drain_register_cmds(
                                    &mut rx,
                                    &mut expected_instance,
                                    &mut resp,
                                    &mut deadline,
                                    timeout,
                                ) {
                                    PollCmd::Shutdown => {
                                        shutdown = true;
                                        break;
                                    }
                                    PollCmd::Supersede => {
                                        state_superseded = true;
                                        last_err =
                                            String::from("registration superseded; retrying");
                                        break;
                                    }
                                    PollCmd::None => {}
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                            }
                            if shutdown {
                                break Err("registration aborted: client shutting down".into());
                            }
                            if state_superseded {
                                continue;
                            }
                            if got_state {
                                break Ok(());
                            }
                            break Err(
                                "registration ack received but initial system state never arrived"
                                    .into(),
                            );
                        }
                        Ok(Err(e))
                            if e == common::clear_epoch::REGISTRATION_RETRY_MARKER =>
                        {
                            last_err = e;
                            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                            continue;
                        }
                        Ok(Err(e)) if e.contains("shutting down") => {
                            break Err(e);
                        }
                        Ok(Err(e)) if e.contains("channel closed") => {
                            last_err = e;
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            continue;
                        }
                        Ok(Err(e)) => {
                            last_err = e;
                            break Err(last_err.clone());
                        }
                        Err(()) => {
                            let mut s = state.lock().await;
                            s.pending_registration = None;
                            last_err = format!(
                                "registration timed out after {}s",
                                timeout.as_secs()
                            );
                            break Err(last_err.clone());
                        }
                    }
                };
                //
                // Always complete the active waiter (including shutdown abort).
                // Superseded waiters were already notified in drain_register_cmds.
                //
                let _ = resp.send(outcome);
            }
            //
            // Drop the state-held sender so we do not leak a cycle.
            //
            let mut s = state.lock().await;
            s.register_cmd_tx = None;
            s.pending_registration = None;
        }));
    }

    async fn handle_direct_message(state: &Arc<Mutex<ClientState>>, data: &[u8]) {
        let Ok(message) = serde_json::from_slice::<ClientDirectMessage>(data) else {
            return;
        };

        //
        // Legacy CommandResponse routing: terminal-create is correlated by
        // command_id. Other CommandResponses fall through (intercept uses
        // InterceptCommandResult instead).
        //

        if let common::ClientDirectMessage::CommandResponse(ref resp) = message {
            let mut state = state.lock().await;
            if let Some(tx) = state.pending_terminal_creates.remove(&resp.command_id) {
                let result = match &resp.result {
                    common::NodeCommandResult::Terminal(
                        common::TerminalCommandResult::Created { terminal_id },
                    ) => Ok(terminal_id.clone()),
                    common::NodeCommandResult::Error { message } => Err(message.clone()),
                    other => Err(format!("Unexpected terminal create result: {:?}", other)),
                };
                let _ = tx.send(result);
                return;
            }
            drop(state);
        }

        let mut state = state.lock().await;

        match message {
            ClientDirectMessage::RegistrationAck(ack) => {
                //
                // Control-plane: only correlated RegistrationAck rebinds.
                // Delayed acks (wrong nonce / unexpected instance) are ignored
                // so they cannot rebind backward after a newer ServiceOnline.
                //
                let Some(pending) = state.pending_registration.take() else {
                    return;
                };
                let accept = common::clear_epoch::may_accept_registration_ack(
                    state.service_instance_id.as_deref(),
                    &ack.service_instance_id,
                    pending.expected_instance.as_deref(),
                    &pending.nonce,
                    &ack.registration_nonce,
                );
                if !accept {
                    match common::clear_epoch::registration_reject_action(
                        &pending.nonce,
                        &ack.registration_nonce,
                        pending.expected_instance.as_deref(),
                        &ack.service_instance_id,
                    ) {
                        common::clear_epoch::RegistrationRejectAction::Retry
                            if pending.nonce != ack.registration_nonce =>
                        {
                            // Foreign attempt: keep waiting on the same oneshot.
                            state.pending_registration = Some(pending);
                        }
                        common::clear_epoch::RegistrationRejectAction::Retry => {
                            //
                            // Right nonce, wrong instance (shared signal queue):
                            // signal worker to re-publish before deadline.
                            //
                            common::log_warn!(
                                "Registration ack rejected (instance={} expected={:?}); will retry",
                                ack.service_instance_id,
                                pending.expected_instance
                            );
                            let _ = pending.resp.send(Err(
                                common::clear_epoch::REGISTRATION_RETRY_MARKER.into(),
                            ));
                        }
                        common::clear_epoch::RegistrationRejectAction::Fail => {
                            let _ = pending.resp.send(Err(format!(
                                "registration ack rejected (instance={} expected={:?})",
                                ack.service_instance_id, pending.expected_instance
                            )));
                        }
                    }
                    return;
                }
                use std::sync::atomic::Ordering;
                let mut epoch = state.clear_epoch.load(Ordering::Acquire);
                let mut inst = state.service_instance_id.clone();
                let changed = common::clear_epoch::rebind_service_instance(
                    &mut inst,
                    &mut epoch,
                    &ack.service_instance_id,
                );
                state.service_instance_id = inst.clone();
                state.clear_epoch.store(epoch, Ordering::Release);
                if changed {
                    if let Some(ref tx) = state.service_instance_tx {
                        let _ = tx.send(inst);
                    }
                }
                //
                // Completing only after StateUpdate would be ideal; service
                // now publishes state before ack. If state is still missing,
                // signal waiters via pending_initial_state path is not used
                // here — worker polls system_state after Ok.
                //
                let _ = pending.resp.send(Ok(()));
            }
            ClientDirectMessage::StateUpdate(system_state) => {
                //
                // Direct StateUpdate is registration bootstrap only. Accept
                // while a registration is in flight, or until the first state
                // arrives after a successful ack. Once bound with state and
                // not re-registering, ignore late direct updates so a stale
                // overlapping service cannot overwrite the accepted instance.
                // Ongoing node changes arrive via broadcast StateUpdate.
                //
                let accept = state.pending_registration.is_some() || state.system_state.is_none();
                if accept {
                    state.system_state = Some(system_state);
                    //
                    // Legacy: complete initial wait if still used. Prefer
                    // RegistrationAck correlation for new clients.
                    //
                    if let Some(tx) = state.pending_initial_state.take() {
                        let _ = tx.send(());
                    }
                }
            }

            ClientDirectMessage::ServiceConfigResponse { values } => {
                if let Some(tx) = state.pending_config.take() {
                    let _ = tx.send(values);
                }
            }
            ClientDirectMessage::ServiceConfigSaved => {
                if let Some(tx) = state.pending_config_save.take() {
                    let _ = tx.send(Ok(()));
                }
            }
            ClientDirectMessage::ServiceConfigSaveFailed { message } => {
                if let Some(tx) = state.pending_config_save.take() {
                    let _ = tx.send(Err(message));
                }
            }

            //
            // Operation and chain responses.
            //
            ClientDirectMessage::ReconGetResponse {
                node_id,
                agent_short_name,
                recon_result,
                ..
            } => {
                if let Some(ref recon) = recon_result {
                    state.cached_project_paths = recon.config.project_paths.clone();
                    state
                        .recon_cache
                        .insert((node_id.clone(), agent_short_name.clone()), recon.clone());
                }
            }
            ClientDirectMessage::SemanticOpQueued { operation_id, .. } => {
                if let Some(tx) = state.pending_semantic_op.take() {
                    let _ = tx.send(operation_id);
                }
            }
            ClientDirectMessage::SemanticOpUpdate(update) => {
                if let Some(idx) = state
                    .operations
                    .iter()
                    .position(|o| o.operation_id == update.operation_id)
                {
                    state.operations[idx] = update;
                } else {
                    state.operations.push(update);
                }
            }
            ClientDirectMessage::SemanticOpList(ops) => {
                state.operations = ops;
                if let Some(tx) = state.pending_semantic_op_list.take() {
                    let _ = tx.send(state.operations.clone());
                }
            }
            ClientDirectMessage::OpDefListResponse { definitions } => {
                state.operation_definitions = definitions;
                if let Some(tx) = state.pending_op_def_list.take() {
                    let _ = tx.send(state.operation_definitions.clone());
                }
            }
            ClientDirectMessage::ChainDefListResponse { chains } => {
                state.chain_definitions = chains;
                if let Some(tx) = state.pending_chain_list.take() {
                    let _ = tx.send(state.chain_definitions.clone());
                }
            }
            ClientDirectMessage::ChainGetResponse { chain } => {
                state.current_chain = chain;
            }
            ClientDirectMessage::ChainExecutionUpdate(exec) => {
                if let Some(idx) = state
                    .chain_executions
                    .iter()
                    .position(|e| e.execution_id == exec.execution_id)
                {
                    state.chain_executions[idx] = exec;
                } else {
                    state.chain_executions.push(exec);
                }
            }
            ClientDirectMessage::ChainExecutionListResponse { executions } => {
                state.chain_executions = executions;
                if let Some(tx) = state.pending_chain_execution_list.take() {
                    let _ = tx.send(state.chain_executions.clone());
                }
            }

            //
            // Chain trigger responses. The full list response replaces the
            // cache; per-item create/update/delete patch the cache in place.
            //
            ClientDirectMessage::ChainTriggerListResponse { triggers } => {
                state.chain_triggers = triggers;
                if let Some(tx) = state.pending_chain_trigger_list.take() {
                    let _ = tx.send(state.chain_triggers.clone());
                }
            }
            ClientDirectMessage::ChainTriggerCreated { trigger } => {
                if let Some(existing) = state.chain_triggers.iter_mut().find(|t| t.id == trigger.id)
                {
                    *existing = trigger;
                } else {
                    state.chain_triggers.push(trigger);
                }
            }
            ClientDirectMessage::ChainTriggerUpdated { trigger } => {
                if let Some(existing) = state.chain_triggers.iter_mut().find(|t| t.id == trigger.id)
                {
                    *existing = trigger;
                } else {
                    state.chain_triggers.push(trigger);
                }
            }
            ClientDirectMessage::ChainTriggerDeleted { trigger_id } => {
                state.chain_triggers.retain(|t| t.id != trigger_id);
            }

            //
            // ACP JSON-RPC frames: route responses to any pending request,
            // buffer streamed chunks for text-collecting requests, and also
            // forward every frame to any external subscriber (the CLI's
            // orchestrator bridge uses this stream).
            //
            ClientDirectMessage::AcpMessage { json_rpc } => {
                Self::handle_acp_frame(&mut state, &json_rpc);
                if let Some(ref tx) = state.acp_event_tx {
                    let _ = tx.send(json_rpc);
                }
            }

            ClientDirectMessage::TerminalOutput(output) => {
                if let Some(ref tx) = state.terminal_output_tx {
                    let _ = tx.send(output);
                }
            }

            ClientDirectMessage::LuaAgentScriptListResponse { scripts } => {
                state.lua_agent_scripts = scripts;
            }
            ClientDirectMessage::LuaAgentScriptAdded { .. }
            | ClientDirectMessage::LuaAgentScriptUpdated { .. }
            | ClientDirectMessage::LuaAgentScriptDeleted { .. }
            | ClientDirectMessage::LuaAgentScriptDefaultsReset { .. }
            | ClientDirectMessage::LuaAgentScriptDisabledToggled { .. } => {
                // Trigger a re-fetch handled by the app layer.
            }

            ClientDirectMessage::InterceptTargetsState {
                text,
                targets,
                error,
            } => {
                state.intercept_targets_text = text;
                state.intercept_targets_parsed = targets;
                state.intercept_targets_error = error;
            }

            //
            // ClientDirectMessage::SessionUpdate is the legacy NodeCommand
            // streaming path; node sessions now stream via ACP `session/update`
            // notifications carried in `ClientDirectMessage::AcpMessage`. The
            // variant is still defined for the web client; ignore it here.
            //
            ClientDirectMessage::SessionUpdate(_) => {}

            //
            // Intercept traffic responses — correlated by request_id.
            //
            ClientDirectMessage::TrafficLogResponse {
                request_id,
                entries,
                total_count,
                error,
            } => {
                if let Some(tx) = state.pending_traffic_log.remove(&request_id) {
                    let _ = tx.send(match error {
                        Some(error) => Err(error),
                        None => Ok((entries, total_count)),
                    });
                }
            }
            ClientDirectMessage::TrafficSearchResponse {
                request_id,
                entries,
                total_count,
                error,
            } => {
                if let Some(tx) = state.pending_traffic_search.remove(&request_id) {
                    let _ = tx.send(match error {
                        Some(error) => Err(error),
                        None => Ok((entries, total_count)),
                    });
                }
            }
            ClientDirectMessage::TrafficMatchesResponse {
                request_id,
                matches,
                total_count,
                error,
            } => {
                if let Some(tx) = state.pending_traffic_matches.remove(&request_id) {
                    let _ = tx.send(match error {
                        Some(error) => Err(error),
                        None => Ok((matches, total_count)),
                    });
                }
            }
            ClientDirectMessage::TrafficCleared {
                request_id,
                deleted_count,
                generation,
                service_instance_id,
                error,
            } => {
                //
                // Pending clear is scoped to the instance at request time.
                // Foreign instance → error (never transplant generation).
                // Same instance → apply epoch + boundary watch.
                //
                if let Some(pending) = state.pending_traffic_clear.remove(&request_id) {
                    if let Some(err) = error {
                        let _ = pending.resp.send(Err(err));
                    } else if !common::clear_epoch::clear_pending_accepts_response(
                        &pending.expected_instance,
                        &service_instance_id,
                    ) {
                        let _ = pending.resp.send(Err(format!(
                            "clear response from service instance {} after rebind to {}; retry clear",
                            service_instance_id, pending.expected_instance
                        )));
                    } else {
                        use std::sync::atomic::Ordering;
                        let mut epoch = state.clear_epoch.load(Ordering::Acquire);
                        if common::clear_epoch::apply_clear_response(
                            state.service_instance_id.as_deref(),
                            &mut epoch,
                            &service_instance_id,
                            generation,
                        ) {
                            state.clear_epoch.store(epoch, Ordering::Release);
                            if let Some(ref tx) = state.clear_boundary_tx {
                                let _ = tx.send(Some((service_instance_id.clone(), generation)));
                            }
                            let _ = pending.resp.send(Ok((
                                service_instance_id,
                                deleted_count,
                                generation,
                            )));
                        } else {
                            let _ = pending.resp.send(Err(format!(
                                "clear response rejected for instance {}",
                                service_instance_id
                            )));
                        }
                    }
                } else if error.is_none() {
                    //
                    // Late success after oneshot timeout: still reconcile TUI
                    // for the current instance only.
                    //
                    use std::sync::atomic::Ordering;
                    let mut epoch = state.clear_epoch.load(Ordering::Acquire);
                    if common::clear_epoch::apply_clear_response(
                        state.service_instance_id.as_deref(),
                        &mut epoch,
                        &service_instance_id,
                        generation,
                    ) {
                        state.clear_epoch.store(epoch, Ordering::Release);
                        if let Some(ref tx) = state.clear_boundary_tx {
                            let _ = tx.send(Some((service_instance_id.clone(), generation)));
                        }
                    }
                }
            }
            ClientDirectMessage::TrafficGetResponse {
                request_id,
                id: _,
                entry,
                error,
            } => {
                if let Some(tx) = state.pending_traffic_get.remove(&request_id) {
                    let _ = tx.send(error.map_or(Ok(entry), Err));
                }
            }
            ClientDirectMessage::InterceptRuleListResponse { rules } => {
                if let Some(tx) = state.pending_rules_list.take() {
                    let _ = tx.send(rules);
                }
            }
            ClientDirectMessage::InterceptRuleCreated { rule } => {
                if let Some(tx) = state.pending_rule_op.take() {
                    let _ = tx.send(RuleOpOutcome::Created(rule));
                }
            }
            ClientDirectMessage::InterceptRuleUpdated { rule } => {
                if let Some(tx) = state.pending_rule_op.take() {
                    let _ = tx.send(RuleOpOutcome::Updated(rule));
                }
            }
            ClientDirectMessage::InterceptRuleDeleted { id: _, success } => {
                if let Some(tx) = state.pending_rule_op.take() {
                    let _ = tx.send(RuleOpOutcome::Deleted { success });
                }
            }
            ClientDirectMessage::InterceptRuleError { message } => {
                if let Some(tx) = state.pending_rule_op.take() {
                    let _ = tx.send(RuleOpOutcome::Error(message));
                }
            }
            ClientDirectMessage::InterceptStatusUpdate(status) => {
                //
                // watch::send is synchronous last-value-wins (not a dropped future).
                //
                if let Some(ref tx) = state.intercept_status_tx {
                    let _ = tx.send(Some(status));
                }
            }
            ClientDirectMessage::InterceptCommandResult {
                request_id,
                node_id: _,
                error,
                status,
            } => {
                if let Some(status) = status {
                    if let Some(ref tx) = state.intercept_status_tx {
                        let _ = tx.send(Some(status));
                    }
                }
                if let Some(tx) = state.pending_intercept_toggles.remove(&request_id) {
                    let _ = tx.send(match error {
                        Some(msg) => Err(msg),
                        None => Ok(()),
                    });
                }
            }

            //
            // LogQuery responses. Only one query is in flight at a time.
            //
            ClientDirectMessage::LogQueryResponse {
                columns,
                rows,
                total_count,
            } => {
                if let Some(tx) = state.pending_log_query.take() {
                    let _ = tx.send(Ok(LogQueryResults {
                        columns,
                        rows,
                        total_count,
                    }));
                }
            }
            ClientDirectMessage::LogQueryError { message } => {
                if let Some(tx) = state.pending_log_query.take() {
                    let _ = tx.send(Err(message));
                }
            }

            //
            // Documentation helper streaming responses.
            //
            ClientDirectMessage::DocHelperChunk { request_id, delta } => {
                if let Some(ref tx) = state.doc_helper_tx {
                    let _ = tx.send(crate::event::DocHelperEvent::Chunk { request_id, delta });
                }
            }
            ClientDirectMessage::DocHelperFollowUp { request_id } => {
                if let Some(ref tx) = state.doc_helper_tx {
                    let _ = tx.send(crate::event::DocHelperEvent::FollowUp { request_id });
                }
            }
            ClientDirectMessage::DocHelperComplete { request_id } => {
                if let Some(ref tx) = state.doc_helper_tx {
                    let _ = tx.send(crate::event::DocHelperEvent::Complete { request_id });
                }
            }
            ClientDirectMessage::DocHelperError {
                request_id,
                message,
            } => {
                if let Some(ref tx) = state.doc_helper_tx {
                    let _ = tx.send(crate::event::DocHelperEvent::Error {
                        request_id,
                        message,
                    });
                }
            }

            _ => {}
        }
    }

    async fn handle_broadcast_message(state: &Arc<Mutex<ClientState>>, data: &[u8]) {
        let Ok(message) = serde_json::from_slice::<ClientBroadcastMessage>(data) else {
            return;
        };

        let mut state = state.lock().await;

        match message {
            ClientBroadcastMessage::StateUpdate(system_state) => {
                state.system_state = Some(system_state);
            }
            ClientBroadcastMessage::SemanticOpUpdate(update) => {
                if let Some(idx) = state
                    .operations
                    .iter()
                    .position(|o| o.operation_id == update.operation_id)
                {
                    state.operations[idx] = update;
                } else {
                    state.operations.push(update);
                }
            }
            ClientBroadcastMessage::ChainExecutionUpdate(exec) => {
                if let Some(idx) = state
                    .chain_executions
                    .iter()
                    .position(|e| e.execution_id == exec.execution_id)
                {
                    state.chain_executions[idx] = exec;
                } else {
                    state.chain_executions.push(exec);
                }
            }

            //
            // Live intercept streams: bounded try_send, drop-when-full with
            // rate-limited diagnostics (see intercept_live).
            //
            ClientBroadcastMessage::InterceptedTrafficBatch {
                entries,
                generation,
                service_instance_id,
            } => {
                use std::sync::atomic::Ordering;
                let epoch = state.clear_epoch.load(Ordering::Acquire);
                //
                // Data plane never rebinds instance identity.
                //
                let accept = common::clear_epoch::accept_live_batch(
                    state.service_instance_id.as_deref(),
                    epoch,
                    &service_instance_id,
                    generation,
                );
                if accept {
                    if let Some(ref tx) = state.intercept_entries_tx {
                        match try_push_bounded(
                            tx,
                            (service_instance_id, generation, entries),
                            &state.intercept_entries_drops,
                        ) {
                            LivePushResult::Sent => {}
                            LivePushResult::Dropped {
                                drop_count,
                                should_log,
                            } => {
                                if should_log {
                                    common::log_warn!(
                                        "Dropped live intercept traffic batch (channel full, capacity {}, total drops {})",
                                        INTERCEPT_LIVE_CAPACITY,
                                        drop_count
                                    );
                                }
                            }
                            LivePushResult::Closed => {
                                state.intercept_entries_tx = None;
                            }
                        }
                    }
                }
            }
            ClientBroadcastMessage::TrafficMatchBatch {
                matches,
                generation,
                service_instance_id,
            } => {
                use std::sync::atomic::Ordering;
                let epoch = state.clear_epoch.load(Ordering::Acquire);
                let accept = common::clear_epoch::accept_live_batch(
                    state.service_instance_id.as_deref(),
                    epoch,
                    &service_instance_id,
                    generation,
                );
                if accept {
                    if let Some(ref tx) = state.intercept_matches_tx {
                        match try_push_bounded(
                            tx,
                            (service_instance_id, generation, matches),
                            &state.intercept_matches_drops,
                        ) {
                            LivePushResult::Sent => {}
                            LivePushResult::Dropped {
                                drop_count,
                                should_log,
                            } => {
                                if should_log {
                                    common::log_warn!(
                                        "Dropped live intercept match batch (channel full, capacity {}, total drops {})",
                                        INTERCEPT_LIVE_CAPACITY,
                                        drop_count
                                    );
                                }
                            }
                            LivePushResult::Closed => {
                                state.intercept_matches_tx = None;
                            }
                        }
                    }
                }
            }
            ClientBroadcastMessage::InterceptStatusUpdate(status) => {
                if let Some(ref tx) = state.intercept_status_tx {
                    let _ = tx.send(Some(status));
                }
            }
            ClientBroadcastMessage::ServiceOnline {
                service_instance_id,
            } => {
                //
                // Control plane: serialized re-register with announced instance.
                // Observe outcome (do not drop the response future).
                //
                let expected = if service_instance_id.is_empty() {
                    None
                } else {
                    Some(service_instance_id.clone())
                };
                if let Some(ref tx) = state.register_cmd_tx {
                    let (resp_tx, resp_rx) = oneshot::channel();
                    if tx
                        .send(RegisterCmd::Register {
                            expected_instance: expected,
                            resp: resp_tx,
                        })
                        .is_err()
                    {
                        common::log_warn!("ServiceOnline: registration worker gone");
                    } else {
                        let announced = service_instance_id;
                        tokio::spawn(async move {
                            match resp_rx.await {
                                Ok(Ok(())) => {
                                    common::log_info!(
                                        "ServiceOnline re-registration complete (instance={})",
                                        announced
                                    );
                                }
                                Ok(Err(e)) => {
                                    common::log_warn!(
                                        "ServiceOnline re-registration failed (instance={}): {}",
                                        announced,
                                        e
                                    );
                                }
                                Err(_) => {
                                    common::log_warn!(
                                        "ServiceOnline re-registration channel closed (instance={})",
                                        announced
                                    );
                                }
                            }
                        });
                    }
                } else {
                    common::log_warn!(
                        "ServiceOnline received before registration worker was ready"
                    );
                }
            }

            _ => {}
        }
    }

    async fn register(&self) -> Result<()> {
        let tx = self
            .register_cmd_tx
            .as_ref()
            .ok_or_else(|| anyhow!("registration worker not started"))?
            .clone();
        let (resp_tx, resp_rx) = oneshot::channel();
        tx.send(RegisterCmd::Register {
            expected_instance: None,
            resp: resp_tx,
        })
        .map_err(|_| anyhow!("registration worker closed"))?;
        match tokio::time::timeout(self.timeout, resp_rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(e))) => Err(anyhow!("registration failed: {}", e)),
            Ok(Err(_)) => Err(anyhow!("registration response channel closed")),
            Err(_) => Err(anyhow!(
                "Timeout after {}s waiting for registration",
                self.timeout.as_secs()
            )),
        }
    }

    pub async fn disconnect(self) {
        if let Some(tx) = &self.register_cmd_tx {
            let _ = tx.send(RegisterCmd::Shutdown);
        }
        if let Some(handle) = self.register_worker {
            let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
        }
        {
            let mut s = self.state.lock().await;
            s.register_cmd_tx = None;
            s.pending_registration = None;
        }
        if let Some(handle) = self.consumer_handle {
            handle.abort();
        }
    }

    async fn publish_signal(&self, message: ClientSignalMessage) -> Result<()> {
        publish_json(&self.channel, CLIENT_SIGNAL_QUEUE, &message).await?;
        Ok(())
    }

    //
    // Generic request/response over the signal queue: store a oneshot sender
    // in the pending slot, publish the signal, and await the response with
    // the client timeout. The slot is cleared on publish failure or timeout
    // so a late response can't fire into a stale sender.
    //

    async fn request<T>(
        &self,
        op_name: &str,
        slot: impl Fn(&mut ClientState) -> &mut Option<oneshot::Sender<T>>,
        message: ClientSignalMessage,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            *slot(&mut state) = Some(tx);
        }

        if let Err(e) = self.publish_signal(message).await {
            *slot(&mut *self.state.lock().await) = None;
            return Err(e);
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => Err(anyhow!("{} response channel closed", op_name)),
            Err(_) => {
                *slot(&mut *self.state.lock().await) = None;
                Err(anyhow!(
                    "Timeout after {}s waiting for {} response",
                    self.timeout.as_secs(),
                    op_name
                ))
            }
        }
    }

    pub async fn get_state(&self) -> Option<SystemState> {
        self.state.lock().await.system_state.clone()
    }

    //
    // ACP methods.
    //

    pub fn subscribe_acp_events(&self) -> tokio::sync::mpsc::UnboundedReceiver<String> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.acp_event_tx = Some(tx);
        });
        rx
    }

    pub async fn send_acp_message(&self, json_rpc: String) -> Result<()> {
        let message = ClientSignalMessage::AcpMessage {
            client_id: self.client_id.clone(),
            json_rpc,
        };
        self.publish_signal(message).await
    }

    //
    // Service config methods.
    //

    pub async fn get_config(&self, keys: Vec<String>) -> Result<HashMap<String, String>> {
        let message = ClientSignalMessage::ServiceConfigGet {
            client_id: self.client_id.clone(),
            keys,
        };
        self.request("config", |s| &mut s.pending_config, message)
            .await
    }

    pub async fn set_config(&self, values: HashMap<String, String>) -> Result<()> {
        let message = ClientSignalMessage::ServiceConfigSet {
            client_id: self.client_id.clone(),
            values,
        };
        self.request("config save", |s| &mut s.pending_config_save, message)
            .await?
            .map_err(|message| anyhow!(message))
    }

    //
    // Operation methods.
    //

    //
    // Send an ACP JSON-RPC request to the given node and await its
    // response. The target node id is encoded as
    // `params._meta.praxis.nodeId` so the service routes the frame.
    //

    pub async fn acp_request(&self, node_id: &str, method: &str, params: Value) -> Result<Value> {
        self.do_acp_request(node_id, method, params, false)
            .await
            .map(|(v, _)| v)
    }

    //
    // Same as `acp_request` but additionally buffers any streamed
    // `agent_message_chunk` text that arrives while the request is in
    // flight, returning it alongside the response result.
    //

    pub async fn acp_request_collecting_text(
        &self,
        node_id: &str,
        method: &str,
        params: Value,
    ) -> Result<(Value, String)> {
        self.do_acp_request(node_id, method, params, true).await
    }

    //
    // Fire an ACP JSON-RPC notification (no id, no response). Used for
    // e.g. session/cancel.
    //

    pub async fn acp_notification(&self, node_id: &str, method: &str, params: Value) -> Result<()> {
        let frame = build_notification_frame(node_id, method, params);
        self.publish_signal(ClientSignalMessage::AcpMessage {
            client_id: self.client_id.clone(),
            json_rpc: serde_json::to_string(&frame)?,
        })
        .await
    }

    async fn do_acp_request(
        &self,
        node_id: &str,
        method: &str,
        params: Value,
        collect_text: bool,
    ) -> Result<(Value, String)> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        let session_id = params
            .get("sessionId")
            .and_then(|v| v.as_str())
            .map(String::from);

        {
            let mut state = self.state.lock().await;
            state.pending_acp.insert(
                request_id.clone(),
                PendingAcp {
                    response_tx: Some(tx),
                    text_buf: if collect_text {
                        Some(String::new())
                    } else {
                        None
                    },
                    session_id,
                },
            );
        }

        let frame = build_request_frame(&request_id, node_id, method, params);
        if let Err(e) = self
            .publish_signal(ClientSignalMessage::AcpMessage {
                client_id: self.client_id.clone(),
                json_rpc: serde_json::to_string(&frame)?,
            })
            .await
        {
            self.state.lock().await.pending_acp.remove(&request_id);
            return Err(e);
        }

        let outcome = tokio::time::timeout(self.timeout, rx).await;

        //
        // Always drop the PendingAcp entry before producing a result so
        // error paths (JSON-RPC error, dropped oneshot, timeout) don't leak
        // the entry — handle_acp_frame would otherwise keep appending
        // streamed chunks into its text_buf forever.
        //

        let text_buf = self
            .state
            .lock()
            .await
            .pending_acp
            .remove(&request_id)
            .and_then(|p| p.text_buf)
            .unwrap_or_default();

        let result = match outcome {
            Ok(Ok(Ok(value))) => value,
            Ok(Ok(Err(message))) => return Err(anyhow!(message)),
            Ok(Err(_)) => return Err(anyhow!("ACP response channel closed")),
            Err(_) => {
                return Err(anyhow!(
                    "Timeout waiting for ACP response to {} after {}s",
                    method,
                    self.timeout.as_secs()
                ));
            }
        };

        Ok((result, text_buf))
    }

    fn handle_acp_frame(state: &mut ClientState, json_rpc: &str) {
        let msg: Value = match serde_json::from_str(json_rpc) {
            Ok(v) => v,
            Err(_) => return,
        };

        let has_method = msg.get("method").and_then(|m| m.as_str()).is_some();
        let id_str = msg.get("id").map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        });

        if !has_method {
            let Some(request_id) = id_str else { return };
            //
            // Take only the response_tx — leave the PendingAcp entry (with its
            // text_buf) in place so do_acp_request can collect the buffered
            // chunk text after awaiting the response. do_acp_request removes
            // the entry once it's read the text.
            //
            let Some(pending) = state.pending_acp.get_mut(&request_id) else {
                return;
            };
            let Some(tx) = pending.response_tx.take() else {
                return;
            };

            if let Some(err) = msg.get("error") {
                let message = err
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ACP error")
                    .to_string();
                let _ = tx.send(Err(message));
            } else {
                let result = msg.get("result").cloned().unwrap_or(Value::Null);
                let _ = tx.send(Ok(result));
            }
            return;
        }

        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        if method != "session/update" {
            return;
        }
        let params = match msg.get("params") {
            Some(p) => p,
            None => return,
        };
        let session_id = match params.get("sessionId").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return,
        };
        let update = match params.get("update") {
            Some(u) => u,
            None => return,
        };
        if update.get("sessionUpdate").and_then(|v| v.as_str()) != Some("agent_message_chunk") {
            return;
        }
        let Some(text) = update
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(|v| v.as_str())
        else {
            return;
        };

        for pending in state.pending_acp.values_mut() {
            if let (Some(buf), Some(sid)) = (&mut pending.text_buf, &pending.session_id)
                && sid == session_id
            {
                buf.push_str(text);
            }
        }
    }

    pub async fn request_recon(&self, node_id: &str, agent_short_name: &str) {
        let message = ClientSignalMessage::ReconGet {
            client_id: self.client_id.clone(),
            node_id: node_id.to_string(),
            agent_short_name: agent_short_name.to_string(),
        };
        let _ = self.publish_signal(message).await;
    }

    pub async fn get_cached_project_paths(&self) -> Vec<String> {
        self.state.lock().await.cached_project_paths.clone()
    }

    pub async fn get_cached_recon(
        &self,
        node_id: &str,
        agent_short_name: &str,
    ) -> Option<common::ReconResult> {
        self.state
            .lock()
            .await
            .recon_cache
            .get(&(node_id.to_string(), agent_short_name.to_string()))
            .cloned()
    }

    //
    // Node management.
    //

    pub async fn reset_node(&self, node_id: &str) -> Result<()> {
        let message = ClientSignalMessage::ResetNode {
            node_id: node_id.to_string(),
        };
        self.publish_signal(message).await
    }

    pub async fn remove_node(&self, node_id: &str) -> Result<()> {
        let message = ClientSignalMessage::RemoveNode {
            node_id: node_id.to_string(),
        };
        self.publish_signal(message).await
    }

    pub async fn add_remote_node(
        &self,
        kind: String,
        url: String,
        token: Option<String>,
    ) -> Result<()> {
        let message = ClientSignalMessage::AddRemoteNode { kind, url, token };
        self.publish_signal(message).await
    }

    //
    // Terminal methods.
    //

    //
    // Terminal create needs a response (the terminal_id). The terminal
    // surface still uses the legacy Command dispatch path — it has no ACP
    // counterpart — so we keep a narrow awaitable wrapper that correlates
    // by command_id via a pending-creates map populated by
    // handle_direct_message.
    //

    pub async fn create_terminal(&self, node_id: &str) -> Result<String> {
        let command_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel::<Result<String, String>>();
        {
            let mut state = self.state.lock().await;
            state
                .pending_terminal_creates
                .insert(command_id.clone(), tx);
        }

        let publish = common::publish_terminal_command_with_id(
            &self.channel,
            &self.client_id,
            node_id,
            &command_id,
            common::TerminalCommand::Create,
        )
        .await;
        if let Err(e) = publish {
            self.state
                .lock()
                .await
                .pending_terminal_creates
                .remove(&command_id);
            return Err(e);
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(Ok(terminal_id))) => Ok(terminal_id),
            Ok(Ok(Err(msg))) => Err(anyhow!(msg)),
            Ok(Err(_)) => Err(anyhow!("Terminal create channel closed")),
            Err(_) => {
                self.state
                    .lock()
                    .await
                    .pending_terminal_creates
                    .remove(&command_id);
                Err(anyhow!("Timeout waiting for terminal create"))
            }
        }
    }

    pub async fn send_terminal_input(&self, node_id: &str, data: Vec<u8>) -> Result<()> {
        publish_terminal_command(
            &self.channel,
            &self.client_id,
            node_id,
            common::TerminalCommand::Write { data },
        )
        .await
    }

    pub async fn send_terminal_resize(&self, node_id: &str, rows: u16, cols: u16) -> Result<()> {
        publish_terminal_command(
            &self.channel,
            &self.client_id,
            node_id,
            common::TerminalCommand::Resize { rows, cols },
        )
        .await
    }

    pub async fn send_terminal_close(&self, node_id: &str) -> Result<()> {
        publish_terminal_command(
            &self.channel,
            &self.client_id,
            node_id,
            common::TerminalCommand::Close,
        )
        .await
    }

    pub fn subscribe_terminal_output(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<TerminalOutput> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.terminal_output_tx = Some(tx);
        });
        rx
    }

    pub fn subscribe_doc_helper(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<crate::event::DocHelperEvent> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.doc_helper_tx = Some(tx);
        });
        rx
    }

    //
    // Submit a documentation-helper prompt. Responses arrive asynchronously
    // via the doc-helper subscription, correlated by `request_id`.
    //
    pub async fn send_doc_helper_prompt(
        &self,
        request_id: String,
        prompt: String,
        history: Vec<(String, String)>,
        context: Option<String>,
    ) -> Result<()> {
        self.publish_signal(ClientSignalMessage::DocHelperPrompt {
            client_id: self.client_id.clone(),
            request_id,
            prompt,
            history,
            context,
        })
        .await
    }

    pub async fn send_doc_helper_cancel(&self, request_id: String) -> Result<()> {
        self.publish_signal(ClientSignalMessage::DocHelperCancel {
            client_id: self.client_id.clone(),
            request_id,
        })
        .await
    }

    //
    // Awaitable variants of the list refreshes: publish the request and
    // resolve with the fresh list when the response arrives, instead of
    // requiring callers to sleep and read the cache.
    //

    pub async fn fetch_operation_definitions(&self) -> Result<Vec<OperationDefinitionInfo>> {
        let message = ClientSignalMessage::OpDefList {
            client_id: self.client_id.clone(),
        };
        self.request("op def list", |s| &mut s.pending_op_def_list, message)
            .await
    }

    pub async fn fetch_operations(&self) -> Result<Vec<SemanticOpUpdate>> {
        self.request(
            "semantic op list",
            |s| &mut s.pending_semantic_op_list,
            ClientSignalMessage::SemanticOpListRequest,
        )
        .await
    }

    pub async fn fetch_chain_definitions(&self) -> Result<Vec<ChainDefinitionInfo>> {
        let message = ClientSignalMessage::ChainDefList {
            client_id: self.client_id.clone(),
        };
        self.request("chain list", |s| &mut s.pending_chain_list, message)
            .await
    }

    pub async fn fetch_chain_executions(&self) -> Result<Vec<ChainExecutionUpdate>> {
        let message = ClientSignalMessage::ChainExecutionList {
            client_id: self.client_id.clone(),
        };
        self.request(
            "chain execution list",
            |s| &mut s.pending_chain_execution_list,
            message,
        )
        .await
    }

    pub async fn fetch_chain_triggers(&self) -> Result<Vec<ChainTriggerInfo>> {
        let message = ClientSignalMessage::ChainTriggerList {
            client_id: self.client_id.clone(),
            chain_id: None,
        };
        self.request(
            "chain trigger list",
            |s| &mut s.pending_chain_trigger_list,
            message,
        )
        .await
    }

    pub async fn get_operation_definitions(&self) -> Vec<OperationDefinitionInfo> {
        self.state.lock().await.operation_definitions.clone()
    }

    pub async fn request_semantic_op_list(&self) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpListRequest;
        self.publish_signal(message).await
    }

    pub async fn get_operations(&self) -> Vec<SemanticOpUpdate> {
        self.state.lock().await.operations.clone()
    }

    pub async fn run_semantic_op(
        &self,
        node_id: String,
        agent_short_name: String,
        operation_name: String,
        working_dir: Option<String>,
    ) -> Result<String> {
        let message = ClientSignalMessage::SemanticOpRun {
            client_id: self.client_id.clone(),
            node_id,
            agent_short_name,
            operation_name,
            request_id: uuid::Uuid::new_v4().to_string(),
            working_dir,
        };
        self.request(
            "semantic op queued",
            |s| &mut s.pending_semantic_op,
            message,
        )
        .await
    }

    pub async fn cancel_semantic_op(&self, operation_id: String) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpCancel { operation_id };
        self.publish_signal(message).await
    }

    pub async fn add_op_def(&self, content: String) -> Result<()> {
        let message = ClientSignalMessage::OpDefAdd {
            client_id: self.client_id.clone(),
            content,
        };
        self.publish_signal(message).await
    }

    pub async fn delete_op_def(&self, full_name: String) -> Result<()> {
        let message = ClientSignalMessage::OpDefDelete {
            client_id: self.client_id.clone(),
            full_name,
        };
        self.publish_signal(message).await
    }

    //
    // Chain methods.
    //

    pub async fn get_chain_definitions(&self) -> Vec<ChainDefinitionInfo> {
        self.state.lock().await.chain_definitions.clone()
    }

    pub async fn request_chain_execution_list(&self) -> Result<()> {
        let message = ClientSignalMessage::ChainExecutionList {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_chain_executions(&self) -> Vec<ChainExecutionUpdate> {
        self.state.lock().await.chain_executions.clone()
    }

    pub async fn run_chain(
        &self,
        chain_id: String,
        node_id: String,
        agent_short_name: String,
        working_dir: Option<String>,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainRun {
            client_id: self.client_id.clone(),
            chain_id,
            node_id,
            agent_short_name,
            working_dir,
            target_spec: None,
        };
        self.publish_signal(message).await
    }

    pub async fn cancel_chain(&self, execution_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainCancel {
            client_id: self.client_id.clone(),
            execution_id,
        };
        self.publish_signal(message).await
    }

    pub async fn remove_semantic_op(&self, operation_id: String) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpRemove { operation_id };
        self.publish_signal(message).await
    }

    pub async fn request_chain_def(&self, chain_id: &str) -> Result<()> {
        let message = ClientSignalMessage::ChainGet {
            client_id: self.client_id.clone(),
            chain_id: chain_id.to_string(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_current_chain(&self) -> Option<ChainDefinitionFull> {
        self.state.lock().await.current_chain.clone()
    }

    pub async fn add_chain_def(&self, definition: ChainDefinitionInput) -> Result<()> {
        let message = ClientSignalMessage::ChainCreate {
            client_id: self.client_id.clone(),
            definition,
        };
        self.publish_signal(message).await
    }

    pub async fn update_chain_def(
        &self,
        chain_id: String,
        definition: ChainDefinitionInput,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainUpdate {
            client_id: self.client_id.clone(),
            chain_id,
            definition,
        };
        self.publish_signal(message).await
    }

    pub async fn delete_chain_def(&self, chain_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainDelete {
            client_id: self.client_id.clone(),
            chain_id,
        };
        self.publish_signal(message).await
    }

    pub async fn clear_all_ops(&self) -> Result<()> {
        self.publish_signal(ClientSignalMessage::SemanticOpClear)
            .await
    }

    pub async fn clear_all_chains(&self) -> Result<()> {
        self.publish_signal(ClientSignalMessage::ChainExecutionClear)
            .await
    }

    pub async fn remove_chain_execution(&self, execution_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainExecutionRemove { execution_id };
        self.publish_signal(message).await
    }

    //
    // Chain triggers.
    //

    pub async fn get_chain_triggers(&self) -> Vec<ChainTriggerInfo> {
        self.state.lock().await.chain_triggers.clone()
    }

    pub async fn create_chain_trigger(
        &self,
        chain_id: String,
        trigger_config: TriggerConfig,
        target_spec: TargetSpec,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerCreate {
            client_id: self.client_id.clone(),
            chain_id,
            trigger_config,
            target_spec,
        };
        self.publish_signal(message).await
    }

    pub async fn update_chain_trigger(
        &self,
        trigger_id: String,
        enabled: Option<bool>,
        trigger_config: Option<TriggerConfig>,
        target_spec: Option<TargetSpec>,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerUpdate {
            client_id: self.client_id.clone(),
            trigger_id,
            enabled,
            trigger_config,
            target_spec,
        };
        self.publish_signal(message).await
    }

    pub async fn delete_chain_trigger(&self, trigger_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerDelete {
            client_id: self.client_id.clone(),
            trigger_id,
        };
        self.publish_signal(message).await
    }

    //
    // Lua agent script methods.
    //

    pub async fn request_lua_agent_scripts(&self) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptList {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_lua_agent_scripts(&self) -> Vec<LuaAgentScriptInfo> {
        self.state.lock().await.lua_agent_scripts.clone()
    }

    pub async fn add_lua_agent_script(&self, name: String, script: String) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptAdd {
            client_id: self.client_id.clone(),
            name,
            script,
        };
        self.publish_signal(message).await
    }

    pub async fn update_lua_agent_script(
        &self,
        script_id: String,
        name: String,
        script: String,
    ) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptUpdate {
            client_id: self.client_id.clone(),
            script_id,
            name,
            script,
        };
        self.publish_signal(message).await
    }

    pub async fn delete_lua_agent_script(&self, script_id: String) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptDelete {
            client_id: self.client_id.clone(),
            script_id,
        };
        self.publish_signal(message).await
    }

    pub async fn toggle_lua_agent_script_disabled(
        &self,
        script_id: String,
        disabled: bool,
    ) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptToggleDisabled {
            client_id: self.client_id.clone(),
            script_id,
            disabled,
        };
        self.publish_signal(message).await
    }

    pub async fn reset_lua_agent_script_defaults(&self) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptResetDefaults {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    //
    // Intercept targets virtual file.
    //

    pub async fn request_intercept_targets(&self) -> Result<()> {
        let message = ClientSignalMessage::InterceptTargetsGet {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_intercept_targets(&self) -> Vec<common::InterceptTargetConfig> {
        self.state.lock().await.intercept_targets_parsed.clone()
    }

    pub async fn get_intercept_targets_text(&self) -> String {
        self.state.lock().await.intercept_targets_text.clone()
    }

    pub async fn get_intercept_targets_error(&self) -> Option<String> {
        self.state.lock().await.intercept_targets_error.clone()
    }

    pub async fn set_intercept_targets(&self, text: String) -> Result<()> {
        let message = ClientSignalMessage::InterceptTargetsSet {
            client_id: self.client_id.clone(),
            text,
        };
        self.publish_signal(message).await
    }

    pub async fn reset_intercept_targets_defaults(&self) -> Result<()> {
        let message = ClientSignalMessage::InterceptTargetsResetDefaults {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    //
    // Intercept traffic: live streams.
    //

    pub fn subscribe_intercept_entries(
        &self,
    ) -> mpsc::Receiver<(String, u64, Vec<InterceptedTrafficEntry>)> {
        let (tx, rx) = mpsc::channel(INTERCEPT_LIVE_CAPACITY);
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.intercept_entries_tx = Some(tx);
        });
        rx
    }

    pub fn subscribe_intercept_matches(
        &self,
    ) -> mpsc::Receiver<(String, u64, Vec<TrafficMatchWithDetails>)> {
        let (tx, rx) = mpsc::channel(INTERCEPT_LIVE_CAPACITY);
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.intercept_matches_tx = Some(tx);
        });
        rx
    }

    /// Last-value-wins status channel (not capacity-shared with traffic).
    pub fn subscribe_intercept_status(
        &self,
    ) -> tokio::sync::watch::Receiver<Option<InterceptStatus>> {
        let (tx, rx) = tokio::sync::watch::channel(None);
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.intercept_status_tx = Some(tx);
        });
        rx
    }

    pub fn subscribe_service_instance(
        &self,
    ) -> tokio::sync::watch::Receiver<Option<String>> {
        //
        // Seed with the already-known registration instance so the TUI does
        // not start at None after connect (v4 review).
        //
        let (tx, rx) = tokio::sync::watch::channel(None);
        let state = self.state.clone();
        tokio::spawn(async move {
            let mut s = state.lock().await;
            let seed = s.service_instance_id.clone();
            if seed.is_some() {
                let _ = tx.send(seed);
            }
            s.service_instance_tx = Some(tx);
        });
        rx
    }

    pub fn subscribe_clear_boundary(
        &self,
    ) -> tokio::sync::watch::Receiver<Option<(String, u64)>> {
        let (tx, rx) = tokio::sync::watch::channel(None);
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.clear_boundary_tx = Some(tx);
        });
        rx
    }

    pub async fn clear_epoch(&self) -> u64 {
        use std::sync::atomic::Ordering;
        self.state.lock().await.clear_epoch.load(Ordering::Acquire)
    }

    pub async fn service_instance_id(&self) -> Option<String> {
        self.state.lock().await.service_instance_id.clone()
    }

    //
    // Intercept traffic: request/response helpers.
    //

    async fn traffic_request<T>(
        &self,
        op_name: &str,
        map_slot: impl Fn(
            &mut ClientState,
        ) -> &mut HashMap<String, oneshot::Sender<Result<T, String>>>,
        build: impl FnOnce(String) -> ClientSignalMessage,
    ) -> Result<T> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            map_slot(&mut state).insert(request_id.clone(), tx);
        }
        if let Err(e) = self.publish_signal(build(request_id.clone())).await {
            map_slot(&mut *self.state.lock().await).remove(&request_id);
            return Err(e);
        }
        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) => Err(anyhow!(error)),
            Ok(Err(_)) => Err(anyhow!("{} response channel closed", op_name)),
            Err(_) => {
                map_slot(&mut *self.state.lock().await).remove(&request_id);
                Err(anyhow!(
                    "Timeout after {}s waiting for {} response",
                    self.timeout.as_secs(),
                    op_name
                ))
            }
        }
    }

    pub async fn request_traffic_log(
        &self,
        filters: TrafficLogFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        let client_id = self.client_id.clone();
        self.traffic_request(
            "traffic log",
            |s| &mut s.pending_traffic_log,
            |request_id| ClientSignalMessage::TrafficLogRequest {
                client_id,
                request_id,
                filters,
            },
        )
        .await
    }

    pub async fn request_traffic_search(
        &self,
        filters: TrafficSearchFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        let client_id = self.client_id.clone();
        self.traffic_request(
            "traffic search",
            |s| &mut s.pending_traffic_search,
            |request_id| ClientSignalMessage::TrafficSearchRequest {
                client_id,
                request_id,
                filters,
            },
        )
        .await
    }

    pub async fn request_traffic_matches(
        &self,
        rule_id: Option<i64>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<TrafficMatchWithDetails>, usize)> {
        let client_id = self.client_id.clone();
        self.traffic_request(
            "traffic matches",
            |s| &mut s.pending_traffic_matches,
            |request_id| ClientSignalMessage::TrafficMatchesRequest {
                client_id,
                request_id,
                rule_id,
                limit,
                offset,
            },
        )
        .await
    }

    ///
    /// Returns `(service_instance_id, deleted_count, generation)` for the
    /// validated clear response so the TUI applies only that scope (never
    /// re-reads a later-rebound client identity).
    ///
    pub async fn clear_all_traffic(&self) -> Result<(String, usize, u64)> {
        let client_id = self.client_id.clone();
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            let expected = state.service_instance_id.clone().unwrap_or_default();
            state.pending_traffic_clear.insert(
                request_id.clone(),
                PendingClear {
                    expected_instance: expected,
                    resp: tx,
                },
            );
        }
        let message = ClientSignalMessage::TrafficClear {
            client_id,
            request_id: request_id.clone(),
        };
        if let Err(e) = self.publish_signal(message).await {
            self.state
                .lock()
                .await
                .pending_traffic_clear
                .remove(&request_id);
            return Err(e);
        }
        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) => Err(anyhow!(error)),
            Ok(Err(_)) => Err(anyhow!("traffic clear response channel closed")),
            Err(_) => {
                self.state
                    .lock()
                    .await
                    .pending_traffic_clear
                    .remove(&request_id);
                Err(anyhow!(
                    "Timeout after {}s waiting for traffic clear response",
                    self.timeout.as_secs()
                ))
            }
        }
    }

    pub async fn fetch_traffic_entry(&self, id: i64) -> Result<Option<InterceptedTrafficEntry>> {
        let client_id = self.client_id.clone();
        self.traffic_request(
            "traffic get",
            |s| &mut s.pending_traffic_get,
            |request_id| ClientSignalMessage::TrafficGetRequest {
                client_id,
                request_id,
                id,
            },
        )
        .await
    }

    //
    // Intercept rules.
    //

    pub async fn list_intercept_rules(&self) -> Result<Vec<InterceptRule>> {
        let message = ClientSignalMessage::InterceptRuleList {
            client_id: self.client_id.clone(),
        };
        self.request("rules list", |s| &mut s.pending_rules_list, message)
            .await
    }

    pub async fn create_intercept_rule(
        &self,
        name: String,
        regex_pattern: String,
        target_direction: TargetDirection,
        scope: RuleScope,
        summarization_prompt: Option<String>,
    ) -> Result<InterceptRule> {
        let message = ClientSignalMessage::InterceptRuleCreate {
            client_id: self.client_id.clone(),
            name,
            regex_pattern,
            target_direction,
            scope,
            summarization_prompt,
        };
        match self
            .request("rule create", |s| &mut s.pending_rule_op, message)
            .await?
        {
            RuleOpOutcome::Created(rule) => Ok(rule),
            RuleOpOutcome::Error(msg) => Err(anyhow!(msg)),
            other => Err(anyhow!("Unexpected rule op outcome: {:?}", other)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_intercept_rule(
        &self,
        id: i64,
        name: Option<String>,
        regex_pattern: Option<String>,
        target_direction: Option<TargetDirection>,
        scope: Option<RuleScope>,
        enabled: Option<bool>,
        summarization_prompt: Option<Option<String>>,
    ) -> Result<InterceptRule> {
        let message = ClientSignalMessage::InterceptRuleUpdate {
            client_id: self.client_id.clone(),
            id,
            name,
            regex_pattern,
            target_direction,
            scope,
            enabled,
            summarization_prompt,
        };
        match self
            .request("rule update", |s| &mut s.pending_rule_op, message)
            .await?
        {
            RuleOpOutcome::Updated(rule) => Ok(rule),
            RuleOpOutcome::Error(msg) => Err(anyhow!(msg)),
            other => Err(anyhow!("Unexpected rule op outcome: {:?}", other)),
        }
    }

    pub async fn delete_intercept_rule(&self, id: i64) -> Result<bool> {
        let message = ClientSignalMessage::InterceptRuleDelete {
            client_id: self.client_id.clone(),
            id,
        };
        match self
            .request("rule delete", |s| &mut s.pending_rule_op, message)
            .await?
        {
            RuleOpOutcome::Deleted { success, .. } => Ok(success),
            RuleOpOutcome::Error(msg) => Err(anyhow!(msg)),
            other => Err(anyhow!("Unexpected rule op outcome: {:?}", other)),
        }
    }

    //
    // Intercept enable/disable.
    //

    pub async fn enable_intercept(
        &self,
        node_id: String,
        method: Option<InterceptMethod>,
    ) -> Result<()> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let message = ClientSignalMessage::InterceptEnable {
            client_id: self.client_id.clone(),
            request_id: request_id.clone(),
            node_id,
            method,
        };
        self.request_intercept_toggle("intercept enable", request_id, message)
            .await
    }

    pub async fn disable_intercept(&self, node_id: String) -> Result<()> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let message = ClientSignalMessage::InterceptDisable {
            client_id: self.client_id.clone(),
            request_id: request_id.clone(),
            node_id,
        };
        self.request_intercept_toggle("intercept disable", request_id, message)
            .await
    }

    async fn request_intercept_toggle(
        &self,
        op_name: &str,
        request_id: String,
        message: ClientSignalMessage,
    ) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.state
            .lock()
            .await
            .pending_intercept_toggles
            .insert(request_id.clone(), tx);

        if let Err(e) = self.publish_signal(message).await {
            self.state
                .lock()
                .await
                .pending_intercept_toggles
                .remove(&request_id);
            return Err(e);
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(message))) => Err(anyhow!(message)),
            Ok(Err(_)) => Err(anyhow!("{} response channel closed", op_name)),
            Err(_) => {
                self.state
                    .lock()
                    .await
                    .pending_intercept_toggles
                    .remove(&request_id);
                Err(anyhow!(
                    "Timeout after {}s waiting for {} response",
                    self.timeout.as_secs(),
                    op_name
                ))
            }
        }
    }

    //
    // LogQuery: run a KQL query on the service and wait for the result.
    // The Ok side is a materialized result set; the Err side carries either
    // the service-provided error message or a transport failure.
    //

    pub async fn run_log_query(&self, query: String) -> Result<LogQueryResults, String> {
        let message = ClientSignalMessage::LogQuery {
            client_id: self.client_id.clone(),
            query,
        };
        self.request("log query", |s| &mut s.pending_log_query, message)
            .await
            .map_err(|e| e.to_string())?
    }
}
