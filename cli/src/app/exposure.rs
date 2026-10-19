use super::*;
use crate::ui::hits::ExposureHintAction;
use common::exposure::{AgentExposure, ExposureOptions, ExposureReport};

//
// Exposure overlay (Nodes window, `x`). Shows the lethal-trifecta report
// computed by the service across every agent with stored recon: a ranked
// agent list with P/U/X badges, trifecta attack paths and MCP servers /
// hosts shared between agents, each with a drill-down detail pane.
//

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExposureTab {
    Agents,
    Paths,
    Shared,
}

impl ExposureTab {
    pub const ALL: [ExposureTab; 3] =
        [ExposureTab::Agents, ExposureTab::Paths, ExposureTab::Shared];

    pub fn label(&self) -> &'static str {
        match self {
            ExposureTab::Agents => "Agents",
            ExposureTab::Paths => "Paths",
            ExposureTab::Shared => "Shared",
        }
    }
}

pub struct ExposureOverlay {
    pub report: Option<ExposureReport>,
    pub is_loading: bool,
    pub loading_label: &'static str,
    pub error: Option<String>,
    pub tab: ExposureTab,
    pub selected: usize,
    pub detail_focused: bool,
    pub detail_scroll: u16,
    pub detail_max_scroll: Cell<u16>,
    pub status: Option<(String, std::time::Instant)>,

    //
    // Sequence of the in-flight report request; results carrying an older
    // sequence (e.g. a refresh superseded by another) are dropped.
    //
    pub request_seq: u64,

    //
    // A rules edit the service rejected, reopened on the next `e` so the
    // operator can fix it instead of starting over.
    //
    pub rules_draft: Option<String>,
}

impl ExposureOverlay {
    //
    // Agents ranked by risk (highest first), then by id for stability.
    // Shared by the renderer and key handling so indices agree.
    //
    pub fn sorted_agents(report: &ExposureReport) -> Vec<&AgentExposure> {
        let mut agents: Vec<&AgentExposure> = report.agents.iter().collect();
        agents.sort_by(|a, b| b.risk.cmp(&a.risk).then_with(|| a.id.cmp(&b.id)));
        agents
    }

    pub fn item_count(&self, tab: ExposureTab) -> usize {
        let Some(report) = &self.report else {
            return 0;
        };
        match tab {
            ExposureTab::Agents => report.agents.len(),
            ExposureTab::Paths => report.paths.len(),
            ExposureTab::Shared => report.shared.len(),
        }
    }

    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some((message.into(), std::time::Instant::now()));
    }

    fn select(&mut self, index: usize) {
        let count = self.item_count(self.tab);
        self.selected = index.min(count.saturating_sub(1));
        self.detail_scroll = 0;
    }

    pub fn switch_tab(&mut self, tab: ExposureTab) {
        self.tab = tab;
        self.selected = 0;
        self.detail_scroll = 0;
        self.detail_focused = false;
    }

    pub fn select_row(&mut self, index: usize) {
        self.detail_focused = false;
        self.select(index);
    }

    //
    // Mouse wheel: scroll the detail pane when focused, otherwise move the
    // list selection.
    //
    pub fn scroll_wheel(&mut self, delta: i32) {
        if self.detail_focused {
            self.scroll_detail(delta);
        } else {
            let index = (self.selected as i32 + delta).max(0) as usize;
            self.select(index);
        }
    }

    pub fn scroll_detail(&mut self, delta: i32) {
        let max = self.detail_max_scroll.get();
        self.detail_scroll = (self.detail_scroll as i32 + delta).clamp(0, max as i32) as u16;
    }
}

impl App {
    pub(crate) fn open_exposure(&mut self) {
        self.nodes.exposure = Some(ExposureOverlay {
            report: None,
            is_loading: false,
            loading_label: "",
            error: None,
            tab: ExposureTab::Agents,
            selected: 0,
            detail_focused: false,
            detail_scroll: 0,
            detail_max_scroll: Cell::new(0),
            status: None,
            request_seq: 0,
            rules_draft: None,
        });
        self.refresh_exposure(ExposureOptions::default());
    }

