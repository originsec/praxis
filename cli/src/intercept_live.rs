//
// Bounded live intercept delivery: capacity, try-push, drop-when-full with
// rate-limited diagnostics (every 1st and then every Nth drop).
//

use std::sync::atomic::{AtomicU64, Ordering};

/// Capacity for CLI intercept entry/match subscription channels.
pub const INTERCEPT_LIVE_CAPACITY: usize = 32;

/// How often to emit a diagnostic after the first drop (every Nth drop).
pub const INTERCEPT_DROP_LOG_EVERY: u64 = 100;

/// Result of pushing a live intercept batch into a bounded channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivePushResult {
    Sent,
    Dropped { drop_count: u64, should_log: bool },
    Closed,
}

/// Pure policy: whether this drop_count should emit a diagnostic.
pub fn should_log_drop(drop_count: u64, every: u64) -> bool {
    if drop_count == 0 {
        return false;
    }
    if every == 0 {
        return true;
    }
    drop_count == 1 || drop_count % every == 0
}

/// Record a full-channel drop and decide whether to log.
pub fn record_live_drop(counter: &AtomicU64, every: u64) -> LivePushResult {
    let drop_count = counter.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    LivePushResult::Dropped {
        drop_count,
        should_log: should_log_drop(drop_count, every),
    }
}

/// Map try_send outcome to LivePushResult (pure for Full branch via record).
pub fn classify_try_send_full(counter: &AtomicU64) -> LivePushResult {
    record_live_drop(counter, INTERCEPT_DROP_LOG_EVERY)
}

/// Push helper used by production wiring tests: try_send then classify Full.
pub fn try_push_bounded<T>(
    tx: &tokio::sync::mpsc::Sender<T>,
    value: T,
    drop_counter: &AtomicU64,
) -> LivePushResult {
    match tx.try_send(value) {
        Ok(()) => LivePushResult::Sent,
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            classify_try_send_full(drop_counter)
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => LivePushResult::Closed,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_try_send_full, should_log_drop, try_push_bounded, LivePushResult,
        INTERCEPT_DROP_LOG_EVERY, INTERCEPT_LIVE_CAPACITY,
    };
    use std::sync::atomic::AtomicU64;

    #[test]
    fn drop_log_rate_limit() {
        assert!(!should_log_drop(0, 100));
        assert!(should_log_drop(1, 100));
        assert!(!should_log_drop(2, 100));
        assert!(should_log_drop(100, 100));
        assert!(should_log_drop(200, 100));
        assert!(!should_log_drop(101, 100));
    }

    #[test]
    fn classify_full_increments_and_logs_first() {
        let counter = AtomicU64::new(0);
        match classify_try_send_full(&counter) {
            LivePushResult::Dropped {
                drop_count,
                should_log,
            } => {
                assert_eq!(drop_count, 1);
                assert!(should_log);
            }
            other => panic!("expected Dropped, got {:?}", other),
        }
        match classify_try_send_full(&counter) {
            LivePushResult::Dropped {
                drop_count,
                should_log,
            } => {
                assert_eq!(drop_count, 2);
                assert!(!should_log);
            }
            other => panic!("expected Dropped, got {:?}", other),
        }
        while counter.load(std::sync::atomic::Ordering::Relaxed) + 1 < INTERCEPT_DROP_LOG_EVERY {
            let _ = classify_try_send_full(&counter);
        }
        match classify_try_send_full(&counter) {
            LivePushResult::Dropped {
                drop_count,
                should_log,
            } => {
                assert_eq!(drop_count, INTERCEPT_DROP_LOG_EVERY);
                assert!(should_log);
            }
            other => panic!("expected Dropped, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn real_mpsc_try_send_drops_when_full() {
        //
        // Drive the same capacity and try_push_bounded helper production uses
        // for live intercept paths (client + event bridge).
        //
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u32>(INTERCEPT_LIVE_CAPACITY);
        let drops = AtomicU64::new(0);

        for i in 0..INTERCEPT_LIVE_CAPACITY as u32 {
            assert_eq!(
                try_push_bounded(&tx, i, &drops),
                LivePushResult::Sent,
                "slot {i} should send"
            );
        }
        // Channel full — next push is dropped with policy.
        match try_push_bounded(&tx, 9999, &drops) {
            LivePushResult::Dropped {
                drop_count,
                should_log,
            } => {
                assert_eq!(drop_count, 1);
                assert!(should_log);
            }
            other => panic!("expected Dropped on full channel, got {other:?}"),
        }
        // Consumer drains one slot — send succeeds again.
        assert_eq!(rx.recv().await, Some(0));
        assert_eq!(try_push_bounded(&tx, 42, &drops), LivePushResult::Sent);

        // Exactly capacity items remain readable (capacity-1 left + 42).
        let mut n = 0;
        while rx.try_recv().is_ok() {
            n += 1;
        }
        assert_eq!(n, INTERCEPT_LIVE_CAPACITY);
    }
}
