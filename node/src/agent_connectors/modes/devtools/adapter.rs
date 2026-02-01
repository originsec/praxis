//
// DevTools adapter trait - agent-specific code implements this to configure how
// the generic DevTools session interacts with a particular application.
//

use anyhow::Result;
use chromiumoxide::page::Page;
use std::future::Future;

//
// Check if hidden desktop should be used for DevTools-based agent windows.
//
// By default, returns true (use hidden desktop) to keep agent windows
// invisible during normal operation. When PRAXIS_NOT_HIDDEN environment
// variable is set to "1", returns false to make windows visible for
// debugging/testing.
//
// Hidden desktop (Windows only):
// - Spawns agent processes on an invisible desktop
// - Processes run normally but windows are not displayed to the user
// - Useful for headless/automated operation
//
// To disable hidden desktop:
//   set PRAXIS_NOT_HIDDEN=1
//

pub fn use_hidden_desktop() -> bool {
    std::env::var("PRAXIS_NOT_HIDDEN")
        .map(|v| v != "1")
        .unwrap_or(true)
}

/// Configuration for the DevTools session.
pub struct DevToolsConfig {
    pub process_path: Option<String>,
    /// Environment variable to set the debug port (e.g., "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").
    pub debug_port_env_var: String,
    /// Format string for the debug port argument (e.g., "--remote-debugging-port={}").
    pub debug_port_format: String,
    /// Base port to start from when picking a random debug port.
    pub base_port: u16,
    /// Range of ports to pick from (port = base_port + random(0..port_range)).
    pub port_range: u16,
    /// Whether to run the process on a hidden desktop (Windows only).
    pub use_hidden_desktop: bool,
}

/// Adapter trait for DevTools-based sessions. Agent-specific code implements
/// this to define how to interact with the target application.
pub trait DevToolsAdapter: Send + Sync {
    /// Returns the configuration for this adapter.
    fn config(&self) -> DevToolsConfig;

    /// CSS selector for the input element where prompts are typed.
    fn input_selector(&self) -> &str;

    /// CSS selector for message elements in the chat.
    fn message_selector(&self) -> &str;

    /// Returns the working directory for this session, if any.
    fn working_dir(&self) -> Option<String> {
        None
    }

    /// Check if the response is complete. Returns Some(text) if done, None if still generating.
    /// The page is provided so adapters can run JavaScript queries as needed.
    fn check_response_complete(
        &self,
        page: &Page,
        initial_count: usize,
    ) -> impl Future<Output = Result<Option<String>>> + Send;

    /// Called after text is inserted but before submit. Adapters can use this
    /// to wait for submit button to be ready, etc. Default does nothing.
    fn wait_for_submit_ready(
        &self,
        _page: &Page,
    ) -> impl Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    /// Called after the session is initialized (page connected, ready for use).
    /// Adapters can use this to perform post-initialization tasks like clicking
    /// mode toggle buttons. Default does nothing.
    fn post_initialize(
        &self,
        _page: &Page,
    ) -> impl Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }
}