    pub(crate) fn refresh_exposure(&mut self, options: ExposureOptions) {
        let Some(overlay) = self.nodes.exposure.as_mut() else {
            return;
        };
        overlay.request_seq += 1;
        overlay.is_loading = true;
        overlay.error = None;
        overlay.loading_label = match (options.probe_configs, options.llm) {
            (true, _) => "probing agent configs…",
            (_, true) => "classifying with LLM…",
            _ => "computing…",
        };
        let seq = overlay.request_seq;
        let client = self.client.clone();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = client
                .request_exposure_report(options)
                .await
                .map(Box::new)
                .map_err(|e| e.to_string());
            if let Some(tx) = tx {
                let _ = tx.send(AppEvent::ExposureReport { seq, result });
            }
        });
    }

    pub(crate) fn apply_exposure_report(
        &mut self,
        seq: u64,
        result: Result<Box<ExposureReport>, String>,
    ) {
        let Some(overlay) = self.nodes.exposure.as_mut() else {
            return;
        };
        if seq != overlay.request_seq {
            return;
        }
        overlay.is_loading = false;
        match result {
            Ok(report) => {
                overlay.report = Some(*report);
                let selected = overlay.selected;
                overlay.select(selected);
            }
            Err(e) => overlay.error = Some(e),
        }
    }

    pub(crate) async fn handle_exposure_key(&mut self, key: KeyEvent) {
        let Some(overlay) = self.nodes.exposure.as_mut() else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Esc => {
                if overlay.detail_focused {
                    overlay.detail_focused = false;
                } else {
                    self.nodes.exposure = None;
                }
            }
            KeyCode::Char('q') if ctrl => self.nodes.exposure = None,
            KeyCode::Tab | KeyCode::BackTab => {
                let idx = ExposureTab::ALL
                    .iter()
                    .position(|t| *t == overlay.tab)
                    .unwrap_or(0);
                let next = if key.code == KeyCode::Tab {
                    (idx + 1) % ExposureTab::ALL.len()
                } else {
                    (idx + ExposureTab::ALL.len() - 1) % ExposureTab::ALL.len()
                };
                overlay.switch_tab(ExposureTab::ALL[next]);
            }
            KeyCode::Char(c @ '1'..='3') if !ctrl => {
                overlay.switch_tab(ExposureTab::ALL[(c as u8 - b'1') as usize]);
            }
            KeyCode::Up if overlay.detail_focused => overlay.scroll_detail(-1),
            KeyCode::Down if overlay.detail_focused => overlay.scroll_detail(1),
            KeyCode::Up => {
                let i = overlay.selected.saturating_sub(1);
                overlay.select(i);
            }
            KeyCode::Down => {
                let i = overlay.selected + 1;
                overlay.select(i);
            }
            KeyCode::PageUp => overlay.scroll_detail(-10),
            KeyCode::PageDown => overlay.scroll_detail(10),
            KeyCode::Right | KeyCode::Enter => {
                if overlay.item_count(overlay.tab) > 0 {
                    overlay.detail_focused = true;
                }
            }
            KeyCode::Left => overlay.detail_focused = false,
            KeyCode::Char('r') if !ctrl => {
                self.dispatch_exposure_hint(ExposureHintAction::Refresh)
                    .await
            }
            KeyCode::Char('p') if !ctrl => {
                self.dispatch_exposure_hint(ExposureHintAction::Probe).await
            }
            KeyCode::Char('l') if !ctrl => {
                self.dispatch_exposure_hint(ExposureHintAction::Llm).await
            }
            KeyCode::Char('x') if !ctrl => {
                self.dispatch_exposure_hint(ExposureHintAction::Export)
                    .await
            }
            KeyCode::Char('e') if !ctrl => {
                self.dispatch_exposure_hint(ExposureHintAction::EditRules)
                    .await
            }
            KeyCode::Char('d') if ctrl => {
                self.confirm = Some(ConfirmAction {
                    message: "Reset exposure rules to built-in defaults?".to_string(),
                    action: ConfirmKind::ResetExposureRules,
                });
            }
            _ => {}
        }
    }

    pub(crate) async fn dispatch_exposure_hint(&mut self, hint: ExposureHintAction) {
        match hint {
            ExposureHintAction::Refresh => self.refresh_exposure(ExposureOptions::default()),
            ExposureHintAction::Probe => self.refresh_exposure(ExposureOptions {
                probe_configs: true,
                ..Default::default()
            }),
            ExposureHintAction::Llm => self.refresh_exposure(ExposureOptions {
                llm: true,
                ..Default::default()
            }),
            ExposureHintAction::EditRules => self.edit_exposure_rules().await,
            ExposureHintAction::Export => self.export_exposure(),
            ExposureHintAction::Close => self.nodes.exposure = None,
        }
    }

    //
    // Write the current report as Graphviz DOT and JSON next to the CLI's
    // working directory.
    //

    fn export_exposure(&mut self) {
        let Some(overlay) = self.nodes.exposure.as_mut() else {
            return;
        };
        let Some(report) = &overlay.report else {
            overlay.set_status("Nothing to export yet");
            return;
        };
        let stem = format!(
            "praxis-exposure-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let json = match serde_json::to_string_pretty(report) {
            Ok(j) => j,
            Err(e) => {
                overlay.set_status(format!("Export failed: {}", e));
                return;
            }
        };
        let written = std::fs::write(format!("{}.dot", stem), report.to_dot())
            .and_then(|_| std::fs::write(format!("{}.json", stem), json));
        match written {
            Ok(()) => overlay.set_status(format!("Exported {}.dot and {}.json", stem, stem)),
            Err(e) => overlay.set_status(format!("Export failed: {}", e)),
        }
    }

    pub(crate) async fn reset_exposure_rules(&mut self) {
        let outcome = self.client.reset_exposure_rules().await;
        if let Some(overlay) = self.nodes.exposure.as_mut() {
            overlay.rules_draft = None;
            match outcome {
                Ok(state) if state.error.is_none() => {
                    overlay.set_status(format!("Rules reset ({} rules)", state.rule_count))
                }
                Ok(state) => {
                    overlay.set_status(format!("Reset failed: {}", state.error.unwrap_or_default()))
                }
                Err(e) => overlay.set_status(format!("Reset failed: {}", e)),
            }
        }
        self.refresh_exposure(ExposureOptions::default());
    }

    //
    // Open the exposure-rules virtual file in $VISUAL / $EDITOR. A file the
    // service rejects is kept as a draft and reopened on the next `e`.
    //

    async fn edit_exposure_rules(&mut self) {
        let draft = self
            .nodes
            .exposure
            .as_mut()
            .and_then(|o| o.rules_draft.take());
        let initial = match draft {
            Some(d) => d,
            None => match self.client.get_exposure_rules().await {
                Ok(state) => state.text,
                Err(e) => {
                    if let Some(o) = self.nodes.exposure.as_mut() {
                        o.set_status(format!("Failed to load rules: {}", e));
                    }
                    return;
                }
            },
        };

        let edited = match self.run_external_editor(&initial, "praxis_exposure_rules_", ".toml") {
            Ok(edited) => edited,
            Err(e) => {
                if let Some(o) = self.nodes.exposure.as_mut() {
                    o.set_status(e);
                }
                return;
            }
        };
        let Some(text) = edited.filter(|t| *t != initial) else {
            if let Some(o) = self.nodes.exposure.as_mut() {
                o.set_status("No changes");
            }
            return;
        };

        let outcome = self.client.set_exposure_rules(text.clone()).await;
        let Some(overlay) = self.nodes.exposure.as_mut() else {
            return;
        };
        match outcome {
            Ok(state) if state.error.is_none() => {
                overlay.set_status(format!("Saved ({} rules)", state.rule_count));
                self.refresh_exposure(ExposureOptions::default());
            }
            Ok(state) => {
                overlay.rules_draft = Some(text);
                overlay.set_status(format!(
                    "Rules rejected: {} (press e to fix)",
                    state.error.unwrap_or_default()
                ));
            }
            Err(e) => {
                overlay.rules_draft = Some(text);
                overlay.set_status(format!("Save failed: {}", e));
            }
        }
    }

    //
    // Suspend the TUI, edit `initial` in the user's editor and return the
    // saved text (None when the editor exited unsuccessfully). Same
    // terminal hand-off as the other editor flows.
    //

    fn run_external_editor(
        &mut self,
        initial: &str,
        prefix: &str,
        suffix: &str,
    ) -> Result<Option<String>, String> {
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
            .prefix(prefix)
            .suffix(suffix)
            .tempfile()
            .map_err(|e| format!("Failed to create temp file: {}", e))?;
        tmp.as_file()
            .write_all(initial.as_bytes())
            .map_err(|e| format!("Failed to write temp file: {}", e))?;

        self.terminal_paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crossterm::terminal::disable_raw_mode().ok();
        crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen,
        )
        .ok();

        let status = std::process::Command::new(&editor).arg(tmp.path()).status();

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
        while crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
            let _ = crossterm::event::read();
        }
        self.needs_full_redraw = true;

        match status {
            Ok(s) if s.success() => std::fs::read_to_string(tmp.path())
                .map(Some)
                .map_err(|e| format!("Failed to read edited file: {}", e)),
            Ok(_) => Ok(None),
            Err(e) => Err(format!("Failed to launch editor '{}': {}", editor, e)),
        }
    }
}
