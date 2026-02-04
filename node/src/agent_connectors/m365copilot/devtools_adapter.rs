//
// M365 Copilot-specific DevTools adapter implementation.
//

use crate::agent_connectors::modes::devtools::{wait_for_element, DevToolsAdapter, DevToolsConfig, ResponseCheckState};
use anyhow::Result;
use chromiumoxide::page::Page;

use super::session::{WORKING_DIR_WEB, WORKING_DIR_WORK};

pub struct M365DevToolsAdapter {
    process_path: Option<String>,
    working_dir: Option<String>,
}

impl M365DevToolsAdapter {
    pub fn new(process_path: Option<String>, working_dir: Option<String>) -> Self {
        Self {
            process_path,
            working_dir,
        }
    }
}

impl DevToolsAdapter for M365DevToolsAdapter {
    fn config(&self) -> DevToolsConfig {
        DevToolsConfig {
            process_path: self.process_path.clone(),
            debug_port_env_var: "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS".to_string(),
            debug_port_format: "--remote-debugging-port={}".to_string(),
            base_port: 9222,
            port_range: 778,
            use_hidden_desktop: super::USE_HIDDEN_DESKTOP,
        }
    }

    fn input_selector(&self) -> &str {
        r#"#m365-chat-editor-target-element"#
    }

    fn message_selector(&self) -> &str {
        r#"div[data-testid="markdown-reply"]"#
    }

    fn working_dir(&self) -> Option<String> {
        self.working_dir.clone()
    }

    async fn check_response_complete(
        &self,
        page: &Page,
        initial_count: usize,
    ) -> Result<Option<String>> {
        let state = self.check_response_state(page, initial_count).await?;
        Ok(state.response)
    }

    async fn check_response_state(
        &self,
        page: &Page,
        initial_count: usize,
    ) -> Result<ResponseCheckState> {
        //
        // Check message count, toolbar count, and last message text via JavaScript.
        //

        let result: serde_json::Value = page
            .evaluate(
                r#"
                (function() {
                    const contentElements = document.querySelectorAll('div[data-testid="markdown-reply"]');

                    let responseText = '';
                    if (contentElements.length > 0) {
                        const lastContent = contentElements[contentElements.length - 1];
                        responseText = (lastContent.innerText || lastContent.textContent || '').trim();
                    }

                    // Check if still generating by looking for "Stop generating" button
                    const stopButton = document.querySelector('button[aria-label="Stop generating"]');

                    return {
                        responseText: responseText,
                        messageCount: contentElements.length,
                        isGenerating: stopButton !== null
                    };
                })()
                "#,
            )
            .await?
            .into_value()?;

        let message_count = result["messageCount"].as_u64().unwrap_or(0) as usize;
        let is_generating = result["isGenerating"].as_bool().unwrap_or(false);
        let response_text = result["responseText"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let has_new_messages = message_count > initial_count;

        common::log_debug!(
            "check_response: messages={}, initial={}, generating={}, response_len={}",
            message_count, initial_count, is_generating, response_text.len()
        );

        //
        // Determine if response is complete.
        //

        let response = if has_new_messages && !is_generating && !response_text.is_empty() {
            common::log_debug!("check_response: complete!");
            Some(response_text.trim().to_string())
        } else {
            if !has_new_messages {
                common::log_debug!("check_response: no new messages yet");
            } else if is_generating {
                common::log_debug!("check_response: still generating");
            } else {
                common::log_debug!("check_response: waiting for text content");
            }
            None
        };

        Ok(ResponseCheckState {
            response,
            is_generating,
            has_new_messages,
        })
    }

    async fn wait_for_submit_ready(&self, page: &Page) -> anyhow::Result<()> {
        let selector = r#"button[aria-label="Send"]:not([aria-disabled="true"])"#;
        wait_for_element(page, selector, 100, 100).await;
        Ok(())
    }

    async fn post_initialize(&self, page: &Page) -> anyhow::Result<()> {
        //
        // Wait for input element to be ready.
        //

        let input_selector = self.input_selector();
        common::log_debug!("Waiting for input element: {}", input_selector);
        wait_for_element(page, input_selector, 30, 300).await;

        //
        // Click the Work/Web toggle button. Defaults to "Work" if not specified.
        //

        let working_dir = self.working_dir.as_deref().unwrap_or(WORKING_DIR_WORK);
        let toggle_selector = match working_dir {
            WORKING_DIR_WORK => r#"button[data-testid="toggle-work"]"#,
            WORKING_DIR_WEB => r#"button[data-testid="toggle-web"]"#,
            _ => {
                common::log_warn!(
                    "Unknown working_dir '{}', expected '{}' or '{}'",
                    working_dir,
                    WORKING_DIR_WORK,
                    WORKING_DIR_WEB
                );
                return Ok(());
            }
        };

        if let Some(button) = wait_for_element(page, toggle_selector, 3, 300).await {
            common::log_debug!("Clicking {} toggle button", working_dir);
            if let Err(e) = button.click().await {
                common::log_warn!("Failed to click {} toggle: {}", working_dir, e);
            }
        }

        //
        // Click the new chat menu button, then select "New private chat".
        //

        let menu_selector = r#"button[data-automation-id="newPrivateChatMenuButton"]"#;
        if let Some(menu_button) = wait_for_element(page, menu_selector, 3, 300).await {
            common::log_debug!("Clicking new private chat menu button");
            if let Err(e) = menu_button.click().await {
                common::log_warn!("Failed to click menu button: {}", e);
                return Ok(());
            }

            let new_chat_selector = r#"div[data-automation-id="newPrivateChatButton"]"#;
            if let Some(chat_button) = wait_for_element(page, new_chat_selector, 5, 300).await {
                common::log_debug!("Clicking new private chat button");
                if let Err(e) = chat_button.click().await {
                    common::log_warn!("Failed to click new private chat button: {}", e);
                }
            }
        }

        Ok(())
    }
}
