//
// Live intercept traffic broadcaster.
//
// Coalesces newly-captured traffic entries and new rule matches into
// small batches (100 ms window or 50-item cap) and publishes them to the
// client broadcast exchange. Receivers (web frontend, cli TUI) can then
// show entries in real time without polling.
//
// Request/response bodies are stripped from the broadcast payload to
// keep messages small; clients refetch the full entry via
// ClientSignalMessage::TrafficGetRequest when the user opens a row.
//

use std::sync::Arc;
use std::time::Duration;

use common::{
    publish_json_exchange, ClientBroadcastMessage, InterceptedTrafficEntry,
    TrafficMatchWithDetails, CLIENT_BROADCAST_EXCHANGE,
};
use lapin::Channel;
use tokio::sync::mpsc;

const FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const MAX_BATCH: usize = 50;

//
// Strip request/response bodies from an entry. Callers broadcast header
// metadata only; full bodies are available via TrafficGetRequest.
//

fn strip_bodies(entry: &mut InterceptedTrafficEntry) {
    entry.request_body = None;
    entry.response_body = None;
}

pub struct InterceptBroadcaster {
    entries_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
    matches_tx: mpsc::UnboundedSender<TrafficMatchWithDetails>,
}

impl InterceptBroadcaster {
    //
    // Spawn the batcher task and return a handle that producers can use
    // to push entries and matches. The task lives for the lifetime of the
    // process.
    //
    pub fn spawn(broadcast_channel: Channel) -> Arc<Self> {
        let (entries_tx, entries_rx) = mpsc::unbounded_channel();
        let (matches_tx, matches_rx) = mpsc::unbounded_channel();

        tokio::spawn(batch_entries(entries_rx, broadcast_channel.clone()));
        tokio::spawn(batch_matches(matches_rx, broadcast_channel));

        Arc::new(Self {
            entries_tx,
            matches_tx,
        })
    }

    pub fn push_entry(&self, mut entry: InterceptedTrafficEntry) {
        strip_bodies(&mut entry);
        let _ = self.entries_tx.send(entry);
    }

    pub fn push_match(&self, mut m: TrafficMatchWithDetails) {
        strip_bodies(&mut m.traffic);
        let _ = self.matches_tx.send(m);
    }
}

async fn batch_entries(
    mut rx: mpsc::UnboundedReceiver<InterceptedTrafficEntry>,
    channel: Channel,
) {
    let mut buffer: Vec<InterceptedTrafficEntry> = Vec::with_capacity(MAX_BATCH);
    let mut flush_timer = tokio::time::interval(FLUSH_INTERVAL);
    flush_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            entry = rx.recv() => {
                match entry {
                    Some(e) => {
                        buffer.push(e);
                        if buffer.len() >= MAX_BATCH {
                            flush_entries(&channel, &mut buffer).await;
                        }
                    }
                    None => {
                        flush_entries(&channel, &mut buffer).await;
                        return;
                    }
                }
            }
            _ = flush_timer.tick() => {
                if !buffer.is_empty() {
                    flush_entries(&channel, &mut buffer).await;
                }
            }
        }
    }
}

async fn batch_matches(
    mut rx: mpsc::UnboundedReceiver<TrafficMatchWithDetails>,
    channel: Channel,
) {
    let mut buffer: Vec<TrafficMatchWithDetails> = Vec::with_capacity(MAX_BATCH);
    let mut flush_timer = tokio::time::interval(FLUSH_INTERVAL);
    flush_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            m = rx.recv() => {
                match m {
                    Some(m) => {
                        buffer.push(m);
                        if buffer.len() >= MAX_BATCH {
                            flush_matches(&channel, &mut buffer).await;
                        }
                    }
                    None => {
                        flush_matches(&channel, &mut buffer).await;
                        return;
                    }
                }
            }
            _ = flush_timer.tick() => {
                if !buffer.is_empty() {
                    flush_matches(&channel, &mut buffer).await;
                }
            }
        }
    }
}

async fn flush_entries(channel: &Channel, buffer: &mut Vec<InterceptedTrafficEntry>) {
    if buffer.is_empty() {
        return;
    }
    let entries = std::mem::take(buffer);
    let msg = ClientBroadcastMessage::InterceptedTrafficBatch { entries };
    if let Err(e) = publish_json_exchange(channel, CLIENT_BROADCAST_EXCHANGE, &msg).await {
        common::log_error!("Failed to broadcast intercepted traffic batch: {}", e);
    }
}

async fn flush_matches(channel: &Channel, buffer: &mut Vec<TrafficMatchWithDetails>) {
    if buffer.is_empty() {
        return;
    }
    let matches = std::mem::take(buffer);
    let msg = ClientBroadcastMessage::TrafficMatchBatch { matches };
    if let Err(e) = publish_json_exchange(channel, CLIENT_BROADCAST_EXCHANGE, &msg).await {
        common::log_error!("Failed to broadcast traffic match batch: {}", e);
    }
}
