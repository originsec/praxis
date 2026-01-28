//
// Generic DevTools session implementation using Chrome DevTools Protocol via
// chromiumoxide. Agent-specific behavior is provided via the DevToolsAdapter trait.
//

mod adapter;

pub use adapter::{use_hidden_desktop, DevToolsAdapter, DevToolsConfig};

use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::utils;
use anyhow::{anyhow, Result};
use chromiumoxide::browser::Browser;
use chromiumoxide::page::Page;
use futures::StreamExt;
use std::sync::Mutex;
use uuid::Uuid;

pub struct GenericDevToolsSession<A: DevToolsAdapter> {
    adapter: A,
    session_id: Uuid,
    process_id: Option<u32>,
    process_path: Option<String>,
    page: Mutex<Option<Page>>,
    #[cfg(windows)]
    hidden_desktop: Mutex<Option<utils::HiddenDesktop>>,
}

impl<A: DevToolsAdapter> GenericDevToolsSession<A> {
    pub async fn new(adapter: A) -> Result<Self> {
        let config = adapter.config();
        let process_path = config.process_path.clone();

        let mut session = Self {
            adapter,
            session_id: Uuid::new_v4(),
            process_id: None,
            process_path: process_path.clone(),
            page: Mutex::new(None),
            #[cfg(windows)]
            hidden_desktop: Mutex::new(None),
        };

        session.initialize().await?;
        Ok(session)
    }

