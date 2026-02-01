//
// M365 Copilot-specific DevTools adapter implementation.
//

use crate::agent_connectors::modes::devtools::{DevToolsAdapter, DevToolsConfig};
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
        let is_generating = result["isGenerating"].as_bool().unwrap_or(true);
        let response_text = result["responseText"]
            .as_str()
            .unwrap_or("")
            .to_string();

        common::log_debug!(
            "check_response: messages={}, initial={}, generating={}, response_len={}",
            message_count, initial_count, is_generating, response_text.len()
        );

        //
        // Check if we have a new message (response).
        //

        if message_count <= initial_count {
            common::log_debug!("check_response: no new messages yet");
            return Ok(None);
        }

        //
        // Check if still generating.
        //

        if is_generating {
            common::log_debug!("check_response: still generating");
            return Ok(None);
        }

        //
        // Check if we have text content.
        //

        if !response_text.is_empty() {
            common::log_debug!("check_response: complete!");
            return Ok(Some(response_text.trim().to_string()));
        }

        common::log_debug!("check_response: waiting for text content");
        Ok(None)
    }

    async fn wait_for_submit_ready(&self, page: &Page) -> anyhow::Result<()> {
        let submit_selector = r#"button[aria-label="Send"]:not([aria-disabled="true"])"#;
        for _ in 0..100 {
            if page.find_element(submit_selector).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Ok(())
    }

    async fn post_initialize(&self, page: &Page) -> anyhow::Result<()> {
        //
        // Click the Work/Web toggle button. Defaults to "Work" if not specified.
        // These buttons may not always be available, which is fine.
        //

        let working_dir = self.working_dir.as_deref().unwrap_or(WORKING_DIR_WORK);

        let selector = match working_dir {
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

        //
        // Wait briefly for the button to appear, then click if available.
        //

        for _ in 0..30 {
            if let Ok(button) = page.find_element(selector).await {
                common::log_info!("Clicking {} toggle button", working_dir);
                if let Err(e) = button.click().await {
                    common::log_warn!("Failed to click {} toggle: {}", working_dir, e);
                }
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        //
        // Click the "New private chat" button to start a fresh conversation.
        //

        let new_chat_selector = r#"span[data-automation-id="newPrivateChatButton"]"#;
        for _ in 0..30 {
            if let Ok(button) = page.find_element(new_chat_selector).await {
                common::log_info!("Clicking new private chat button");
                if let Err(e) = button.click().await {
                    common::log_warn!("Failed to click new private chat button: {}", e);
                }
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        common::log_debug!("New private chat button not available, proceeding without it");
        Ok(())
    }
}
