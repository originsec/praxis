use super::*;

impl App {
    pub(crate) async fn load_agent_scripts(&mut self) {
        if let Err(e) = self.client.request_lua_agent_scripts().await {
            self.settings.status_message = Some(format!("Failed to request scripts: {}", e));
        }
    }

    pub(crate) fn poll_agent_scripts(&mut self, scripts: Vec<common::LuaAgentScriptInfo>) {
        let mut scripts = scripts;
        scripts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.settings.agent_scripts = scripts;
        self.settings.agent_scripts_loaded = true;
    }

    pub(crate) async fn edit_agent_script_in_editor(
        &mut self,
        existing: Option<common::LuaAgentScriptInfo>,
    ) {
        use std::io::Write;

        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| {
                if cfg!(windows) {
                    "notepad".to_string()
                } else {
                    "vi".to_string()
                }
            });

        let extension = ".lua";
        let prefix = existing
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or("new_agent");
        let tmp = match tempfile::Builder::new()
            .prefix(prefix)
            .suffix(extension)
            .tempfile()
        {
            Ok(f) => f,
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to create temp file: {}", e));
                self.settings.status_message_at = Some(std::time::Instant::now());
                return;
            }
        };

        if let Some(ref script) = existing {
            if let Err(e) = tmp.as_file().write_all(script.script.as_bytes()) {
                self.settings.status_message = Some(format!("Failed to write temp file: {}", e));
                self.settings.status_message_at = Some(std::time::Instant::now());
                return;
            }
        }

        let path = tmp.path().to_path_buf();

        //
        // Pause the event reader and suspend the terminal so the editor
        // can take over stdin/stdout without interference.
        //

        self.terminal_paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crossterm::terminal::disable_raw_mode().ok();
        crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen).ok();

        let status = std::process::Command::new(&editor).arg(&path).status();

        crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )
        .ok();
        crossterm::terminal::enable_raw_mode().ok();
        self.terminal_paused
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.terminal_resume.notify_one();

        //
        // Drain any buffered terminal events so stale keypresses from the
        // editor (e.g. the Enter from :q!) don't get processed by the TUI.
        //

        while crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
            let _ = crossterm::event::read();
        }

        self.needs_full_redraw = true;

        match status {
            Ok(s) if s.success() => {
                match std::fs::read_to_string(&path) {
                    Ok(content) if content.trim().is_empty() => {
                        self.settings.status_message = Some("Empty file — not saved".to_string());
                    }
                    Ok(content) => {
                        let result = if let Some(ref script) = existing {
                            self.client
                                .update_lua_agent_script(
                                    script.id.clone(),
                                    script.name.clone(),
                                    content,
                                )
                                .await
                        } else {
                            //
                            // Derive name from filename stem of the temp file,
                            // or ask user. For simplicity, derive from content.
                            //
                            let name = Self::derive_agent_script_name(&path);
                            self.client.add_lua_agent_script(name, content).await
                        };
                        match result {
                            Ok(_) => {
                                self.settings.status_message = Some("Saved".to_string());
                                self.settings.agent_scripts_loaded = false;
                                self.load_agent_scripts().await;
                            }
                            Err(e) => {
                                self.settings.status_message =
                                    Some(format!("Upload failed: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        self.settings.status_message = Some(format!("Failed to read file: {}", e));
                    }
                }
            }
            Ok(_) => {
                self.settings.status_message = Some("Editor exited with error".to_string());
            }
            Err(e) => {
                self.settings.status_message =
                    Some(format!("Failed to launch editor '{}': {}", editor, e));
            }
        }
        self.settings.status_message_at = Some(std::time::Instant::now());
    }

    pub(crate) fn derive_agent_script_name(path: &std::path::Path) -> String {
        //
        // Try to extract an agent_name from the Lua source. Fall back to
        // the filename stem (without the random suffix tempfile adds).
        //

        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("agent_name") {
                    let rest = rest.trim_start().trim_start_matches('=').trim();
                    let name = rest.trim_matches('"').trim_matches('\'').trim_matches(',');
                    if !name.is_empty() {
                        return name.to_string();
                    }
                }
            }
        }

        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("new_agent")
            .to_string()
    }
}
