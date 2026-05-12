use lapin::Channel;
use std::sync::Arc;
use tokio::sync::{Notify, RwLock as TokioRwLock};

use crate::acp_node_proxy::AcpNodeProxy;
use crate::config::ServiceConfig;
use crate::database::Database;
use crate::semantic_ops::ChainExecutor;
use crate::semantic_ops::chain_execution::resolve_targets;
use crate::state::NodeRegistry;
use crate::tools::ToolkitManager;

/// Debounce window for intercept-match triggers (seconds)
const INTERCEPT_DEBOUNCE_SECS: i64 = 60;

/// TriggerEngine handles automated chain firing based on configured triggers.
pub struct TriggerEngine {
    database: Arc<Database>,
    chain_executor: Arc<ChainExecutor>,
    node_registry: Arc<NodeRegistry>,
    service_config: Arc<TokioRwLock<ServiceConfig>>,
    acp_node_proxy: Arc<AcpNodeProxy>,
    semantic_ops_channel: Channel,
    broadcast_channel: Channel,
    toolkit_manager: Arc<ToolkitManager>,
    refresh_notify: Notify,
}

impl TriggerEngine {
    pub fn new(
        database: Arc<Database>,
        chain_executor: Arc<ChainExecutor>,
        node_registry: Arc<NodeRegistry>,
        service_config: Arc<TokioRwLock<ServiceConfig>>,
        acp_node_proxy: Arc<AcpNodeProxy>,
        semantic_ops_channel: Channel,
        broadcast_channel: Channel,
        toolkit_manager: Arc<ToolkitManager>,
    ) -> Self {
        Self {
            database,
            chain_executor,
            node_registry,
            service_config,
            acp_node_proxy,
            semantic_ops_channel,
            broadcast_channel,
            toolkit_manager,
            refresh_notify: Notify::new(),
        }
    }

    /// Signal the scheduler to re-check triggers (e.g. after CRUD operations)
    pub async fn refresh(&self) {
        self.refresh_notify.notify_one();
    }

    /// Start the scheduler loop (runs every 30 seconds or on refresh signal)
    pub fn start_scheduler(self: &Arc<Self>) {
        let engine = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = interval.tick() => {}
                    _ = engine.refresh_notify.notified() => {}
                }
                engine.check_scheduled_triggers().await;
            }
        });
    }

    /// Check and fire any scheduled triggers that are due
    async fn check_scheduled_triggers(&self) {
        let due_triggers = match self.database.list_due_triggers().await {
            Ok(triggers) => triggers,
            Err(e) => {
                common::log_error!("Failed to list due triggers: {}", e);
                return;
            }
        };

        for trigger in due_triggers {
            common::log_info!(
                "Firing scheduled trigger {} for chain {}",
                common::short_id(&trigger.id),
                trigger.chain_id
            );

            let chain = match self.database.get_chain(&trigger.chain_id).await {
                Ok(Some(chain)) => chain,
                Ok(None) => {
                    common::log_warn!(
                        "Chain {} not found for trigger {}, disabling",
                        trigger.chain_id,
                        trigger.id
                    );
                    let _ = self.database.mark_trigger_fired(&trigger.id, true).await;
                    continue;
                }
                Err(e) => {
                    common::log_error!("Failed to load chain {}: {}", trigger.chain_id, e);
                    continue;
                }
            };

            let targets = resolve_targets(&trigger.target_spec, &self.node_registry, None).await;
            if targets.is_empty() {
                common::log_warn!("No targets matched for trigger {}", trigger.id);
            } else {
                let _results = self
                    .chain_executor
                    .execute_fan_out(
                        chain,
                        targets,
                        None,
                        None,
                        self.service_config.clone(),
                        self.semantic_ops_channel.clone(),
                        self.broadcast_channel.clone(),
                        self.acp_node_proxy.clone(),
                        self.database.clone(),
                        Some(self.toolkit_manager.clone()),
                    )
                    .await;
            }

            let disable = matches!(
                trigger.trigger_config,
                common::TriggerConfig::Scheduled {
                    recurring: false,
                    ..
                }
            );
            if let Err(e) = self.database.mark_trigger_fired(&trigger.id, disable).await {
                common::log_error!(
                    "Failed to update trigger {} after firing: {}",
                    trigger.id,
                    e
                );
            }
        }
    }

    /// Fire triggers that match an intercept rule.
    /// Called from dispatch/node.rs after traffic rule matches.
    pub async fn fire_intercept_match_triggers(
        &self,
        matched_rule_ids: &[i64],
        node_id: &str,
        match_context: &str,
    ) {
        let triggers = match self
            .database
            .list_enabled_triggers_by_type("InterceptMatch")
            .await
        {
            Ok(t) => t,
            Err(e) => {
                common::log_error!("Failed to list intercept triggers: {}", e);
                return;
            }
        };

        for trigger in triggers {
            let rule_id = match &trigger.trigger_config {
                common::TriggerConfig::InterceptMatch { rule_id } => *rule_id,
                _ => continue,
            };

            if !matched_rule_ids.contains(&rule_id) {
                continue;
            }

            if let Some(last_fired) = trigger.last_fired_at {
                let elapsed = chrono::Utc::now() - last_fired;
                if elapsed.num_seconds() < INTERCEPT_DEBOUNCE_SECS {
                    continue;
                }
            }

            common::log_info!(
                "Firing intercept-match trigger {} for rule {} on node {}",
                common::short_id(&trigger.id),
                rule_id,
                common::short_id(&node_id)
            );

            let chain = match self.database.get_chain(&trigger.chain_id).await {
                Ok(Some(chain)) => chain,
                _ => continue,
            };

            let targets =
                resolve_targets(&trigger.target_spec, &self.node_registry, Some(node_id)).await;

            if !targets.is_empty() {
                let _results = self
                    .chain_executor
                    .execute_fan_out(
                        chain,
                        targets,
                        Some(match_context.to_string()),
                        None,
                        self.service_config.clone(),
                        self.semantic_ops_channel.clone(),
                        self.broadcast_channel.clone(),
                        self.acp_node_proxy.clone(),
                        self.database.clone(),
                        Some(self.toolkit_manager.clone()),
                    )
                    .await;
            }

            let _ = self.database.mark_trigger_fired(&trigger.id, false).await;
        }
    }

    /// Fire triggers for new node registration.
    /// Called from handlers/node_message_handler.rs after registration.
    pub async fn fire_new_node_triggers(&self, node_id: &str) {
        let triggers = match self.database.list_enabled_triggers_by_type("NewNode").await {
            Ok(t) => t,
            Err(e) => {
                common::log_error!("Failed to list new-node triggers: {}", e);
                return;
            }
        };

        for trigger in triggers {
            common::log_info!(
                "Firing new-node trigger {} for node {}",
                common::short_id(&trigger.id),
                common::short_id(node_id)
            );

            let chain = match self.database.get_chain(&trigger.chain_id).await {
                Ok(Some(chain)) => chain,
                _ => continue,
            };

            let targets =
                resolve_targets(&trigger.target_spec, &self.node_registry, Some(node_id)).await;

            if !targets.is_empty() {
                let _results = self
                    .chain_executor
                    .execute_fan_out(
                        chain,
                        targets,
                        None,
                        None,
                        self.service_config.clone(),
                        self.semantic_ops_channel.clone(),
                        self.broadcast_channel.clone(),
                        self.acp_node_proxy.clone(),
                        self.database.clone(),
                        Some(self.toolkit_manager.clone()),
                    )
                    .await;
            }

            let _ = self.database.mark_trigger_fired(&trigger.id, false).await;
        }
    }
}
