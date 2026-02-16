//!
//! Unified logging - sends log entries to both tracing and optionally to a centralized event log.
//!

use chrono::Utc;
use crate::ApplicationLogEntry;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

//
// Global channel for sending log events.
//
static EVENT_LOG_TX: OnceLock<mpsc::UnboundedSender<ApplicationLogEntry>> = OnceLock::new();
static SOURCE: OnceLock<String> = OnceLock::new();
static SOURCE_ID: OnceLock<String> = OnceLock::new();
static EVENT_LOG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Initialize the event log sender. Call this once during startup.
/// source is the category ("node", "service", "web"), source_id is the instance identifier.
pub fn init(source: String, source_id: String, tx: mpsc::UnboundedSender<ApplicationLogEntry>) {
    let _ = SOURCE.set(source);
    let _ = SOURCE_ID.set(source_id);
    let _ = EVENT_LOG_TX.set(tx);
}

/// Enable or disable centralized event logging.
pub fn set_event_log_enabled(enabled: bool) {
    EVENT_LOG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if centralized event logging is enabled.
pub fn is_event_log_enabled() -> bool {
    EVENT_LOG_ENABLED.load(Ordering::Relaxed)
}

/// Send a log event to the service.
/// This is non-blocking and will silently fail if not initialized.
pub fn send_event(level: &str, target: &str, message: String) {
    if !is_event_log_enabled() {
        return;
    }

    if let (Some(tx), Some(source)) = (EVENT_LOG_TX.get(), SOURCE.get()) {
        let entry = ApplicationLogEntry {
            source: source.clone(),
            source_id: SOURCE_ID.get().cloned().unwrap_or_default(),
            level: level.to_string(),
            message,
            target: Some(target.to_string()),
            timestamp: Utc::now(),
        };

        //
        // Fire-and-forget - don't block on logging.
        //
        let _ = tx.send(entry);
    }
}

/// Check if event logging is initialized.
pub fn is_initialized() -> bool {
    EVENT_LOG_TX.get().is_some() && SOURCE.get().is_some()
}

//
// Logging macros that send to both tracing and the event log.
//

/// Log an info message to both tracing and the event log.
#[macro_export]
macro_rules! log_info {
    (target: $target:expr, $($arg:tt)*) => {{
        tracing::info!(target: $target, $($arg)*);
        $crate::logging::send_event("info", $target, format!($($arg)*));
    }};
    ($($arg:tt)*) => {{
        tracing::info!($($arg)*);
        $crate::logging::send_event("info", module_path!(), format!($($arg)*));
    }};
}

/// Log a warning message to both tracing and the event log.
#[macro_export]
macro_rules! log_warn {
    (target: $target:expr, $($arg:tt)*) => {{
        tracing::warn!(target: $target, $($arg)*);
        $crate::logging::send_event("warn", $target, format!($($arg)*));
    }};
    ($($arg:tt)*) => {{
        tracing::warn!($($arg)*);
        $crate::logging::send_event("warn", module_path!(), format!($($arg)*));
    }};
}

/// Log an error message to both tracing and the event log.
#[macro_export]
macro_rules! log_error {
    (target: $target:expr, $($arg:tt)*) => {{
        tracing::error!(target: $target, $($arg)*);
        $crate::logging::send_event("error", $target, format!($($arg)*));
    }};
    ($($arg:tt)*) => {{
        tracing::error!($($arg)*);
        $crate::logging::send_event("error", module_path!(), format!($($arg)*));
    }};
}

/// Log a debug message to both tracing and the event log.
#[macro_export]
macro_rules! log_debug {
    (target: $target:expr, $($arg:tt)*) => {{
        tracing::debug!(target: $target, $($arg)*);
        $crate::logging::send_event("debug", $target, format!($($arg)*));
    }};
    ($($arg:tt)*) => {{
        tracing::debug!($($arg)*);
        $crate::logging::send_event("debug", module_path!(), format!($($arg)*));
    }};
}

/// Log a trace message to both tracing and the event log.
#[macro_export]
macro_rules! log_trace {
    (target: $target:expr, $($arg:tt)*) => {{
        tracing::trace!(target: $target, $($arg)*);
        $crate::logging::send_event("trace", $target, format!($($arg)*));
    }};
    ($($arg:tt)*) => {{
        tracing::trace!($($arg)*);
        $crate::logging::send_event("trace", module_path!(), format!($($arg)*));
    }};
}
