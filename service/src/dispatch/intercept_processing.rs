use std::sync::Arc;

use chrono::Utc;
use common::{InterceptedTrafficEntry, TrafficMatch, TrafficMatchWithDetails};
use tokio::sync::{RwLock, Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::config::ServiceConfig;
use crate::database::Database;
use crate::semantic_helpers;
use crate::trigger_engine::TriggerEngine;

use super::traffic_barrier::{should_drop_stale_ingest, TrafficTableBarrier};
use super::traffic_broadcast::InterceptBroadcaster;
use crate::database::rules_snapshot::RulesSnapshot;

const QUEUE_CAPACITY: usize = 64;
const WORKER_CONCURRENCY: usize = 8;
const BACKGROUND_CONCURRENCY: usize = 8;

/// Traffic accepted into the processor queue, stamped with clear generation
/// at enqueue so a later clear drops the item instead of re-inserting it.
struct QueuedTraffic {
    entry: InterceptedTrafficEntry,
    enqueued_gen: u64,
}

struct ProcessingContext {
    database: Arc<Database>,
    service_config: Arc<RwLock<ServiceConfig>>,
    trigger_engine: Option<Arc<TriggerEngine>>,
    broadcaster: Arc<InterceptBroadcaster>,
    barrier: Arc<TrafficTableBarrier>,
    rules_snapshot: Arc<RulesSnapshot>,
    cancel: CancellationToken,
    tasks: TaskTracker,
    background_permits: Arc<Semaphore>,
}

pub struct InterceptProcessor {
    tx: mpsc::Sender<QueuedTraffic>,
    barrier: Arc<TrafficTableBarrier>,
    cancel: CancellationToken,
    tasks: TaskTracker,
}

impl InterceptProcessor {
    pub fn spawn(
        database: Arc<Database>,
        service_config: Arc<RwLock<ServiceConfig>>,
        trigger_engine: Option<Arc<TriggerEngine>>,
        broadcaster: Arc<InterceptBroadcaster>,
        barrier: Arc<TrafficTableBarrier>,
        rules_snapshot: Arc<RulesSnapshot>,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel(QUEUE_CAPACITY);
        let cancel = CancellationToken::new();
        let tasks = TaskTracker::new();
        let context = Arc::new(ProcessingContext {
            database,
            service_config,
            trigger_engine,
            broadcaster,
            barrier: barrier.clone(),
            rules_snapshot,
            cancel: cancel.clone(),
            tasks: tasks.clone(),
            background_permits: Arc::new(Semaphore::new(BACKGROUND_CONCURRENCY)),
        });
        let dispatcher_context = context.clone();
        let permits = Arc::new(Semaphore::new(WORKER_CONCURRENCY));
        let dispatcher_tasks = tasks.clone();

        tasks.spawn(async move {
            loop {
                let queued = tokio::select! {
                    biased;
                    _ = dispatcher_context.cancel.cancelled() => break,
                    queued = rx.recv() => match queued {
                        Some(queued) => queued,
                        None => break,
                    },
                };
                let permit = tokio::select! {
                    biased;
                    _ = dispatcher_context.cancel.cancelled() => break,
                    permit = permits.clone().acquire_owned() => match permit {
                        Ok(permit) => permit,
                        Err(_) => break,
                    },
                };
                let worker_context = dispatcher_context.clone();
                dispatcher_tasks.spawn(async move {
                    let _permit = permit;
                    tokio::select! {
                        biased;
                        _ = worker_context.cancel.cancelled() => {}
                        _ = process_entry(worker_context.clone(), queued) => {}
                    }
                });
            }
        });

        Self {
            tx,
            barrier,
            cancel,
            tasks,
        }
    }

    pub fn enqueue(&self, entry: InterceptedTrafficEntry) -> Result<(), String> {
        //
        // Stamp clear generation at accept time (not at process start) so
        // items waiting in the mpsc queue or worker backlog are dropped after
        // a successful clear instead of re-inserted.
        //
        let queued = QueuedTraffic {
            enqueued_gen: self.barrier.generation(),
            entry,
        };
        self.tx.try_send(queued).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => format!(
                "intercept processing queue is full (capacity {})",
                QUEUE_CAPACITY
            ),
            mpsc::error::TrySendError::Closed(_) => {
                "intercept processing queue is closed".to_string()
            }
        })
    }

    pub async fn shutdown(&self) {
        self.cancel.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

impl Drop for InterceptProcessor {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.tasks.close();
    }
}

