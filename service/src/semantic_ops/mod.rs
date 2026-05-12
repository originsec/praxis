pub mod chain_execution;
pub mod executor;
pub mod manager;

pub use chain_execution::ChainExecutor;
#[allow(unused_imports)]
pub use executor::{
    cancel_session_prompt, close_session, create_session, execute_agent_mode, execute_by_mode,
    execute_one_shot,
};
pub use manager::SemanticOpsManager;

//
// Sentinel error returned by executors when an operation is cancelled.
// Carrying a typed value lets callers distinguish cancellation from
// failure via `downcast_ref::<Cancelled>()` instead of string-matching
// the `Display` form of the error.
//

#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Operation cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub fn is_cancelled(err: &anyhow::Error) -> bool {
    err.downcast_ref::<Cancelled>().is_some()
}
