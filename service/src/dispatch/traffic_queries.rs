use std::sync::Arc;

use common::{
    ClientDirectMessage, TrafficLogFilters, TrafficMatchWithDetails, TrafficSearchFilters,
};
use lapin::Channel;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::database::Database;
use crate::messaging::send_to_client;

use super::traffic_barrier::TrafficTableBarrier;

const QUEUE_CAPACITY: usize = 64;
const WORKER_CONCURRENCY: usize = 4;

pub enum TrafficQuery {
    Log {
        client_id: String,
        request_id: String,
        filters: TrafficLogFilters,
    },
    Matches {
        client_id: String,
        request_id: String,
        rule_id: Option<i64>,
        limit: usize,
        offset: usize,
    },
    Clear {
        client_id: String,
        request_id: String,
    },
    Search {
        client_id: String,
        request_id: String,
        filters: TrafficSearchFilters,
    },
    Get {
        client_id: String,
        request_id: String,
        id: i64,
    },
}

impl TrafficQuery {
    fn client_id(&self) -> &str {
        match self {
            Self::Log { client_id, .. }
            | Self::Matches { client_id, .. }
            | Self::Clear { client_id, .. }
            | Self::Search { client_id, .. }
            | Self::Get { client_id, .. } => client_id,
        }
    }

    fn request_id(&self) -> &str {
        match self {
            Self::Log { request_id, .. }
            | Self::Matches { request_id, .. }
            | Self::Clear { request_id, .. }
            | Self::Search { request_id, .. }
            | Self::Get { request_id, .. } => request_id,
        }
    }

    fn is_clear(&self) -> bool {
        matches!(self, Self::Clear { .. })
    }

    fn error_response(&self, message: String) -> ClientDirectMessage {
        let request_id = self.request_id().to_string();
        match self {
            Self::Log { .. } => ClientDirectMessage::TrafficLogResponse {
                request_id,
                entries: Vec::new(),
                total_count: 0,
                error: Some(message),
            },
            Self::Matches { .. } => ClientDirectMessage::TrafficMatchesResponse {
                request_id,
                matches: Vec::new(),
                total_count: 0,
                error: Some(message),
            },
            Self::Clear { .. } => ClientDirectMessage::TrafficCleared {
                request_id,
                deleted_count: 0,
                generation: 0,
                service_instance_id: String::new(),
                error: Some(message),
            },
            Self::Search { .. } => ClientDirectMessage::TrafficSearchResponse {
                request_id,
                entries: Vec::new(),
                total_count: 0,
                error: Some(message),
            },
            Self::Get { id, .. } => ClientDirectMessage::TrafficGetResponse {
                request_id,
                id: *id,
                entry: None,
                error: Some(message),
            },
        }
    }
}

struct QueryContext {
    database: Arc<Database>,
    client_publish_channel: Channel,
    cancel: CancellationToken,
    //
    // Shared with intercept ingest/prune: clear takes write; all other
    // traffic mutations and queries take read.
    //
    barrier: Arc<TrafficTableBarrier>,
}

pub struct TrafficQueryProcessor {
    tx: mpsc::Sender<TrafficQuery>,
    client_publish_channel: Channel,
    cancel: CancellationToken,
    tasks: TaskTracker,
}

impl TrafficQueryProcessor {
    pub fn spawn(
        database: Arc<Database>,
        client_publish_channel: Channel,
        barrier: Arc<TrafficTableBarrier>,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel(QUEUE_CAPACITY);
        let cancel = CancellationToken::new();
        let tasks = TaskTracker::new();
        let context = Arc::new(QueryContext {
            database,
            client_publish_channel: client_publish_channel.clone(),
            cancel: cancel.clone(),
            barrier,
        });
        let permits = Arc::new(Semaphore::new(WORKER_CONCURRENCY));
        let dispatcher_context = context.clone();
        let dispatcher_tasks = tasks.clone();

        tasks.spawn(async move {
            loop {
                let query = tokio::select! {
                    biased;
                    _ = dispatcher_context.cancel.cancelled() => break,
                    query = rx.recv() => match query {
                        Some(query) => query,
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
                        _ = worker_context.cancel.cancelled() => {
                            //
                            // Shutdown drops queued/in-flight queries without
                            // reply; callers time out or disconnect with the
                            // service.
                            //
                        }
                        _ = process_query(worker_context.clone(), query) => {}
                    }
                });
            }
        });

        Self {
            tx,
            client_publish_channel,
            cancel,
            tasks,
        }
    }

    pub async fn enqueue(&self, query: TrafficQuery) {
        let rejected = match self.tx.try_send(query) {
            Ok(()) => return,
            Err(mpsc::error::TrySendError::Full(query)) => Some((
                query,
                format!("traffic query queue is full (capacity {})", QUEUE_CAPACITY),
            )),
            Err(mpsc::error::TrySendError::Closed(query)) => {
                Some((query, "traffic query processor is closed".to_string()))
            }
        };

        if let Some((query, error)) = rejected {
            common::log_warn!("Rejected traffic query: {}", error);
            send_response(
                &self.client_publish_channel,
                query.client_id(),
                query.error_response(error),
            )
            .await;
        }
    }

    pub async fn shutdown(&self) {
        self.cancel.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

impl Drop for TrafficQueryProcessor {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.tasks.close();
    }
}

