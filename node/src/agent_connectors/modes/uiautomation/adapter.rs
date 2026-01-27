//
// UIAutomation adapter trait - agent-specific code implements this to configure
// how the generic UIAutomation session interacts with a particular application.
//

use crate::utils::UIAutomationControl;
use anyhow::Result;

/// Configuration for the UIAutomation session.
pub struct UIAutomationConfig {
    pub process_path: Option<String>,
    /// Window title prefix to search for (e.g., "Microsoft 365 Copilot").
    pub window_title_prefix: String,
}

/// Adapter trait for UIAutomation-based sessions. Agent-specific code implements
/// this to define how to interact with the target application.
pub trait UIAutomationAdapter: Send + Sync {
    /// Returns the configuration for this adapter.
    fn config(&self) -> UIAutomationConfig;

    /// Class name prefix for the input element.
    fn input_class_prefix(&self) -> &str;

    /// Class name prefix for message elements.
    fn message_class_prefix(&self) -> &str;

    /// Check if the input element is ready.
    fn is_input_ready(&self, ctrl: &UIAutomationControl) -> bool;

    /// Count the number of messages currently displayed.
    fn count_messages(&self, ctrl: &UIAutomationControl) -> Result<usize>;

    /// Send the prompt text to the input element.
    fn send_prompt(&self, ctrl: &UIAutomationControl, prompt: &str) -> Result<()>;

    /// Submit the prompt (e.g., press Enter).
    fn submit_prompt(&self, ctrl: &UIAutomationControl) -> Result<()>;

    /// Check if the response is complete. Returns Some(text) if done, None if still generating.
    fn check_response_complete(
        &self,
        ctrl: &UIAutomationControl,
        initial_count: usize,
    ) -> Result<Option<String>>;

    /// Optional prompt to use for get_info(). Returns None if get_info is not supported.
    fn info_prompt(&self) -> Option<&str> {
        None
    }
}
