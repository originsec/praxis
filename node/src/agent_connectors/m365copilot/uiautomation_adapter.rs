//
// M365 Copilot-specific UIAutomation adapter implementation.
//

use crate::agent_connectors::modes::uiautomation::{UIAutomationAdapter, UIAutomationConfig};
use crate::utils::UIAutomationControl;
use anyhow::Result;

use super::ui_operations::{
    count_toolbars_by_class_prefix, extract_message_text, find_messages_by_class_prefix,
    get_message_content_element,
};

pub struct M365UIAutomationAdapter {
    process_path: Option<String>,
}

impl M365UIAutomationAdapter {
    pub fn new(process_path: Option<String>) -> Self {
        Self { process_path }
    }
}

impl UIAutomationAdapter for M365UIAutomationAdapter {
    fn config(&self) -> UIAutomationConfig {
        UIAutomationConfig {
            process_path: self.process_path.clone(),
            window_title_prefix: "Microsoft 365 Copilot".to_string(),
        }
    }

    fn input_class_prefix(&self) -> &str {
        "fai-EditorInput__input"
    }

    fn message_class_prefix(&self) -> &str {
        "fai-CopilotMessage"
    }

    fn is_input_ready(&self, ctrl: &UIAutomationControl) -> bool {
        ctrl.has_element_with_class_prefix(self.input_class_prefix())
    }

    fn count_messages(&self, ctrl: &UIAutomationControl) -> Result<usize> {
        let messages =
            find_messages_by_class_prefix(ctrl, self.message_class_prefix()).unwrap_or_default();
        Ok(messages.len())
    }

    fn send_prompt(&self, ctrl: &UIAutomationControl, prompt: &str) -> Result<()> {
        ctrl.send_text(self.input_class_prefix(), prompt)
            .map_err(|e| anyhow::anyhow!("Failed to send text: {}", e))
    }

    fn submit_prompt(&self, ctrl: &UIAutomationControl) -> Result<()> {
        ctrl.send_keys(self.input_class_prefix(), "{ENTER}")
            .map_err(|e| anyhow::anyhow!("Failed to send keys: {}", e))
    }

    fn check_response_complete(
        &self,
        ctrl: &UIAutomationControl,
        initial_count: usize,
    ) -> Result<Option<String>> {
        let messages =
            find_messages_by_class_prefix(ctrl, self.message_class_prefix()).unwrap_or_default();
        let message_count = messages.len();

        //
        // Check if we have a new message (response).
        //

        if message_count <= initial_count {
            return Ok(None);
        }

        //
        // Check if toolbar count matches message count (message is complete).
        //

        let toolbar_count =
            count_toolbars_by_class_prefix(ctrl, self.message_class_prefix()).unwrap_or(0);

        if toolbar_count >= message_count {
            //
            // Message is complete - extract text from the last message.
            //

            if let Some(last_message) = messages.last() {
                if let Ok(Some(content_elem)) = get_message_content_element(ctrl, last_message) {
                    if let Ok(Some(text)) = extract_message_text(ctrl, &content_elem) {
                        if !text.is_empty() {
                            return Ok(Some(text));
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}