async fn process_entry(context: Arc<ProcessingContext>, queued: QueuedTraffic) {
    let QueuedTraffic {
        mut entry,
        enqueued_gen,
    } = queued;
    common::log_info!(
        "Received intercepted traffic: node={} agent={} {} {} {} (status={})",
        common::short_id(&entry.node_id),
        entry.agent_short_name,
        entry.direction,
        entry.method.as_deref().unwrap_or("-"),
        entry.host,
        entry
            .response_status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "-".to_string())
    );

    //
    // Drop if clear advanced generation since enqueue (queued pre-clear
    // backlog). Hold the barrier only for DB insert+match; re-check
    // generation before live broadcast so clients that just cleared are not
    // re-populated from a row clear already deleted.
    //
    if should_drop_stale_ingest(enqueued_gen, context.barrier.generation()) {
        common::log_info!(
            "Dropped pre-clear intercept entry for node {} (generation advanced since enqueue)",
            common::short_id(&entry.node_id)
        );
        return;
    }

    let (traffic_id, matches, gen_at_commit) = {
        let _guard = context.barrier.read().await;
        if should_drop_stale_ingest(enqueued_gen, context.barrier.generation()) {
            common::log_info!(
                "Dropped pre-clear intercept entry for node {} (generation advanced under lock)",
                common::short_id(&entry.node_id)
            );
            return;
        }

        let traffic_id = match context.database.insert_traffic(&entry).await {
            Ok(traffic_id) => traffic_id,
            Err(error) => {
                common::log_error!("Failed to store intercepted traffic: {}", error);
                return;
            }
        };
        entry.id = Some(traffic_id);

        let matches = match context
            .database
            .check_and_insert_matches_with_snapshot(
                traffic_id,
                &entry,
                &context.rules_snapshot,
            )
            .await
        {
            Ok(matches) => matches,
            Err(error) => {
                common::log_error!("Failed to check traffic matches: {}", error);
                drop(_guard);
                maybe_prune_old_traffic(&context.database, &context.barrier).await;
                return;
            }
        };
        // matches path already uses dirty snapshot fallback inside helper
        let gen_at_commit = context.barrier.generation();
        (traffic_id, matches, gen_at_commit)
    };

    //
    // Clear may have run after we released the read lock (deleted our row and
    // bumped generation). Do not live-broadcast in that case.
    //
    if should_drop_stale_ingest(gen_at_commit, context.barrier.generation()) {
        common::log_info!(
            "Skipped live broadcast for traffic id {} (clear completed after insert)",
            traffic_id
        );
        return;
    }

    context
        .broadcaster
        .push_entry(entry.clone(), gen_at_commit);

    //
    // Trigger/summarization run only when a background permit is free
    // (try_acquire). Never await capacity here — that parked ingest workers
    // and caused capture drops under slow LLM/trigger load.
    //
    if !matches.is_empty()
        && let Some(trigger_engine) = context.trigger_engine.clone()
    {
        let matched_rule_ids = matches.iter().map(|(_, rule)| rule.id).collect::<Vec<_>>();
        let node_id = entry.node_id.clone();
        let match_context = format!(
            "Intercept match on URL: {}\nMatched rules: {}",
            entry.url,
            matches
                .iter()
                .map(|(_, rule)| rule.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let cancel = context.cancel.clone();
        let barrier = context.barrier.clone();
        let gen_at_commit = gen_at_commit;
        match context.background_permits.clone().try_acquire_owned() {
            Ok(permit) => {
                context.tasks.spawn(async move {
                    let _permit = permit;
                    //
                    // Clear may still race after this check while the trigger
                    // engine runs. Product policy: clear deletes stored rows;
                    // in-flight trigger side effects are best-effort and may
                    // still fire. We skip only when clear already advanced
                    // before we enter the engine.
                    //
                    if should_drop_stale_ingest(gen_at_commit, barrier.generation()) {
                        common::log_info!(
                            "Skipped intercept-match trigger (clear completed after insert)"
                        );
                        return;
                    }
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => {}
                        _ = trigger_engine.fire_intercept_match_triggers(
                            &matched_rule_ids,
                            &node_id,
                            &match_context,
                        ) => {}
                    }
                });
            }
            Err(_) => {
                common::log_warn!(
                    "Dropped intercept-match trigger dispatch (analysis capacity full); capture was still persisted"
                );
            }
        }
    }

    for (match_id, rule) in matches {
        if should_drop_stale_ingest(gen_at_commit, context.barrier.generation()) {
            common::log_info!(
                "Skipped live match broadcast for traffic id {} (clear completed after insert)",
                traffic_id
            );
            break;
        }
        let match_info = TrafficMatch {
            id: match_id,
            traffic_id,
            rule_id: rule.id,
            rule_name: rule.name.clone(),
            matched_at: Utc::now(),
            summary: None,
        };
        context.broadcaster.push_match(
            TrafficMatchWithDetails {
                match_info: match_info.clone(),
                traffic: entry.clone(),
            },
            gen_at_commit,
        );

        if let Some(prompt) = rule.summarization_prompt.clone() {
            let summarize_context = context.clone();
            let entry = entry.clone();
            let rule_id = rule.id;
            let rule_name = rule.name.clone();
            let gen_at_commit = gen_at_commit;
            match context.background_permits.clone().try_acquire_owned() {
                Ok(permit) => {
                    context.tasks.spawn(async move {
                        let _permit = permit;
                        let cancel = summarize_context.cancel.clone();
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => {}
                            _ = async {
                                let result = semantic_helpers::summarize_traffic(
                                    &summarize_context.service_config,
                                    &entry,
                                    &prompt,
                                ).await;
                                if result.success {
                                    if let Some(summary) = result.summary {
                                        if let Err(error) = summarize_context
                                            .database
                                            .update_match_summary(match_id, &summary)
                                            .await
                                        {
                                            common::log_error!("Failed to update match summary: {}", error);
                                        }
                                        //
                                        // Do not re-populate clients after a clear.
                                        //
                                        if should_drop_stale_ingest(
                                            gen_at_commit,
                                            summarize_context.barrier.generation(),
                                        ) {
                                            return;
                                        }
                                        summarize_context.broadcaster.push_match(
                                            TrafficMatchWithDetails {
                                                match_info: TrafficMatch {
                                                    id: match_id,
                                                    traffic_id,
                                                    rule_id,
                                                    rule_name,
                                                    matched_at: match_info.matched_at,
                                                    summary: Some(summary),
                                                },
                                                traffic: entry,
                                            },
                                            gen_at_commit,
                                        );
                                    }
                                } else if let Some(error) = result.error {
                                    common::log_warn!("Summarization failed for match {}: {}", match_id, error);
                                }
                            } => {}
                        }
                    });
                }
                Err(_) => {
                    common::log_warn!(
                        "Dropped traffic summarization for match {} (analysis capacity full); capture was still persisted",
                        match_id
                    );
                }
            }
        }
    }

    maybe_prune_old_traffic(&context.database, &context.barrier).await;
}

async fn maybe_prune_old_traffic(database: &Database, barrier: &TrafficTableBarrier) {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static LAST_PRUNE_UNIX: AtomicI64 = AtomicI64::new(0);
    const PRUNE_INTERVAL_SECS: i64 = 3600;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let last = LAST_PRUNE_UNIX.load(Ordering::Relaxed);
    if now.saturating_sub(last) < PRUNE_INTERVAL_SECS {
        return;
    }
    if LAST_PRUNE_UNIX
        .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    //
    // Prune shares the traffic barrier with clear/ingest so it cannot delete
    // under a concurrent clear or mid-query scan without coordination.
    //
    let _guard = barrier.read().await;
    if let Err(error) = database.prune_old_traffic().await {
        common::log_warn!("Failed to prune old traffic: {}", error);
    }
}
