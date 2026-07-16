//
// Shared mutation barrier for intercepted traffic: clear takes an exclusive
// write lock; queries, ingest, and prune take a shared read lock. A generation
// counter advances on successful clear so workers that waited across a clear
// can drop stale pre-clear entries instead of re-inserting them.
//

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

/// Pure transition for clear generation (unit-tested).
pub fn next_clear_generation(current: u64) -> u64 {
    current.wrapping_add(1)
}

/// True when an in-flight ingest started under `started_at` must be dropped
/// because a clear advanced the generation.
pub fn should_drop_stale_ingest(started_at: u64, current: u64) -> bool {
    started_at != current
}

pub struct TrafficTableBarrier {
    lock: Arc<RwLock<()>>,
    generation: AtomicU64,
    /// Opaque id for this service process (scopes generation across restarts).
    service_instance_id: String,
}

impl TrafficTableBarrier {
    pub fn new(service_instance_id: String) -> Arc<Self> {
        Arc::new(Self {
            lock: Arc::new(RwLock::new(())),
            generation: AtomicU64::new(0),
            service_instance_id,
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn service_instance_id(&self) -> &str {
        &self.service_instance_id
    }

    pub async fn read(&self) -> OwnedRwLockReadGuard<()> {
        self.lock.clone().read_owned().await
    }

    pub async fn write(&self) -> OwnedRwLockWriteGuard<()> {
        self.lock.clone().write_owned().await
    }

    /// Advance generation after a successful clear while holding the write lock.
    pub fn bump_generation(&self) -> u64 {
        let prev = self.generation.load(Ordering::Acquire);
        let next = next_clear_generation(prev);
        self.generation.store(next, Ordering::Release);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::{next_clear_generation, should_drop_stale_ingest};

    #[test]
    fn generation_advances_and_wraps() {
        assert_eq!(next_clear_generation(0), 1);
        assert_eq!(next_clear_generation(41), 42);
        assert_eq!(next_clear_generation(u64::MAX), 0);
    }

    #[test]
    fn stale_ingest_dropped_when_generation_changes() {
        assert!(!should_drop_stale_ingest(7, 7));
        assert!(should_drop_stale_ingest(7, 8));
        assert!(should_drop_stale_ingest(0, 1));
    }

    #[test]
    fn enqueue_stamp_survives_clear_semantics() {
        //
        // Enqueue stamps gen G; clear advances to G+1; process must drop.
        //
        let enqueued_at = 3u64;
        let after_clear = next_clear_generation(enqueued_at);
        assert!(should_drop_stale_ingest(enqueued_at, after_clear));
        // Post-clear enqueue at new gen is accepted.
        assert!(!should_drop_stale_ingest(after_clear, after_clear));
    }
}
