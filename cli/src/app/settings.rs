mod model_form;

use super::*;

pub use self::model_form::ModelEditForm;

#[derive(Clone, Copy, PartialEq)]
pub enum SettingsTab {
    Llm,
    Agents,
    Intercept,
    Service,
    About,
}

pub struct SettingsState {
    pub tab: SettingsTab,
    pub selected: usize,
    pub editing: bool,
    pub edit_buffer: String,
    pub loaded: bool,
    pub status_message: Option<String>,
    pub status_message_at: Option<std::time::Instant>,

    //
    // LLM settings.
    //
    pub model_definitions: Vec<ModelDef>,
    pub model_form: Option<ModelEditForm>,
    pub orchestrator_model: String,
    pub orchestrator_max_tokens: String,
    pub semantic_ops_model: String,
    pub semantic_parser_model: String,
    pub traffic_parser_model: String,
    pub praxis_agent_model_ref: String,
    pub praxis_agent_thinking_effort: String,
    pub praxis_agent_enabled: bool,
    pub praxis_agent_system_prompt: String,
    pub praxis_agent_prompt_editing: bool,
    pub praxis_agent_prompt_buffer: String,

    //
    // Service settings.
    //
    pub mcp_enabled: bool,
    pub mcp_port: String,
    pub logging_enabled: bool,
    pub log_query_row_limit: String,
    pub prompt_timeout_secs: String,

    //
    // Claude Bridge settings.
    //
    pub claude_ccrv1_enabled: bool,
    pub claude_ccrv1_port: String,
    pub claude_ccrv2_enabled: bool,
    pub claude_ccrv2_port: String,

    //
    // Model select dropdown for feature assignments.
    //
    pub dropdown_open: bool,
    pub dropdown_selected: usize,
    pub dropdown_field: usize, // which model assignment field (1-6) the dropdown is for

    //
    // Agent scripts.
    //
    pub agent_scripts: Vec<common::LuaAgentScriptInfo>,
    pub agent_scripts_loaded: bool,

    //
    // Intercept targets (URLs/filters pushed to nodes). The virtual file
    // lives as TOML text on the service; we cache the parsed list and
    // any parse error here for the Intercept settings tab.
    //
    pub intercept_targets: Vec<common::InterceptTargetConfig>,
    pub intercept_targets_text: String,
    pub intercept_targets_error: Option<String>,
    pub intercept_targets_loaded: bool,

    //
    // Connection info (read-only, set at startup).
    //
    pub rabbitmq_url: String,
    pub client_id: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelDef {
    pub name: String,
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey", default)]
    pub api_key: String,
    #[serde(rename = "baseUrl", default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

pub fn sorted_providers() -> Vec<common::Provider> {
    let mut providers = common::Provider::all();
    providers.sort_by(|a, b| {
        a.display_name()
            .to_lowercase()
            .cmp(&b.display_name().to_lowercase())
    });
    providers
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            tab: SettingsTab::Llm,
            selected: 0,
            editing: false,
            edit_buffer: String::new(),
            loaded: false,
            status_message: None,
            status_message_at: None,
            model_definitions: Vec::new(),
            model_form: None,
            orchestrator_model: String::new(),
            orchestrator_max_tokens: "25000".to_string(),
            semantic_ops_model: String::new(),
            semantic_parser_model: String::new(),
            traffic_parser_model: String::new(),
            praxis_agent_model_ref: String::new(),
            praxis_agent_thinking_effort: String::new(),
            praxis_agent_enabled: false,
            praxis_agent_system_prompt: String::new(),
            praxis_agent_prompt_editing: false,
            praxis_agent_prompt_buffer: String::new(),
            mcp_enabled: true,
            mcp_port: "8585".to_string(),
            logging_enabled: false,
            log_query_row_limit: "10000000".to_string(),
            prompt_timeout_secs: "600".to_string(),
            claude_ccrv1_enabled: false,
            claude_ccrv1_port: "8586".to_string(),
            claude_ccrv2_enabled: false,
            claude_ccrv2_port: "8587".to_string(),
            agent_scripts: Vec::new(),
            agent_scripts_loaded: false,
            intercept_targets: Vec::new(),
            intercept_targets_text: String::new(),
            intercept_targets_error: None,
            intercept_targets_loaded: false,
            dropdown_open: false,
            dropdown_selected: 0,
            dropdown_field: 0,
            rabbitmq_url: String::new(),
            client_id: String::new(),
        }
    }
}

