//
// Live intercept traffic broadcaster.
//
// Coalesces newly-captured traffic entries and new rule matches into
// small batches (100 ms window or 50-item cap) and publishes them to the
// client broadcast exchange. Each item carries the clear generation at
// push time; flush drops items older than the current barrier generation.
//

use std::sync::Arc;
use std::time::Duration;

use common::{
    CLIENT_BROADCAST_EXCHANGE, ClientBroadcastMessage, InterceptedTrafficEntry,
    TrafficMatchWithDetails, publish_json_exchange,
};
use lapin::Channel;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::traffic_barrier::TrafficTableBarrier;

const FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const MAX_BATCH: usize = 50;
const QUEUE_CAPACITY: usize = 4096;

fn strip_bodies(entry: &mut InterceptedTrafficEntry) {
    entry.strip_bodies();
}

/// Pure filter: retain only items whose generation is still current.
pub fn retain_current_generation<T>(
    items: &mut Vec<(u64, T)>,
    current_generation: u64,
) -> usize {
    let before = items.len();
    items.retain(|(item_gen, _)| *item_gen >= current_generation);
    before.saturating_sub(items.len())
}

struct GenEntry {
    generation: u64,
    entry: InterceptedTrafficEntry,
}

struct GenMatch {
    generation: u64,
    m: TrafficMatchWithDetails,
}

pub struct InterceptBroadcaster {
    entries_tx: mpsc::Sender<GenEntry>,
    matches_tx: mpsc::Sender<GenMatch>,
    barrier: Arc<TrafficTableBarrier>,
    dropped: Arc<std::sync::atomic::AtomicU64>,
    last_warn_unix: Arc<std::sync::atomic::AtomicU64>,
    cancel: CancellationToken,
    tasks: TaskTracker,
}

impl InterceptBroadcaster {
    pub fn spawn(
        broadcast_channel: Channel,
        barrier: Arc<TrafficTableBarrier>,
    ) -> Arc<Self> {
        let (entries_tx, entries_rx) = mpsc::channel(QUEUE_CAPACITY);
        let (matches_tx, matches_rx) = mpsc::channel(QUEUE_CAPACITY);
        let cancel = CancellationToken::new();
        let tasks = TaskTracker::new();

        tasks.spawn(batch_entries(
            entries_rx,
            broadcast_channel.clone(),
            barrier.clone(),
            cancel.clone(),
        ));
        tasks.spawn(batch_matches(
            matches_rx,
            broadcast_channel,
            barrier.clone(),
            cancel.clone(),
        ));

        Arc::new(Self {
            entries_tx,
            matches_tx,
            barrier,
            dropped: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            last_warn_unix: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            cancel,
            tasks,
        })
    }

    pub fn push_entry(&self, mut entry: InterceptedTrafficEntry, generation: u64) {
        strip_bodies(&mut entry);
        if generation < self.barrier.generation() {
            return;
        }
        if self
            .entries_tx
            .try_send(GenEntry { generation, entry })
            .is_err()
        {
            self.warn_drop();
        }
    }

    pub fn push_match(&self, mut m: TrafficMatchWithDetails, generation: u64) {
        strip_bodies(&mut m.traffic);
        if generation < self.barrier.generation() {
            return;
        }
        if self
            .matches_tx
            .try_send(GenMatch { generation, m })
            .is_err()
        {
            self.warn_drop();
        }
    }

    fn warn_drop(&self) {
        use std::sync::atomic::Ordering;
        use std::time::{SystemTime, UNIX_EPOCH};

        let dropped = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let last = self.last_warn_unix.load(Ordering::Relaxed);
        if now.saturating_sub(last) < 5
            || self
                .last_warn_unix
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        common::log_warn!(
            "Live intercept broadcast queue is full; {} update(s) dropped",
            dropped
        );
    }

    pub async fn shutdown(&self) {
        self.cancel.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

impl Drop for InterceptBroadcaster {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.tasks.close();
    }
}

async fn batch_entries(
    mut rx: mpsc::Receiver<GenEntry>,
    channel: Channel,
    barrier: Arc<TrafficTableBarrier>,
    cancel: CancellationToken,
) {
    let mut buffer: Vec<(u64, InterceptedTrafficEntry)> = Vec::with_capacity(MAX_BATCH);
    let mut flush_timer = tokio::time::interval(FLUSH_INTERVAL);
    flush_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            entry = rx.recv() => {
                match entry {
                    Some(GenEntry { generation, entry }) => {
                        buffer.push((generation, entry));
                        if buffer.len() >= MAX_BATCH {
                            if !flush_entries(&channel, &mut buffer, &barrier, &cancel).await {
                                return;
                            }
                        }
                    }
                    None => return,
                }
            }
            _ = flush_timer.tick() => {
                if !buffer.is_empty() {
                    if !flush_entries(&channel, &mut buffer, &barrier, &cancel).await {
                        return;
                    }
                }
            }
        }
    }
}

