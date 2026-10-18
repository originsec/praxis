use super::*;

//
// Orchestrator playbook management on the Settings > Orchestrator tab:
// list, view/edit in $EDITOR, create from a template, delete. Documents are
// validated as SKILL.md before they are sent to the service.
//

impl App {
    pub(crate) async fn load_playbooks(&mut self) {
        match self.client.list_playbooks().await {
            Ok(playbooks) => {
                self.settings.playbooks = playbooks;
                self.settings.playbooks_loaded = true;
            }
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to load playbooks: {}", e));
                self.settings.status_message_at = Some(std::time::Instant::now());
            }
        }
    }

    //
    // Open a playbook (or a new-playbook template) in the external editor
    // and save the result. A document that fails validation is kept as a
    // draft and reopened next time instead of being lost. Renaming a
    // playbook in the editor saves it under the new name and removes the
    // old one.
    //
    pub(crate) async fn edit_playbook_in_editor(&mut self, existing: Option<common::PlaybookInfo>) {
        let original_name = existing.as_ref().map(|p| p.name.clone());
        let initial = match self.settings.playbook_draft.take() {
            Some((draft_for, draft)) if draft_for == original_name => draft,
            _ => existing
                .as_ref()
                .map(|p| p.content.clone())
                .unwrap_or_else(|| common::playbook::playbook_template("new-playbook")),
        };

        let edited = match self.run_external_editor(&initial, ".md") {
            Ok(text) => text,
            Err(e) => {
                self.set_settings_status(e);
                return;
            }
        };

        if edited.trim().is_empty() {
            self.set_settings_status("Empty file — not saved".to_string());
            return;
        }
        if existing.as_ref().is_some_and(|p| p.content == edited) {
            self.set_settings_status("No changes".to_string());
            return;
        }

        let parsed = match common::playbook::parse_skill_md(&edited) {
            Ok(p) => p,
            Err(e) => {
                self.settings.playbook_draft = Some((original_name, edited));
                self.set_settings_status(format!(
                    "Failed: invalid SKILL.md ({}). Press Enter again to fix the draft.",
                    e
                ));
                return;
            }
        };

        match self.client.save_playbook(edited.clone()).await {
            Ok(name) => {
                if let Some(old) = original_name.filter(|old| *old != parsed.name)
                    && let Err(e) = self.client.delete_playbook(&old).await
                {
                    self.set_settings_status(format!(
                        "Saved '{}' but failed to remove old '{}': {}",
                        name, old, e
                    ));
                } else {
                    self.set_settings_status(format!("Saved playbook '{}'", name));
                }
                self.load_playbooks().await;
            }
            Err(e) => {
                self.settings.playbook_draft = Some((original_name, edited));
                self.set_settings_status(format!("Failed: {}", e));
            }
        }
    }

    pub(crate) async fn delete_playbook(&mut self, name: &str) {
        match self.client.delete_playbook(name).await {
            Ok(true) => self.set_settings_status(format!("Deleted playbook '{}'", name)),
            Ok(false) => self.set_settings_status(format!("Failed: no playbook '{}'", name)),
            Err(e) => self.set_settings_status(format!("Failed: {}", e)),
        }
        self.load_playbooks().await;
        let max = self.settings_item_count().saturating_sub(1);
        self.settings.selected = self.settings.selected.min(max);
    }

    fn set_settings_status(&mut self, message: String) {
        self.settings.status_message = Some(message);
        self.settings.status_message_at = Some(std::time::Instant::now());
    }

    //
    // Suspend the TUI, edit `initial` in $VISUAL/$EDITOR via a temp file and
    // return the saved text.
    //
    fn run_external_editor(&mut self, initial: &str, suffix: &str) -> Result<String, String> {
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

        let tmp = tempfile::Builder::new()
            .prefix("praxis-playbook")
            .suffix(suffix)
            .tempfile()
            .map_err(|e| format!("Failed to create temp file: {}", e))?;
        tmp.as_file()
            .write_all(initial.as_bytes())
            .map_err(|e| format!("Failed to write temp file: {}", e))?;
        let path = tmp.path().to_path_buf();

        self.terminal_paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crossterm::terminal::disable_raw_mode().ok();
        crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen,
        )
        .ok();

        let status = std::process::Command::new(&editor).arg(&path).status();

        crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )
        .ok();
        crossterm::terminal::enable_raw_mode().ok();
        self.terminal_paused
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.terminal_resume.notify_one();

        //
        // Drop keypresses buffered while the editor ran (e.g. the Enter
        // from :wq) so they are not replayed into the TUI.
        //
        while crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
            let _ = crossterm::event::read();
        }
        self.needs_full_redraw = true;

        match status {
            Ok(s) if s.success() => {
                std::fs::read_to_string(&path).map_err(|e| format!("Failed to read file: {}", e))
            }
            Ok(_) => Err("Editor exited with error".to_string()),
            Err(e) => Err(format!("Failed to launch editor '{}': {}", editor, e)),
        }
    }
}
