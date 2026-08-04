use super::super::*;
use super::sorted_providers;

pub struct ModelEditForm {
    pub edit_index: Option<usize>, // None = adding new, Some(i) = editing existing
    pub focused_field: usize,      // 0=provider, 1=apiKey, 2=baseUrl, 3=model
    pub provider_idx: usize,       // index into Provider::all()
    pub api_key: String,
    pub base_url: String,
    pub model_name: String,
    pub editing_text: bool, // true when typing in a text field
    pub cursor_pos: usize,  // char-based cursor position in active field
    pub available_models: Vec<String>,
    pub model_dropdown_open: bool,
    pub model_dropdown_selected: usize,
    pub model_dropdown_scroll: usize,
    pub model_dropdown_inner_h: std::cell::Cell<usize>,
    pub loading_models: bool,
}

impl ModelEditForm {
    /// Whether the currently selected provider shows the base URL field.
    pub fn shows_base_url(&self) -> bool {
        let providers = sorted_providers();
        providers
            .get(self.provider_idx)
            .map(|p| p.api_key_optional())
            .unwrap_or(false)
    }

    /// The maximum field index (depends on whether base_url is shown).
    pub fn max_field(&self) -> usize {
        if self.shows_base_url() { 3 } else { 2 }
    }

    /// Map focused_field to actual field accounting for hidden base_url.
    /// Returns: 0=provider, 1=apiKey, 2=baseUrl (if shown), 3=model.
    /// When base_url is hidden, field 2 maps to model.
    pub fn logical_field(&self) -> usize {
        if self.shows_base_url() || self.focused_field < 2 {
            self.focused_field
        } else {
            self.focused_field + 1 // skip base_url
        }
    }

    pub fn active_field(&self) -> &str {
        match self.logical_field() {
            1 => &self.api_key,
            2 => &self.base_url,
            3 => &self.model_name,
            _ => "",
        }
    }

    pub fn active_field_len(&self) -> usize {
        self.active_field().chars().count()
    }
}

impl App {
    pub(crate) fn open_model_form(&mut self, edit_index: Option<usize>) {
        let providers = sorted_providers();
        let (provider_idx, api_key, base_url, model_name) = match edit_index {
            Some(idx) => {
                let def = &self.settings.model_definitions[idx];
                let pidx = providers
                    .iter()
                    .position(|p| p.as_str() == def.provider)
                    .unwrap_or(0);
                (
                    pidx,
                    def.api_key.clone(),
                    def.base_url.clone().unwrap_or_default(),
                    def.model.clone(),
                )
            }
            None => {
                let default_url = if providers[0].api_key_optional() {
                    providers[0].base_url().to_string()
                } else {
                    String::new()
                };
                (0, String::new(), default_url, String::new())
            }
        };

        self.settings.model_form = Some(ModelEditForm {
            edit_index,
            focused_field: 0,
            provider_idx,
            api_key,
            base_url,
            model_name,
            editing_text: false,
            cursor_pos: 0,
            available_models: Vec::new(),
            model_dropdown_open: false,
            model_dropdown_selected: 0,
            model_dropdown_scroll: 0,
            model_dropdown_inner_h: std::cell::Cell::new(0),
            loading_models: false,
        });
    }