impl App {
    pub(crate) async fn load_settings(&mut self) {
        let keys = vec![
            "llm_model_definitions".to_string(),
            "llm_feature_orchestrator".to_string(),
            "llm_orchestrator_max_tokens".to_string(),
            "llm_feature_semantic_ops".to_string(),
            "llm_feature_semantic_parser".to_string(),
            "llm_feature_traffic_parser".to_string(),
            "mcp_server_enabled".to_string(),
            "mcp_server_port".to_string(),
            "application_logs_enabled".to_string(),
            "log_query_row_limit".to_string(),
            "prompt_timeout_secs".to_string(),
            "claude_ccrv1_enabled".to_string(),
            "claude_ccrv1_port".to_string(),
            "claude_ccrv2_enabled".to_string(),
            "claude_ccrv2_port".to_string(),
            "praxis_agent_settings".to_string(),
            "praxis_agent_system_prompt".to_string(),
        ];

        match self.client.get_config(keys).await {
            Ok(config) => {
                let s = &mut self.settings;

                let defs_json = config
                    .get("llm_model_definitions")
                    .cloned()
                    .unwrap_or_default();
                s.model_definitions = serde_json::from_str(&defs_json).unwrap_or_default();
                s.model_definitions
                    .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

                s.orchestrator_model = config
                    .get("llm_feature_orchestrator")
                    .cloned()
                    .unwrap_or_default();
                s.orchestrator_max_tokens = config
                    .get("llm_orchestrator_max_tokens")
                    .cloned()
                    .unwrap_or("25000".to_string());
                s.semantic_ops_model = config
                    .get("llm_feature_semantic_ops")
                    .cloned()
                    .unwrap_or_default();
                s.semantic_parser_model = config
                    .get("llm_feature_semantic_parser")
                    .cloned()
                    .unwrap_or_default();
                s.traffic_parser_model = config
                    .get("llm_feature_traffic_parser")
                    .cloned()
                    .unwrap_or_default();
                s.mcp_enabled = config
                    .get("mcp_server_enabled")
                    .map(|v| v != "false" && v != "0" && v != "no")
                    .unwrap_or(true);
                s.mcp_port = config
                    .get("mcp_server_port")
                    .cloned()
                    .unwrap_or("8585".to_string());
                s.logging_enabled = config
                    .get("application_logs_enabled")
                    .map(|v| v == "true" || v == "1" || v == "yes")
                    .unwrap_or(false);
                s.log_query_row_limit = config
                    .get("log_query_row_limit")
                    .cloned()
                    .unwrap_or("10000000".to_string());
                s.prompt_timeout_secs = config
                    .get("prompt_timeout_secs")
                    .cloned()
                    .unwrap_or("600".to_string());
                s.claude_ccrv1_enabled = config
                    .get("claude_ccrv1_enabled")
                    .map(|v| v == "true" || v == "1" || v == "yes")
                    .unwrap_or(false);
                s.claude_ccrv1_port = config
                    .get("claude_ccrv1_port")
                    .cloned()
                    .unwrap_or("8586".to_string());
                s.claude_ccrv2_enabled = config
                    .get("claude_ccrv2_enabled")
                    .map(|v| v == "true" || v == "1" || v == "yes")
                    .unwrap_or(false);
                s.claude_ccrv2_port = config
                    .get("claude_ccrv2_port")
                    .cloned()
                    .unwrap_or("8587".to_string());

                s.praxis_agent_model_ref.clear();
                s.praxis_agent_thinking_effort.clear();
                s.praxis_agent_enabled = false;
                if let Some(settings_json) = config.get("praxis_agent_settings") {
                    if let Ok(settings) = serde_json::from_str::<serde_json::Value>(settings_json) {
                        s.praxis_agent_model_ref = settings
                            .get("modelRef")
                            .or_else(|| settings.get("model_ref"))
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        s.praxis_agent_thinking_effort = settings
                            .get("thinkingEffort")
                            .or_else(|| settings.get("thinking_effort"))
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        s.praxis_agent_enabled = settings
                            .get("enabled")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                    }
                }
                s.praxis_agent_system_prompt = config
                    .get("praxis_agent_system_prompt")
                    .cloned()
                    .unwrap_or_default();
                s.praxis_agent_prompt_editing = false;
                s.praxis_agent_prompt_buffer.clear();

                s.loaded = true;
                s.status_message = None;
            }
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to load settings: {}", e));
            }
        }
    }

    pub(crate) async fn save_setting(&mut self, key: &str, value: &str) {
        let mut values = HashMap::new();
        values.insert(key.to_string(), value.to_string());
        if let Err(e) = self.client.set_config(values).await {
            self.settings.status_message = Some(format!("Save failed: {}", e));
        } else {
            self.settings.status_message = Some("Saved".to_string());
        }
        self.settings.status_message_at = Some(std::time::Instant::now());
    }

    pub(crate) async fn save_praxis_agent_settings(&mut self) {
        let settings = serde_json::json!({
            "modelRef": self.settings.praxis_agent_model_ref.clone(),
            "thinkingEffort": self.settings.praxis_agent_thinking_effort.clone(),
            "enabled": self.settings.praxis_agent_enabled,
        })
        .to_string();
        self.save_setting("praxis_agent_settings", &settings).await;
    }

    pub(crate) async fn edit_praxis_agent_system_prompt(&mut self) {
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

        let tmp = match tempfile::Builder::new()
            .prefix("praxis_agent_system_prompt")
            .suffix(".md")
            .tempfile()
        {
            Ok(f) => f,
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to create temp file: {}", e));
                self.settings.status_message_at = Some(std::time::Instant::now());
                return;
            }
        };

        self.settings.praxis_agent_prompt_editing = true;
        self.settings.praxis_agent_prompt_buffer = self.settings.praxis_agent_system_prompt.clone();

        if let Err(e) = tmp
            .as_file()
            .write_all(self.settings.praxis_agent_prompt_buffer.as_bytes())
        {
            self.settings.status_message = Some(format!("Failed to write temp file: {}", e));
            self.settings.status_message_at = Some(std::time::Instant::now());
            self.settings.praxis_agent_prompt_editing = false;
            return;
        }

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

        while crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
            let _ = crossterm::event::read();
        }

        self.needs_full_redraw = true;
        self.settings.praxis_agent_prompt_editing = false;

        match status {
            Ok(s) if s.success() => match std::fs::read_to_string(&path) {
                Ok(content) => {
                    self.settings.praxis_agent_prompt_buffer = content.clone();
                    self.settings.praxis_agent_system_prompt = content.clone();
                    self.save_setting("praxis_agent_system_prompt", &content)
                        .await;
                }
                Err(e) => {
                    self.settings.status_message = Some(format!("Failed to read file: {}", e));
                    self.settings.status_message_at = Some(std::time::Instant::now());
                }
            },
            Ok(_) => {
                self.settings.status_message = Some("Editor exited with error".to_string());
                self.settings.status_message_at = Some(std::time::Instant::now());
            }
            Err(e) => {
                self.settings.status_message =
                    Some(format!("Failed to launch editor '{}': {}", editor, e));
                self.settings.status_message_at = Some(std::time::Instant::now());
            }
        }
    }

    pub(crate) fn settings_item_count(&self) -> usize {
        match self.settings.tab {
            SettingsTab::Llm => {
                //
                // Items: one row per model definition, then feature assignments
                // and max tokens.
                // Layout: [models...] + add_model + feature assignments.
                //
                self.settings.model_definitions.len() + 6
            }
            SettingsTab::Agents => {
                // Praxis Agent (4 items) + scripts list + "Add new" + "Reset defaults"
                self.settings.agent_scripts.len() + 6
            }
            SettingsTab::Intercept => {
                //
                // Target rows are non-selectable; only the two action
                // rows (edit virtual file, reset to defaults) count.
                //
                2
            }
            SettingsTab::Service => 9, // mcp_enabled, mcp_port, logging, log_query_row_limit, prompt_timeout_secs, ccrv1_enabled, ccrv1_port, ccrv2_enabled, ccrv2_port
            SettingsTab::About => 0,
        }
    }

    pub(crate) fn is_text_editable_field(&self) -> bool {
        let sel = self.settings.selected;
        match self.settings.tab {
            SettingsTab::Llm => {
                let mc = self.settings.model_definitions.len();
                // mc+2 = Orchestrator Max Tokens.
                sel == mc + 2
            }
            SettingsTab::Agents => {
                // 1 = Praxis thinking effort.
                sel == 1
            }
            SettingsTab::Intercept => false,
            SettingsTab::Service => {
                // 1 = MCP port, 3 = log query row limit, 4 = prompt timeout,
                // 6 = CCRv1 port, 8 = CCRv2 port
                sel == 1 || sel == 3 || sel == 4 || sel == 6 || sel == 8
            }
            SettingsTab::About => false,
        }
    }

    pub(crate) async fn apply_dropdown_selection(&mut self) {
        if let Some(def) = self
            .settings
            .model_definitions
            .get(self.settings.dropdown_selected)
        {
            let name = def.name.clone();
            let field = self.settings.dropdown_field;
            match field {
                0 => {
                    self.settings.praxis_agent_model_ref = name.clone();
                    self.save_praxis_agent_settings().await;
                }
                1 => {
                    self.settings.orchestrator_model = name.clone();
                    self.save_setting("llm_feature_orchestrator", &name).await;
                }
                3 => {
                    self.settings.semantic_ops_model = name.clone();
                    self.save_setting("llm_feature_semantic_ops", &name).await;
                }
                4 => {
                    self.settings.semantic_parser_model = name.clone();
                    self.save_setting("llm_feature_semantic_parser", &name)
                        .await;
                }
                5 => {
                    self.settings.traffic_parser_model = name.clone();
                    self.save_setting("llm_feature_traffic_parser", &name).await;
                }
                6 => {
                    self.settings.praxis_agent_model_ref = name.clone();
                    self.save_praxis_agent_settings().await;
                }
                _ => {}
            }
        }
        self.settings.dropdown_open = false;
    }

    pub(crate) fn open_url(url: &str) {
        let cmd = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(target_os = "windows") {
            "cmd"
        } else {
            "xdg-open"
        };

        let mut command = std::process::Command::new(cmd);
        if cfg!(target_os = "windows") {
            command.args(["/C", "start", url]);
        } else {
            command.arg(url);
        }
        let _ = command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    pub(crate) fn auto_enter_edit(&mut self) {
        if self.is_text_editable_field() {
            let val = self.current_field_value();
            self.settings.editing = true;
            self.settings.edit_buffer = val;
        }
    }

    pub(crate) fn current_field_value(&self) -> String {
        let sel = self.settings.selected;
        match self.settings.tab {
            SettingsTab::Llm => {
                let mc = self.settings.model_definitions.len();
                if sel == mc + 2 {
                    self.settings.orchestrator_max_tokens.clone()
                } else {
                    String::new()
                }
            }
            SettingsTab::Agents => {
                if sel == 1 {
                    self.settings.praxis_agent_thinking_effort.clone()
                } else {
                    String::new()
                }
            }
            SettingsTab::Intercept => String::new(),
            SettingsTab::Service => match sel {
                1 => self.settings.mcp_port.clone(),
                3 => self.settings.log_query_row_limit.clone(),
                4 => self.settings.prompt_timeout_secs.clone(),
                6 => self.settings.claude_ccrv1_port.clone(),
                8 => self.settings.claude_ccrv2_port.clone(),
                _ => String::new(),
            },
            SettingsTab::About => String::new(),
        }
    }

    pub(crate) async fn switch_settings_tab(&mut self, tab: SettingsTab) {
        self.settings.tab = tab;
        self.settings.selected = 0;
        if self.settings.tab == SettingsTab::Agents && !self.settings.agent_scripts_loaded {
            self.load_agent_scripts().await;
        }
        if self.settings.tab == SettingsTab::Intercept && !self.settings.intercept_targets_loaded {
            self.load_intercept_targets().await;
        }
    }

    pub(crate) async fn load_intercept_targets(&mut self) {
        if let Err(e) = self.client.request_intercept_targets().await {
            self.settings.status_message = Some(format!("Failed to request targets: {}", e));
            self.settings.status_message_at = Some(std::time::Instant::now());
        }
    }

    pub(crate) async fn poll_intercept_targets(
        &mut self,
        targets: Vec<common::InterceptTargetConfig>,
    ) {
        self.settings.intercept_targets = targets;
        self.settings.intercept_targets_text = self.client.get_intercept_targets_text().await;
        self.settings.intercept_targets_error = self.client.get_intercept_targets_error().await;
        self.settings.intercept_targets_loaded = true;
    }

    pub(crate) async fn handle_settings_key(&mut self, key: KeyEvent) {
        //
        // If editing a field, capture input.
        //

        if self.settings.editing {
            match key.code {
                KeyCode::Esc => {
                    self.settings.editing = false;
                    self.settings.edit_buffer.clear();
                }
                KeyCode::Enter => {
                    let val = self.settings.edit_buffer.clone();
                    self.settings.editing = false;
                    self.apply_settings_edit(val).await;
                }
                KeyCode::Up => {
                    let val = self.settings.edit_buffer.clone();
                    self.settings.editing = false;
                    self.apply_settings_edit(val).await;
                    if self.settings.selected > 0 {
                        self.settings.selected -= 1;
                        self.auto_enter_edit();
                    }
                }
                KeyCode::Down => {
                    let val = self.settings.edit_buffer.clone();
                    self.settings.editing = false;
                    self.apply_settings_edit(val).await;
                    let max = self.settings_item_count();
                    if max > 0 && self.settings.selected < max - 1 {
                        self.settings.selected += 1;
                        self.auto_enter_edit();
                    }
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let val = self.settings.edit_buffer.clone();
                    self.settings.editing = false;
                    self.apply_settings_edit(val).await;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.settings.edit_buffer.push(c);
                }
                KeyCode::Backspace => {
                    self.settings.edit_buffer.pop();
                }
                _ => {}
            }
            return;
        }

        //
        // If dropdown is open for model selection.
        //

        if self.settings.dropdown_open {
            let count = self.settings.model_definitions.len();
            match key.code {
                KeyCode::Esc => {
                    self.settings.dropdown_open = false;
                }
                KeyCode::Up => {
                    if self.settings.dropdown_selected > 0 {
                        self.settings.dropdown_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if count > 0 && self.settings.dropdown_selected < count - 1 {
                        self.settings.dropdown_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    self.apply_dropdown_selection().await;
                }
                _ => {}
            }
            return;
        }

        //
        // If model edit form is open, delegate to it.
        //

        if self.settings.model_form.is_some() {
            self.handle_model_form_key(key).await;
            return;
        }

        match key.code {
            KeyCode::Tab => {
                let next_tab = match self.settings.tab {
                    SettingsTab::Llm => SettingsTab::Agents,
                    SettingsTab::Agents => SettingsTab::Intercept,
                    SettingsTab::Intercept => SettingsTab::Service,
                    SettingsTab::Service => SettingsTab::About,
                    SettingsTab::About => SettingsTab::Llm,
                };
                self.switch_settings_tab(next_tab).await;
            }
            KeyCode::BackTab => {
                let next_tab = match self.settings.tab {
                    SettingsTab::Llm => SettingsTab::About,
                    SettingsTab::Agents => SettingsTab::Llm,
                    SettingsTab::Intercept => SettingsTab::Agents,
                    SettingsTab::Service => SettingsTab::Intercept,
                    SettingsTab::About => SettingsTab::Service,
                };
                self.switch_settings_tab(next_tab).await;
            }
            KeyCode::Up => {
                if self.settings.selected > 0 {
                    self.settings.selected -= 1;
                    self.auto_enter_edit();
                }
            }
            KeyCode::Down => {
                let max = self.settings_item_count();
                if max > 0 && self.settings.selected < max - 1 {
                    self.settings.selected += 1;
                    self.auto_enter_edit();
                }
            }
            KeyCode::Enter => {
                self.activate_settings_item().await;
            }
            KeyCode::Char('d')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.settings.tab == SettingsTab::Llm =>
            {
                let sel = self.settings.selected;
                if sel < self.settings.model_definitions.len() {
                    let name = self.settings.model_definitions[sel].name.clone();
                    self.confirm = Some(ConfirmAction {
                        message: format!("Delete model '{}'?", name),
                        action: ConfirmKind::DeleteModel(sel),
                    });
                }
            }
            KeyCode::Char(' ') if self.settings.tab == SettingsTab::Agents => {
                let sel = self.settings.selected;
                if sel >= 4 && sel < 4 + self.settings.agent_scripts.len() {
                    let script = &self.settings.agent_scripts[sel - 4];
                    let id = script.id.clone();
                    let new_disabled = !script.disabled;
                    let _ = self
                        .client
                        .toggle_lua_agent_script_disabled(id, new_disabled)
                        .await;
                    self.settings.agent_scripts_loaded = false;
                    self.load_agent_scripts().await;
                }
            }
            KeyCode::Char('d')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.settings.tab == SettingsTab::Agents =>
            {
                let sel = self.settings.selected;
                if sel >= 4 && sel < 4 + self.settings.agent_scripts.len() {
                    let script = &self.settings.agent_scripts[sel - 4];
                    let name = script.name.clone();
                    let id = script.id.clone();
                    self.confirm = Some(ConfirmAction {
                        message: format!("Delete agent script '{}'?", name),
                        action: ConfirmKind::DeleteAgentScript(id),
                    });
                }
            }
            _ => {}
        }
    }

    pub(crate) async fn activate_settings_item(&mut self) {
        let sel = self.settings.selected;
        match self.settings.tab {
            SettingsTab::Llm => {
                let model_count = self.settings.model_definitions.len();
                if sel < model_count {
                    self.open_model_form(Some(sel));
                } else {
                    let idx = sel - model_count;
                    match idx {
                        0 => {
                            self.open_model_form(None);
                        }
                        1 | 3 | 4 | 5 => {
                            //
                            // Model assignment fields — open dropdown.
                            //
                            let current = match idx {
                                1 => &self.settings.orchestrator_model,
                                3 => &self.settings.semantic_ops_model,
                                4 => &self.settings.semantic_parser_model,
                                5 => &self.settings.traffic_parser_model,
                                _ => unreachable!(),
                            };
                            let pos = self
                                .settings
                                .model_definitions
                                .iter()
                                .position(|d| d.name == *current)
                                .unwrap_or(0);
                            self.settings.dropdown_open = true;
                            self.settings.dropdown_selected = pos;
                            self.settings.dropdown_field = idx;
                        }
                        2 => {
                            // Max tokens — free text edit.
                            self.settings.editing = true;
                            self.settings.edit_buffer =
                                self.settings.orchestrator_max_tokens.clone();
                        }
                        _ => {}
                    }
                }
            }
            SettingsTab::Agents => {
                let script_count = self.settings.agent_scripts.len();
                if sel < 4 {
                    match sel {
                        0 => {
                            // Praxis model — open dropdown.
                            let current = &self.settings.praxis_agent_model_ref;
                            let pos = self
                                .settings
                                .model_definitions
                                .iter()
                                .position(|d| d.name == *current)
                                .unwrap_or(0);
                            self.settings.dropdown_open = true;
                            self.settings.dropdown_selected = pos;
                            self.settings.dropdown_field = 0;
                        }
                        1 => {
                            // Praxis thinking effort — free text edit.
                            self.settings.editing = true;
                            self.settings.edit_buffer =
                                self.settings.praxis_agent_thinking_effort.clone();
                        }
                        2 => {
                            self.settings.praxis_agent_enabled =
                                !self.settings.praxis_agent_enabled;
                            self.save_praxis_agent_settings().await;
                        }
                        3 => {
                            self.edit_praxis_agent_system_prompt().await;
                        }
                        _ => {}
                    }
                } else if sel < 4 + script_count {
                    //
                    // Edit existing script — open in external editor.
                    //
                    let script = self.settings.agent_scripts[sel - 4].clone();
                    self.edit_agent_script_in_editor(Some(script)).await;
                } else {
                    let idx = sel - 4 - script_count;
                    match idx {
                        0 => {
                            // Add new script.
                            self.edit_agent_script_in_editor(None).await;
                        }
                        1 => {
                            // Reset defaults.
                            self.confirm = Some(ConfirmAction {
                                message: "Reset all agent scripts to built-in defaults?"
                                    .to_string(),
                                action: ConfirmKind::ResetAgentScripts,
                            });
                        }
                        _ => {}
                    }
                }
            }
            SettingsTab::Intercept => {
                //
                // 0 = edit virtual file, 1 = reset to defaults. Target
                // rows are not selectable; the parsed list is shown for
                // reference only.
                //
                match sel {
                    0 => self.edit_intercept_targets_in_editor().await,
                    1 => {
                        self.confirm = Some(ConfirmAction {
                            message: "Reset intercept targets to built-in defaults?".to_string(),
                            action: ConfirmKind::ResetInterceptTargets,
                        });
                    }
                    _ => {}
                }
            }
            SettingsTab::Service => {
                match sel {
                    0 => {
                        // Toggle MCP enabled.
                        self.settings.mcp_enabled = !self.settings.mcp_enabled;
                        let val = if self.settings.mcp_enabled {
                            "true"
                        } else {
                            "false"
                        };
                        self.save_setting("mcp_server_enabled", val).await;
                    }
                    1 => {
                        // Edit MCP port.
                        self.settings.editing = true;
                        self.settings.edit_buffer = self.settings.mcp_port.clone();
                    }
                    2 => {
                        // Toggle logging enabled.
                        self.settings.logging_enabled = !self.settings.logging_enabled;
                        let val = if self.settings.logging_enabled {
                            "true"
                        } else {
                            "false"
                        };
                        self.save_setting("application_logs_enabled", val).await;
                    }
                    3 => {
                        // Edit log query row limit.
                        self.settings.editing = true;
                        self.settings.edit_buffer = self.settings.log_query_row_limit.clone();
                    }
                    4 => {
                        // Edit prompt timeout.
                        self.settings.editing = true;
                        self.settings.edit_buffer = self.settings.prompt_timeout_secs.clone();
                    }
                    5 => {
                        // Toggle CCRv1 enabled.
                        self.settings.claude_ccrv1_enabled = !self.settings.claude_ccrv1_enabled;
                        let val = if self.settings.claude_ccrv1_enabled {
                            "true"
                        } else {
                            "false"
                        };
                        self.save_setting("claude_ccrv1_enabled", val).await;
                    }
                    6 => {
                        // Edit CCRv1 port.
                        self.settings.editing = true;
                        self.settings.edit_buffer = self.settings.claude_ccrv1_port.clone();
                    }
                    7 => {
                        // Toggle CCRv2 enabled.
                        self.settings.claude_ccrv2_enabled = !self.settings.claude_ccrv2_enabled;
                        let val = if self.settings.claude_ccrv2_enabled {
                            "true"
                        } else {
                            "false"
                        };
                        self.save_setting("claude_ccrv2_enabled", val).await;
                    }
                    8 => {
                        // Edit CCRv2 port.
                        self.settings.editing = true;
                        self.settings.edit_buffer = self.settings.claude_ccrv2_port.clone();
                    }
                    _ => {}
                }
            }
            SettingsTab::About => {}
        }
    }

    pub(crate) async fn apply_settings_edit(&mut self, val: String) {
        let sel = self.settings.selected;
        match self.settings.tab {
            SettingsTab::Llm => {
                let model_count = self.settings.model_definitions.len();
                if sel < model_count {
                    // Model edit is handled by handle_model_edit_key.
                } else {
                    let idx = sel - model_count;
                    match idx {
                        2 => {
                            self.settings.orchestrator_max_tokens = val.clone();
                            self.save_setting("llm_orchestrator_max_tokens", &val).await;
                        }
                        _ => {}
                    }
                }
            }
            SettingsTab::Agents => match sel {
                1 => {
                    self.settings.praxis_agent_thinking_effort = val.clone();
                    self.save_praxis_agent_settings().await;
                }
                _ => {}
            },
            SettingsTab::Service => match sel {
                1 => {
                    self.settings.mcp_port = val.clone();
                    self.save_setting("mcp_server_port", &val).await;
                }
                3 => {
                    self.settings.log_query_row_limit = val.clone();
                    self.save_setting("log_query_row_limit", &val).await;
                }
                4 => {
                    self.settings.prompt_timeout_secs = val.clone();
                    self.save_setting("prompt_timeout_secs", &val).await;
                }
                6 => {
                    self.settings.claude_ccrv1_port = val.clone();
                    self.save_setting("claude_ccrv1_port", &val).await;
                }
                8 => {
                    self.settings.claude_ccrv2_port = val.clone();
                    self.save_setting("claude_ccrv2_port", &val).await;
                }
                _ => {}
            },
            SettingsTab::Intercept => {}
            SettingsTab::About => {}
        }
    }

    pub(crate) async fn handle_settings_mouse(
        &mut self,
        mouse: MouseEvent,
        content_area: Rect,
        terminal_area: Rect,
    ) {
        //
        // Settings model edit form popup.
        //
        if self.settings.model_form.is_some() {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                if let Some(ref mut form) = self.settings.model_form {
                    //
                    // Calculate popup geometry matching render_model_form.
                    //
                    let show_base_url = form.shows_base_url();
                    let field_count = if show_base_url { 4u16 } else { 3u16 };
                    let base_lines = field_count + 2 + 2;
                    let dropdown_extra = if form.model_dropdown_open {
                        1 + form.available_models.len() as u16
                    } else if form.loading_models {
                        1
                    } else {
                        0
                    };
                    let popup_h =
                        (base_lines + dropdown_extra).min(terminal_area.height.saturating_sub(4));
                    let popup_w = 60u16.min(terminal_area.width.saturating_sub(4));
                    let px = (terminal_area.width.saturating_sub(popup_w)) / 2;
                    let py = (terminal_area.height.saturating_sub(popup_h)) / 2;
                    let inner_x = px + 1;
                    let inner_y = py + 1;

                    let rel_row = mouse.row.saturating_sub(inner_y) as usize;
                    let rel_col = mouse.column.saturating_sub(inner_x) as usize;

                    //
                    // Row layout depends on whether base_url is shown:
                    // Without: 0=Provider, 1=APIKey, 2=Model, 3=blank, 4=hints
                    // With:    0=Provider, 1=APIKey, 2=BaseURL, 3=Model, 4=blank, 5=hints
                    //
                    let model_row = if show_base_url { 3 } else { 2 };
                    let hints_row = model_row + 2;
                    let dropdown_start = hints_row + 2;

                    if rel_row == 0 {
                        form.focused_field = 0;
                        let providers = crate::app::sorted_providers();
                        if rel_col > 14 {
                            form.provider_idx = (form.provider_idx + 1) % providers.len();
                            let p = providers[form.provider_idx];
                            form.base_url = if p.api_key_optional() {
                                p.base_url().to_string()
                            } else {
                                String::new()
                            };
                        }
                    } else if rel_row == 1 {
                        form.focused_field = 1;
                        if !form.editing_text {
                            form.editing_text = true;
                            form.cursor_pos = form.api_key.len();
                        }
                    } else if show_base_url && rel_row == 2 {
                        form.focused_field = 2;
                        if !form.editing_text {
                            form.editing_text = true;
                            form.cursor_pos = form.base_url.len();
                        }
                    } else if rel_row == model_row {
                        form.focused_field = if show_base_url { 3 } else { 2 };
                        if !form.editing_text {
                            form.editing_text = true;
                            form.cursor_pos = form.model_name.len();
                        }
                    } else if rel_row == hints_row {
                        if rel_col >= 2 && rel_col < 10 {
                            self.save_model_form().await;
                        } else if rel_col >= 11 {
                            self.settings.model_form = None;
                        }
                    } else if form.model_dropdown_open
                        && !form.available_models.is_empty()
                        && rel_row >= dropdown_start
                    {
                        let model_idx = rel_row - dropdown_start + form.model_dropdown_scroll;
                        if model_idx < form.available_models.len() {
                            form.model_dropdown_selected = model_idx;
                            form.model_name = form.available_models[model_idx].clone();
                            form.model_dropdown_open = false;
                        }
                    }
                }
            }
            return;
        }

        //
        // Settings dropdown (model assignment selection).
        //
        if self.settings.dropdown_open {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                let item_count = self.settings.model_definitions.len();
                if item_count > 0 {
                    let popup_h =
                        (item_count as u16 + 2).min(terminal_area.height.saturating_sub(4));
                    let max_name = self
                        .settings
                        .model_definitions
                        .iter()
                        .map(|d| d.name.len())
                        .max()
                        .unwrap_or(20);
                    let popup_w = (max_name as u16 + 6).min(terminal_area.width.saturating_sub(4));
                    let px = content_area.x + (content_area.width.saturating_sub(popup_w)) / 2;
                    let py = content_area.y + (content_area.height.saturating_sub(popup_h)) / 2;
                    let inner_x = px + 1;
                    let inner_y = py + 1;
                    let inner_h = popup_h.saturating_sub(2);

                    if mouse.row >= inner_y
                        && mouse.row < inner_y + inner_h
                        && mouse.column >= inner_x
                        && mouse.column < inner_x + popup_w.saturating_sub(2)
                    {
                        let clicked = (mouse.row - inner_y) as usize;
                        if clicked < item_count {
                            let is_dbl = self.is_double_click(mouse.row, mouse.column);
                            self.settings.dropdown_selected = clicked;
                            if is_dbl {
                                self.apply_dropdown_selection().await;
                            }
                        }
                    } else {
                        self.settings.dropdown_open = false;
                    }
                }
            }
            return;
        }

        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            let settings_chunks = Layout::vertical([
                Constraint::Length(1), // tabs
                Constraint::Length(1), // spacer
                Constraint::Min(1),    // content
                Constraint::Length(1), // status
            ])
            .split(content_area);
            let tabs_area = settings_chunks[0];
            let settings_content = settings_chunks[2];

            //
            // Tab clicks. Match the rendered tab positions:
            // "  LLM  |  Agents  |  Intercept  |  Service  |  About "
            //
            if mouse.row == tabs_area.y {
                let rel = mouse.column.saturating_sub(tabs_area.x) as usize;
                //
                // Positions from render_tabs spans:
                // "  " (2) + " LLM " (5) + "  \u{2502}  " (5) + " Agents " (8)
                //   + "  \u{2502}  " (5) + " Intercept " (11) + "  \u{2502}  " (5)
                //   + " Service " (9) + "  \u{2502}  " (5) + " About " (7).
                //
                if rel >= 2 && rel < 7 {
                    self.switch_settings_tab(SettingsTab::Llm).await;
                } else if rel >= 12 && rel < 20 {
                    self.switch_settings_tab(SettingsTab::Agents).await;
                } else if rel >= 25 && rel < 36 {
                    self.switch_settings_tab(SettingsTab::Intercept).await;
                } else if rel >= 41 && rel < 50 {
                    self.switch_settings_tab(SettingsTab::Service).await;
                } else if rel >= 55 && rel < 62 {
                    self.switch_settings_tab(SettingsTab::About).await;
                }
                return;
            }

            //
            // Content area clicks — select the clicked field/toggle.
            // The content is rendered as a Paragraph with lines. We map
            // the click row to the settings item index.
            //
            if mouse.row >= settings_content.y
                && mouse.row < settings_content.y.saturating_add(settings_content.height)
            {
                let rel_row = (mouse.row - settings_content.y) as usize;
                let item_count = self.settings_item_count();

                //
                // Map visual row to item index based on tab layout.
                // Each tab has headers, blanks, and item rows. We build a
                // mapping from visual row -> item index.
                //
                let clicked_item = match self.settings.tab {
                    SettingsTab::Llm => {
                        let mc = self.settings.model_definitions.len();
                        // Row 0: "Model Definitions" header
                        // Row 1: blank
                        // Rows 2..2+mc: model definition items (idx 0..mc)
                        // Row 2+mc: "+ Add model" (idx mc)
                        // Row 3+mc: blank
                        // Row 4+mc: "Feature Assignments" header
                        // Row 5+mc: blank
                        // Rows 6+mc..6+mc+5: feature items (idx mc+1..mc+5)
                        if rel_row >= 2 && rel_row < 2 + mc {
                            Some(rel_row - 2)
                        } else if rel_row == 2 + mc {
                            Some(mc)
                        } else if rel_row >= 6 + mc && rel_row < 6 + mc + 5 {
                            Some(mc + 1 + (rel_row - 6 - mc))
                        } else {
                            None
                        }
                    }
                    SettingsTab::Agents => {
                        let sc = self.settings.agent_scripts.len();
                        // Row 0: "PRAXIS AGENT" header
                        // Row 1: blank
                        // Row 2: Praxis Model (idx 0)
                        // Row 3: Thinking Effort (idx 1)
                        // Row 4: Praxis Agent toggle (idx 2)
                        // Row 5: System Prompt (idx 3)
                        // Row 6: blank
                        // Row 7: "Lua Agent Connector Scripts" header
                        // Row 8: blank
                        // Rows 9..9+sc: scripts (idx 4..4+sc)
                        // Row 9+sc: blank
                        // Row 10+sc: "+ New agent script" (idx 4+sc)
                        // Row 11+sc: "Reset to defaults" (idx 4+sc+1)
                        if rel_row >= 2 && rel_row < 2 + 4 {
                            Some(rel_row - 2)
                        } else if rel_row >= 9 && rel_row < 9 + sc {
                            Some(4 + rel_row - 9)
                        } else if rel_row == 10 + sc {
                            Some(4 + sc)
                        } else if rel_row == 11 + sc {
                            Some(4 + sc + 1)
                        } else {
                            None
                        }
                    }
                    SettingsTab::Intercept => {
                        //
                        // Layout: header (0), blank (1), tc target lines
                        // (2..2+tc), blank, then the two action rows.
                        // Target rows are not clickable.
                        //
                        let tc = self.settings.intercept_targets.len();
                        if rel_row == 3 + tc {
                            Some(0)
                        } else if rel_row == 4 + tc {
                            Some(1)
                        } else {
                            None
                        }
                    }
                    SettingsTab::Service => {
                        // Row 0: "MCP Server" header, 1: blank
                        // Row 2: MCP Server toggle (0), 3: MCP Port (1)
                        // Row 4: blank, 5: "Logging" header, 6: blank
                        // Row 7: Event Logging (2), 8: Log Query limit (3), 9: Prompt timeout (4)
                        // Row 10: blank, 11: "Claude Bridge" header, 12: description, 13: blank
                        // Row 14: CCRv1 enabled (5), 15: CCRv1 port (6)
                        // Row 16: CCRv2 enabled (7), 17: CCRv2 port (8)
                        match rel_row {
                            2 => Some(0),
                            3 => Some(1),
                            7 => Some(2),
                            8 => Some(3),
                            9 => Some(4),
                            14 => Some(5),
                            15 => Some(6),
                            16 => Some(7),
                            17 => Some(8),
                            _ => None,
                        }
                    }
                    SettingsTab::About => {
                        //
                        // Links row: "originhq.com   praxis.originhq.com"
                        // Located at row 13 in the about content.
                        //
                        if rel_row == 13 {
                            let rel_col = mouse.column.saturating_sub(settings_content.x) as usize;
                            if rel_col < 12 {
                                Self::open_url("https://originhq.com");
                            } else if rel_col >= 15 {
                                Self::open_url("https://praxis.originhq.com");
                            }
                        }
                        None
                    }
                };

                if let Some(idx) = clicked_item {
                    if idx < item_count {
                        let is_dbl = self.is_double_click(mouse.row, mouse.column);

                        //
                        // If already editing, commit current edit first.
                        //
                        if self.settings.editing {
                            let val = self.settings.edit_buffer.clone();
                            self.settings.editing = false;
                            self.apply_settings_edit(val).await;
                        }
                        self.settings.selected = idx;

                        if is_dbl {
                            self.activate_settings_item().await;
                        } else {
                            self.auto_enter_edit();
                        }
                    }
                }
            }
        }
    }

    //
    // Open the intercept-targets virtual file (raw TOML) in $EDITOR.
    // On a clean exit and non-empty content the new text is sent to the
    // service; a parse error there is surfaced via the status line and
    // the locally-cached error field.
    //

    pub(crate) async fn edit_intercept_targets_in_editor(&mut self) {
        use std::io::Write;

        //
        // Make sure we have the current text before opening the editor —
        // poll the cached state first, fetching fresh if needed.
        //
        if !self.settings.intercept_targets_loaded {
            self.load_intercept_targets().await;
        }
        let initial_text = self.client.get_intercept_targets_text().await;
        let initial_text = if initial_text.is_empty() {
            //
            // Fallback for the case where the service hasn't responded yet:
            // start from an empty buffer rather than overwriting silently.
            //
            "# Praxis intercept targets — service did not return content.\n# Save this file to write it; reset to defaults from the settings menu.\n".to_string()
        } else {
            initial_text
        };

        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| {
                if cfg!(windows) {
                    "notepad".to_string()
                } else {
                    "vi".to_string()
                }
            });

        let tmp = match tempfile::Builder::new()
            .prefix("praxis_intercept_targets_")
            .suffix(".toml")
            .tempfile()
        {
            Ok(f) => f,
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to create temp file: {}", e));
                self.settings.status_message_at = Some(std::time::Instant::now());
                return;
            }
        };

        if let Err(e) = tmp.as_file().write_all(initial_text.as_bytes()) {
            self.settings.status_message = Some(format!("Failed to write temp file: {}", e));
            self.settings.status_message_at = Some(std::time::Instant::now());
            return;
        }

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

        while crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
            let _ = crossterm::event::read();
        }
        self.needs_full_redraw = true;

        match status {
            Ok(s) if s.success() => match std::fs::read_to_string(&path) {
                Ok(content) => {
                    if content == initial_text {
                        self.settings.status_message = Some("No changes".to_string());
                        self.settings.status_message_at = Some(std::time::Instant::now());
                        return;
                    }
                    if let Err(e) = self.client.set_intercept_targets(content).await {
                        self.settings.status_message =
                            Some(format!("Failed to save targets: {}", e));
                        self.settings.status_message_at = Some(std::time::Instant::now());
                        return;
                    }
                    self.settings.status_message = Some("Saved".to_string());
                    self.settings.status_message_at = Some(std::time::Instant::now());
                    self.settings.intercept_targets_loaded = false;
                    self.load_intercept_targets().await;
                }
                Err(e) => {
                    self.settings.status_message = Some(format!("Failed to read file: {}", e));
                    self.settings.status_message_at = Some(std::time::Instant::now());
                }
            },
            Ok(_) => {
                self.settings.status_message = Some("Editor exited with error".to_string());
                self.settings.status_message_at = Some(std::time::Instant::now());
            }
            Err(e) => {
                self.settings.status_message =
                    Some(format!("Failed to launch editor '{}': {}", editor, e));
                self.settings.status_message_at = Some(std::time::Instant::now());
            }
        }
    }

    pub(crate) async fn reset_intercept_targets_to_defaults(&mut self) {
        if let Err(e) = self.client.reset_intercept_targets_defaults().await {
            self.settings.status_message = Some(format!("Failed to reset: {}", e));
            self.settings.status_message_at = Some(std::time::Instant::now());
            return;
        }
        self.settings.status_message = Some("Reset to defaults".to_string());
        self.settings.status_message_at = Some(std::time::Instant::now());
        self.settings.intercept_targets_loaded = false;
        self.load_intercept_targets().await;
    }
}