async fn process_query(context: Arc<QueryContext>, query: TrafficQuery) {
    let client_id = query.client_id().to_string();
    let request_id = query.request_id().to_string();

    //
    // Exclusive clear vs concurrent readers (shared with ingest/prune).
    //
    let message = if query.is_clear() {
        let _write = context.barrier.write().await;
        match context.database.clear_all_traffic().await {
            Ok(deleted_count) => {
                let generation = context.barrier.bump_generation();
                ClientDirectMessage::TrafficCleared {
                    request_id,
                    deleted_count,
                    generation,
                    service_instance_id: context.barrier.service_instance_id().to_string(),
                    error: None,
                }
            }
            Err(error) => ClientDirectMessage::TrafficCleared {
                request_id,
                deleted_count: 0,
                generation: context.barrier.generation(),
                service_instance_id: context.barrier.service_instance_id().to_string(),
                error: Some(format!("Failed to clear traffic: {}", error)),
            },
        }
    } else {
        let _read = context.barrier.read().await;
        match query {
            TrafficQuery::Log { filters, .. } => {
                match context.database.query_traffic(&filters).await {
                    Ok((mut entries, total_count)) => {
                        entries.iter_mut().for_each(|entry| entry.strip_bodies());
                        ClientDirectMessage::TrafficLogResponse {
                            request_id,
                            entries,
                            total_count,
                            error: None,
                        }
                    }
                    Err(error) => ClientDirectMessage::TrafficLogResponse {
                        request_id,
                        entries: Vec::new(),
                        total_count: 0,
                        error: Some(format!("Failed to query traffic log: {}", error)),
                    },
                }
            }
            TrafficQuery::Matches {
                rule_id,
                limit,
                offset,
                ..
            } => match context.database.query_matches(rule_id, limit, offset).await {
                Ok((mut matches, total_count)) => {
                    matches
                        .iter_mut()
                        .for_each(|item: &mut TrafficMatchWithDetails| item.traffic.strip_bodies());
                    ClientDirectMessage::TrafficMatchesResponse {
                        request_id,
                        matches,
                        total_count,
                        error: None,
                    }
                }
                Err(error) => ClientDirectMessage::TrafficMatchesResponse {
                    request_id,
                    matches: Vec::new(),
                    total_count: 0,
                    error: Some(format!("Failed to query traffic matches: {}", error)),
                },
            },
            TrafficQuery::Clear { .. } => unreachable!("clear handled above"),
            TrafficQuery::Search { filters, .. } => {
                match context.database.search_traffic(&filters).await {
                    Ok((mut entries, total_count)) => {
                        entries.iter_mut().for_each(|entry| entry.strip_bodies());
                        ClientDirectMessage::TrafficSearchResponse {
                            request_id,
                            entries,
                            total_count,
                            error: None,
                        }
                    }
                    Err(error) => ClientDirectMessage::TrafficSearchResponse {
                        request_id,
                        entries: Vec::new(),
                        total_count: 0,
                        error: Some(format!("Failed to search traffic: {}", error)),
                    },
                }
            }
            TrafficQuery::Get { id, .. } => match context.database.get_traffic(id).await {
                Ok(entry) => ClientDirectMessage::TrafficGetResponse {
                    request_id,
                    id,
                    entry,
                    error: None,
                },
                Err(error) => ClientDirectMessage::TrafficGetResponse {
                    request_id,
                    id,
                    entry: None,
                    error: Some(format!("Failed to fetch traffic entry {}: {}", id, error)),
                },
            },
        }
    };

    send_response(&context.client_publish_channel, &client_id, message).await;
}

async fn send_response(channel: &Channel, client_id: &str, message: ClientDirectMessage) {
    if let Err(error) = send_to_client(channel, client_id, message).await {
        common::log_error!(
            "Failed to send traffic query response to client {}: {}",
            common::short_id(client_id),
            error
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::TrafficLogFilters;

    #[test]
    fn error_response_echoes_request_id() {
        let query = TrafficQuery::Log {
            client_id: "c1".into(),
            request_id: "req-abc".into(),
            filters: TrafficLogFilters::default(),
        };
        match query.error_response("boom".into()) {
            ClientDirectMessage::TrafficLogResponse {
                request_id,
                error,
                total_count,
                entries,
            } => {
                assert_eq!(request_id, "req-abc");
                assert_eq!(error.as_deref(), Some("boom"));
                assert_eq!(total_count, 0);
                assert!(entries.is_empty());
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    fn get_error_response_preserves_id_and_request_id() {
        let query = TrafficQuery::Get {
            client_id: "c1".into(),
            request_id: "get-1".into(),
            id: 42,
        };
        match query.error_response("nope".into()) {
            ClientDirectMessage::TrafficGetResponse {
                request_id,
                id,
                entry,
                error,
            } => {
                assert_eq!(request_id, "get-1");
                assert_eq!(id, 42);
                assert!(entry.is_none());
                assert_eq!(error.as_deref(), Some("nope"));
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }
}