async fn batch_matches(
    mut rx: mpsc::Receiver<GenMatch>,
    channel: Channel,
    barrier: Arc<TrafficTableBarrier>,
    cancel: CancellationToken,
) {
    let mut buffer: Vec<(u64, TrafficMatchWithDetails)> = Vec::with_capacity(MAX_BATCH);
    let mut flush_timer = tokio::time::interval(FLUSH_INTERVAL);
    flush_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            m = rx.recv() => {
                match m {
                    Some(GenMatch { generation, m }) => {
                        buffer.push((generation, m));
                        if buffer.len() >= MAX_BATCH {
                            if !flush_matches(&channel, &mut buffer, &barrier, &cancel).await {
                                return;
                            }
                        }
                    }
                    None => return,
                }
            }
            _ = flush_timer.tick() => {
                if !buffer.is_empty() {
                    if !flush_matches(&channel, &mut buffer, &barrier, &cancel).await {
                        return;
                    }
                }
            }
        }
    }
}

async fn flush_entries(
    channel: &Channel,
    buffer: &mut Vec<(u64, InterceptedTrafficEntry)>,
    barrier: &TrafficTableBarrier,
    cancel: &CancellationToken,
) -> bool {
    if buffer.is_empty() {
        return true;
    }
    let current = barrier.generation();
    let dropped = retain_current_generation(buffer, current);
    if dropped > 0 {
        common::log_info!(
            "Dropped {} pre-clear live traffic item(s) at flush (generation {})",
            dropped,
            current
        );
    }
    if buffer.is_empty() {
        return true;
    }
    //
    // Batch generation is the minimum retained item gen (all >= current).
    //
    let generation = buffer.iter().map(|(g, _)| *g).min().unwrap_or(current);
    let entries: Vec<_> = buffer.drain(..).map(|(_, e)| e).collect();
    let msg = ClientBroadcastMessage::InterceptedTrafficBatch {
        entries,
        generation,
        service_instance_id: barrier.service_instance_id().to_string(),
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => false,
        result = publish_json_exchange(channel, CLIENT_BROADCAST_EXCHANGE, &msg) => {
            if let Err(error) = result {
                common::log_error!("Failed to broadcast intercepted traffic batch: {}", error);
            }
            true
        }
    }
}

async fn flush_matches(
    channel: &Channel,
    buffer: &mut Vec<(u64, TrafficMatchWithDetails)>,
    barrier: &TrafficTableBarrier,
    cancel: &CancellationToken,
) -> bool {
    if buffer.is_empty() {
        return true;
    }
    let current = barrier.generation();
    let dropped = retain_current_generation(buffer, current);
    if dropped > 0 {
        common::log_info!(
            "Dropped {} pre-clear live match item(s) at flush (generation {})",
            dropped,
            current
        );
    }
    if buffer.is_empty() {
        return true;
    }
    let generation = buffer.iter().map(|(g, _)| *g).min().unwrap_or(current);
    let matches: Vec<_> = buffer.drain(..).map(|(_, m)| m).collect();
    let msg = ClientBroadcastMessage::TrafficMatchBatch {
        matches,
        generation,
        service_instance_id: barrier.service_instance_id().to_string(),
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => false,
        result = publish_json_exchange(channel, CLIENT_BROADCAST_EXCHANGE, &msg) => {
            if let Err(error) = result {
                common::log_error!("Failed to broadcast traffic match batch: {}", error);
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::retain_current_generation;

    #[test]
    fn flush_filter_drops_pre_clear_generation() {
        let mut items = vec![(1u64, "a"), (1, "b"), (2, "c"), (3, "d")];
        let dropped = retain_current_generation(&mut items, 2);
        assert_eq!(dropped, 2);
        assert_eq!(items, vec![(2, "c"), (3, "d")]);
    }

    #[test]
    fn flush_filter_keeps_equal_generation() {
        let mut items = vec![(5u64, "x")];
        assert_eq!(retain_current_generation(&mut items, 5), 0);
        assert_eq!(items.len(), 1);
    }
}