    pub(crate) async fn handle_model_form_key(&mut self, key: KeyEvent) {
        let form = match self.settings.model_form.as_mut() {
            Some(f) => f,
            None => return,
        };

        //
        // Model name dropdown navigation.
        //

        if form.model_dropdown_open {
            match key.code {
                KeyCode::Esc => {
                    form.model_dropdown_open = false;
                }
                KeyCode::Up => {
                    if form.model_dropdown_selected > 0 {
                        form.model_dropdown_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if !form.available_models.is_empty()
                        && form.model_dropdown_selected < form.available_models.len() - 1
                    {
                        form.model_dropdown_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(name) = form.available_models.get(form.model_dropdown_selected) {
                        form.model_name = name.clone();
                    }
                    form.model_dropdown_open = false;
                    form.model_dropdown_scroll = 0;
                    form.cursor_pos = form.model_name.chars().count();
                }
                _ => {}
            }

            //
            // Keep the selected model visible in the dropdown. Replicate
            // the popup size calculation from the render function so the
            // scroll window matches exactly.
            //

            let visible = form.model_dropdown_inner_h.get();
            if visible > 0 {
                let sel = form.model_dropdown_selected;
                let scroll = &mut form.model_dropdown_scroll;
                if sel < *scroll {
                    *scroll = sel;
                } else if sel >= *scroll + visible {
                    *scroll = sel - visible + 1;
                }
            }

            return;
        }

        //
        // Text editing mode for api_key or model_name fields.
        //

        if form.editing_text {
            let max_field = form.max_field();
            let is_model_field = form.logical_field() == 3;
            match key.code {
                KeyCode::Esc => {
                    self.settings.model_form = None;
                    return;
                }
                KeyCode::Up | KeyCode::BackTab => {
                    if form.focused_field > 0 {
                        form.focused_field -= 1;
                        Self::sync_model_form_edit(form);
                    }
                }
                KeyCode::Enter => {
                    if is_model_field {
                        self.load_provider_models().await;
                        return;
                    }
                    if form.focused_field < max_field {
                        form.focused_field += 1;
                        Self::sync_model_form_edit(form);
                    }
                }
                KeyCode::Down | KeyCode::Tab => {
                    if form.focused_field < max_field {
                        form.focused_field += 1;
                        Self::sync_model_form_edit(form);
                    }
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.save_model_form().await;
                    return;
                }
                KeyCode::Left => {
                    if form.cursor_pos > 0 {
                        form.cursor_pos -= 1;
                    }
                }
                KeyCode::Right => {
                    let len = form.active_field_len();
                    if form.cursor_pos < len {
                        form.cursor_pos += 1;
                    }
                }
                KeyCode::Home => {
                    form.cursor_pos = 0;
                }
                KeyCode::End => {
                    form.cursor_pos = form.active_field_len();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let pos = form.cursor_pos;
                    let logical = form.logical_field();
                    let field = match logical {
                        1 => &mut form.api_key,
                        2 => &mut form.base_url,
                        3 => &mut form.model_name,
                        _ => {
                            return;
                        }
                    };
                    let byte_pos = field
                        .char_indices()
                        .nth(pos)
                        .map(|(i, _)| i)
                        .unwrap_or(field.len());
                    field.insert(byte_pos, c);
                    form.cursor_pos += 1;
                }
                KeyCode::Backspace => {
                    if form.cursor_pos > 0 {
                        let pos = form.cursor_pos - 1;
                        let logical = form.logical_field();
                        let field = match logical {
                            1 => &mut form.api_key,
                            2 => &mut form.base_url,
                            3 => &mut form.model_name,
                            _ => {
                                return;
                            }
                        };
                        let byte_pos = field
                            .char_indices()
                            .nth(pos)
                            .map(|(i, _)| i)
                            .unwrap_or(field.len());
                        field.remove(byte_pos);
                        form.cursor_pos -= 1;
                    }
                }
                KeyCode::Delete => {
                    let pos = form.cursor_pos;
                    let logical = form.logical_field();
                    let field = match logical {
                        1 => &mut form.api_key,
                        2 => &mut form.base_url,
                        3 => &mut form.model_name,
                        _ => {
                            return;
                        }
                    };
                    let len = field.chars().count();
                    if pos < len {
                        let byte_pos = field
                            .char_indices()
                            .nth(pos)
                            .map(|(i, _)| i)
                            .unwrap_or(field.len());
                        field.remove(byte_pos);
                    }
                }
                _ => {}
            }
            return;
        }

        //
        // Normal form navigation.
        //

        match key.code {
            KeyCode::Esc => {
                self.settings.model_form = None;
            }
            KeyCode::Up | KeyCode::BackTab => {
                let form = self.settings.model_form.as_mut().unwrap();
                if form.focused_field > 0 {
                    form.focused_field -= 1;
                    Self::sync_model_form_edit(form);
                }
            }
            KeyCode::Down | KeyCode::Tab => {
                let form = self.settings.model_form.as_mut().unwrap();
                let max_field = form.max_field();
                if form.focused_field < max_field {
                    form.focused_field += 1;
                    Self::sync_model_form_edit(form);
                }
            }
            KeyCode::Left => {
                let form = self.settings.model_form.as_mut().unwrap();
                if form.focused_field == 0 {
                    let providers = sorted_providers();
                    if form.provider_idx > 0 {
                        form.provider_idx -= 1;
                    } else {
                        form.provider_idx = providers.len() - 1;
                    }
                    form.available_models.clear();
                    let p = providers[form.provider_idx];
                    form.base_url = if p.api_key_optional() {
                        p.base_url().to_string()
                    } else {
                        String::new()
                    };
                }
            }
            KeyCode::Right => {
                let form = self.settings.model_form.as_mut().unwrap();
                if form.focused_field == 0 {
                    let providers = sorted_providers();
                    form.provider_idx = (form.provider_idx + 1) % providers.len();
                    form.available_models.clear();
                    let p = providers[form.provider_idx];
                    form.base_url = if p.api_key_optional() {
                        p.base_url().to_string()
                    } else {
                        String::new()
                    };
                }
            }
            KeyCode::Enter => {
                let form = self.settings.model_form.as_mut().unwrap();
                if form.focused_field == 0 {
                    form.focused_field = 1;
                    Self::sync_model_form_edit(form);
                }
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Save shortcut.
                self.save_model_form().await;
            }
            _ => {}
        }
    }

    pub(crate) async fn load_provider_models(&mut self) {
        let form = match self.settings.model_form.as_mut() {
            Some(f) => f,
            None => return,
        };

        let providers = sorted_providers();
        let provider_enum = providers[form.provider_idx];
        let provider = provider_enum.as_str().to_string();
        let api_key = form.api_key.clone();
        let base_url = if form.base_url.is_empty() {
            None
        } else {
            Some(form.base_url.clone())
        };

        if api_key.is_empty() && !provider_enum.api_key_optional() {
            self.settings.status_message = Some("Enter an API key first".to_string());
            return;
        }

        if provider_enum.requires_base_url() && base_url.is_none() {
            self.settings.status_message = Some("Enter a base URL first".to_string());
            return;
        }

        form.loading_models = true;
        let result =
            common::ai::fetch_models_for_provider(&provider, &api_key, base_url.as_deref()).await;

        let form = match self.settings.model_form.as_mut() {
            Some(f) => f,
            None => return,
        };
        form.loading_models = false;

        match result {
            Ok(models) => {
                if models.is_empty() {
                    self.settings.status_message = Some("No models returned".to_string());
                } else {
                    form.available_models = models;
                    form.model_dropdown_selected = 0;
                    form.model_dropdown_open = true;
                }
            }
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to load models: {}", e));
            }
        }
    }

    pub(crate) fn sync_model_form_edit(form: &mut ModelEditForm) {
        match form.logical_field() {
            1 => {
                form.editing_text = true;
                form.cursor_pos = form.api_key.chars().count();
            }
            2 => {
                form.editing_text = true;
                form.cursor_pos = form.base_url.chars().count();
            }
            3 => {
                form.editing_text = true;
                form.cursor_pos = form.model_name.chars().count();
            }
            _ => {
                form.editing_text = false;
            }
        }
    }

    pub(crate) async fn save_model_form(&mut self) {
        let form = match self.settings.model_form.take() {
            Some(f) => f,
            None => return,
        };

        let providers = sorted_providers();
        let provider_str = providers[form.provider_idx].as_str().to_string();

        if form.model_name.is_empty() {
            self.settings.status_message = Some("Model name is required".to_string());
            self.settings.model_form = Some(form);
            return;
        }

        let name = format!("{}::{}", provider_str, form.model_name);
        let base_url = if form.base_url.is_empty() {
            None
        } else {
            Some(form.base_url)
        };
        let def = ModelDef {
            name,
            provider: provider_str,
            model: form.model_name,
            api_key: form.api_key,
            base_url,
        };

        match form.edit_index {
            Some(idx) => {
                if idx < self.settings.model_definitions.len() {
                    self.settings.model_definitions[idx] = def;
                }
            }
            None => {
                self.settings.model_definitions.push(def);
            }
        }

        self.settings
            .model_definitions
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        self.save_model_definitions().await;
    }

    pub(crate) async fn save_model_definitions(&mut self) {
        //
        // Remove any empty (incomplete) definitions.
        //
        self.settings
            .model_definitions
            .retain(|d| !d.provider.is_empty() && !d.model.is_empty());

        match serde_json::to_string(&self.settings.model_definitions) {
            Ok(json) => {
                if self.save_setting("llm_model_definitions", &json).await {
                    let mut model = self.settings.orchestrator_model.clone();
                    if model.is_empty() && self.settings.model_definitions.len() == 1 {
                        let first_model = self.settings.model_definitions[0].name.clone();
                        if self
                            .save_setting("llm_feature_orchestrator", &first_model)
                            .await
                        {
                            self.settings.orchestrator_model = first_model.clone();
                            model = first_model;
                        }
                    }
                    let model_is_available = self
                        .settings
                        .model_definitions
                        .iter()
                        .any(|definition| definition.name == model);
                    let needs_session = self
                        .orchestrator
                        .active_session()
                        .map(|session| session.session_id.is_empty())
                        .unwrap_or(true);

                    if model_is_available && needs_session {
                        self.orchestrator.configured_model = model.clone();
                        self.select_model(&model).await;
                    }
                }
            }
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to serialize models: {}", e));
            }
        }
    }
}