    async fn initialize(&mut self) -> Result<()> {
        let config = self.adapter.config();

        //
        // Kill any existing processes with the same process name before
        // starting.
        //

        if let Some(ref path) = config.process_path {
            if let Some(process_name) = std::path::Path::new(path).file_name() {
                if let Some(name) = process_name.to_str() {
                    utils::kill_processes_by_name(name);
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }

        //
        // Generate random port for DevTools debugging.
        //

        let port = config.base_port + (rand::random::<u16>() % config.port_range);
        common::log_info!("Using DevTools port {}", port);

        //
        // Launch process with DevTools environment variable. On Windows, spawn
        // on a hidden desktop so the window is invisible but fully functional.
        //

        let pid = if let Some(ref path) = config.process_path {
            let debug_arg = config.debug_port_format.replace("{}", &port.to_string());

            #[cfg(windows)]
            let pid = {
                //
                // Check both config and environment variable for hidden desktop.
                // Config must enable it AND PRAXIS_NOT_HIDDEN must not be "1".
                //
                let desktop = if config.use_hidden_desktop && use_hidden_desktop() {
                    utils::HiddenDesktop::new()
                } else {
                    None
                };

                let pid = if let Some(ref d) = desktop {
                    let pid = utils::spawn_on_hidden_desktop(
                        path,
                        &config.debug_port_env_var,
                        &debug_arg,
                        &d.name,
                    )?;
                    common::log_info!(
                        "Spawned process on hidden desktop '{}' with PID: {}",
                        d.name, pid
                    );
                    pid
                } else {
                    let process = std::process::Command::new(path)
                        .env(&config.debug_port_env_var, &debug_arg)
                        .spawn()
                        .map_err(|e| anyhow!("Failed to spawn process: {}", e))?;
                    let pid = process.id();
                    common::log_info!(
                        "Spawned process with PID: {} (no hidden desktop)",
                        pid
                    );
                    pid
                };
                *self.hidden_desktop.lock().unwrap() = desktop;
                pid
            };

            #[cfg(not(windows))]
            let pid = {
                let process = std::process::Command::new(path)
                    .env(&config.debug_port_env_var, &debug_arg)
                    .spawn()
                    .map_err(|e| anyhow!("Failed to spawn process: {}", e))?;
                let pid = process.id();
                common::log_info!(
                    "Spawned process with PID: {}",
                    pid
                );
                pid
            };

            Some(pid)
        } else {
            return Err(anyhow!("No process path provided"));
        };

        self.process_id = pid;

        //
        // Wait for DevTools endpoint to become available and connect.
        //

        let page = Self::connect_to_devtools(port).await?;

        *self.page.lock().unwrap() = Some(page);
        common::log_info!("Connected to DevTools");

        Ok(())
    }

    async fn connect_to_devtools(port: u16) -> Result<Page> {
        let ws_url = format!("http://127.0.0.1:{}", port);

        //
        // Poll for DevTools endpoint to become available.
        //

        let mut connected = false;
        for attempt in 0..30 {
            common::log_debug!(
                "Connection attempt {} to {}",
                attempt + 1,
                ws_url
            );
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            //
            // Try to fetch /json/version to check if DevTools is ready.
            //

            let version_url = format!("http://127.0.0.1:{}/json/version", port);
            if let Ok(response) = reqwest::get(&version_url).await {
                if response.status().is_success() {
                    connected = true;
                    break;
                }
            }
        }

        if !connected {
            return Err(anyhow!(
                "DevTools endpoint not available after 30 seconds"
            ));
        }

        //
        // Connect via chromiumoxide.
        //

        let (browser, mut handler) = Browser::connect(&ws_url).await?;

        //
        // Spawn handler task.
        //

        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    let err_str = e.to_string();
                    // Suppress expected errors during shutdown
                    if err_str.contains("ResetWithoutClosingHandshake")
                        || err_str.contains("Connection reset")
                    {
                        common::log_debug!("Browser connection closed");
                    } else {
                        common::log_error!("Browser handler error: {}", e);
                    }
                }
            }
        });

        //
        // Wait for a page to become available.
        //

        for attempt in 0..30 {
            let pages = browser.pages().await?;
            if let Some(page) = pages.into_iter().next() {
                return Ok(page);
            }
            common::log_debug!(
                "No pages yet, attempt {}/30",
                attempt + 1
            );
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        Err(anyhow!("No pages found in browser after 30 seconds"))
    }

    fn execute_transact(&self, prompt: &str) -> Result<String> {
        let page_guard = self.page.lock().unwrap();
        let page = page_guard
            .as_ref()
            .ok_or_else(|| anyhow!("DevTools page not connected"))?;

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { self.transact_async(page, prompt).await })
        })
    }

    async fn transact_async(&self, page: &Page, prompt: &str) -> Result<String> {
        common::log_info!("starting with prompt length {}", prompt.len());

        let input_selector = self.adapter.input_selector();
        let message_selector = self.adapter.message_selector();

        //
        // Wait for input element to be ready.
        //

        common::log_info!("waiting for input element (selector: {})", input_selector);
        let mut input_ready = false;
        for attempt in 1..=10 {
            match page.find_element(input_selector).await {
                Ok(_) => {
                    input_ready = true;
                    common::log_info!("input element found on attempt {}", attempt);
                    break;
                }
                Err(e) => {
                    common::log_debug!("input element not found, attempt {}/10: {}", attempt, e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }

        if !input_ready {
            //
            // Get page info for debugging.
            //
            let page_title = page.evaluate("document.title")
                .await
                .ok()
                .and_then(|v| v.into_value().ok())
                .and_then(|v: serde_json::Value| v.as_str().map(String::from))
                .unwrap_or_else(|| "unknown".to_string());

            let page_url = page.url().await.unwrap_or_else(|_| None).unwrap_or_else(|| "unknown".to_string());

            common::log_error!(
                "input element not ready after 10 attempts. Selector: '{}', Page title: '{}', Page URL: '{}'",
                input_selector, page_title, page_url
            );
            return Err(anyhow!(
                "Input element '{}' not ready after 10 seconds. Page: '{}' at '{}'",
                input_selector, page_title, page_url
            ));
        }
        common::log_info!("input element ready");

        //
        // Get initial message count.
        //

        let initial_messages = page
            .find_elements(message_selector)
            .await
            .unwrap_or_default();
        let initial_message_count = initial_messages.len();
        common::log_info!("initial message count = {}", initial_message_count);

        //
        // Find input element and send the prompt. Use InsertText CDP command
        // which handles emojis and special characters (emulates IME input).
        //

        common::log_info!("sending prompt");
        let input = page.find_element(input_selector).await?;
        input.click().await?;

        use chromiumoxide::cdp::browser_protocol::input::InsertTextParams;
        page.execute(InsertTextParams::new(prompt)).await?;

        self.adapter.wait_for_submit_ready(page).await?;

        input.press_key("Enter").await?;
        common::log_info!("prompt sent, waiting for response");

        //
        // Poll for response.
        //

        let max_wait_secs = 120;
        let poll_interval_ms = 250;
        let max_iterations = (max_wait_secs * 1000) / poll_interval_ms;

        for _ in 0..max_iterations {
            tokio::time::sleep(std::time::Duration::from_millis(poll_interval_ms as u64)).await;

            //
            // Delegate response completion check to the adapter.
            //

            if let Some(response) = self
                .adapter
                .check_response_complete(page, initial_message_count)
                .await?
            {
                common::log_info!("response received, length = {}", response.len());
                return Ok(response);
            }
        }

        Err(anyhow!("Timed out waiting for response"))
    }
}

impl<A: DevToolsAdapter + 'static> AgentSession for GenericDevToolsSession<A> {
    fn session_id(&self) -> &Uuid {
        &self.session_id
    }

    fn process_path(&self) -> Option<String> {
        self.process_path.clone()
    }

    fn mode(&self) -> AgentMode {
        AgentMode::DevTools
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        self.execute_transact(prompt)
    }

    fn close(&self) {
        if let Some(pid) = self.process_id {
            utils::terminate_process(pid);
        }

        //
        // Close the hidden desktop if one was created.
        //

        #[cfg(windows)]
        {
            let _ = self.hidden_desktop.lock().unwrap().take();
        }
    }
}

impl<A: DevToolsAdapter> GenericDevToolsSession<A> {
    /// Execute JavaScript on the page and return the result as JSON.
    pub fn execute_js(&self, js: &str) -> Result<serde_json::Value> {
        let page_guard = self.page.lock().unwrap();
        let page = page_guard
            .as_ref()
            .ok_or_else(|| anyhow!("DevTools page not connected"))?;

        let js = js.to_string();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let result = page.evaluate(js).await?;
                Ok(result.into_value()?)
            })
        })
    }
}

impl<A: DevToolsAdapter> Drop for GenericDevToolsSession<A> {
    fn drop(&mut self) {
        if let Some(pid) = self.process_id {
            utils::terminate_process(pid);
        }
    }
}
