//! Overlay and popup mouse dispatch — keeps mouse.rs from growing without bound.

use crossterm::event::MouseEvent;
use ratatui::layout::{Constraint, Layout, Rect};


use crate::app::{
    AddRemoteNodeForm, App, ChainFormEditor, NewOpForm, PopupKind, ReconTab, SettingsTab,
};
use crate::ui::hits::{MouseAction, ReconHintAction, SessionHintAction};

impl App {
    pub(crate) async fn dispatch_overlay_action(
        &mut self,
        mouse: MouseEvent,
        action: MouseAction,
    ) -> bool {
        match action {
            MouseAction::ConfirmYes => {
                if let Some(confirm) = self.confirm.take() {
                    self.execute_confirm(confirm).await;
                }
                true
            }
            MouseAction::ConfirmNo => {
                self.confirm = None;
                true
            }
            MouseAction::ConfirmDismiss => {
                self.confirm = None;
                true
            }

            MouseAction::PopupItem(idx) => {
                let selection = self.popup.as_ref().and_then(|popup| {
                    let filtered = popup.filtered_items();
                    let value = filtered.get(idx).map(|(_, item)| item.value.clone())?;
                    Some((popup.kind, value))
                });
                if let Some(p) = self.popup.as_mut() {
                    p.selected = idx;
                }
                if self.is_double_click(mouse.row, mouse.column) {
                    if let Some((kind, value)) = selection {
                        match kind {
                            PopupKind::ModelSelect => {
                                self.popup = None;
                                self.select_model(&value).await;
                            }
                            PopupKind::CommandPalette => {
                                self.popup = None;
                                self.orchestrator.input.clear();
                                self.orchestrator.cursor_pos = 0;
                                self.handle_slash_command(&format!("/{}", value)).await;
                            }
                            PopupKind::SaveSession => {}
                        }
                    }
                }
                true
            }
            MouseAction::PopupDismiss => {
                self.popup = None;
                true
            }

            MouseAction::NewOpField(field) => {
                if let Some(ref mut form) = self.new_op_form {
                    form.focused_field = field;
                    if NewOpForm::is_toggle(field) {
                        Self::toggle_new_op_field(form);
                    }
                }
                true
            }
            MouseAction::NewOpSave => {
                self.submit_new_op().await;
                true
            }
            MouseAction::NewOpCancel => {
                self.new_op_form = None;
                true
            }

            MouseAction::RunOptionsToggle { section, index } => {
                self.toggle_run_option(section, index);
                true
            }
            MouseAction::RunOptionsRun => {
                if let Some(opts) = self.run_options.take() {
                    self.execute_run_options(opts).await;
                }
                true
            }
            MouseAction::RunOptionsCancel => {
                self.run_options = None;
                true
            }

            MouseAction::TriggerSave => {
                self.submit_trigger_form().await;
                true
            }
            MouseAction::TriggerCancel => {
                self.trigger_form = None;
                true
            }
            MouseAction::TriggerField { section, cursor } => {
                if let Some(ref mut form) = self.trigger_form {
                    form.focused_section = section;
                    form.cursor = cursor;
                    Self::toggle_trigger_form_selection(form);
                }
                true
            }

            MouseAction::AddRemoteField(field) => {
                if let Some(ref mut form) = self.add_remote_node_form {
                    form.focused_field = field;
                    form.editing_text = field != AddRemoteNodeForm::KIND_FIELD;
                }
                true
            }
            MouseAction::AddRemoteSave => {
                self.submit_add_remote_node_form().await;
                true
            }

            MouseAction::SessionsListRow(idx) => {
                if idx < self.nodes.sessions.len() {
                    self.nodes.sessions_list_selected = idx;
                    if self.is_double_click(mouse.row, mouse.column) {
                        if let Some(id) = self.selected_list_session_id() {
                            self.resume_session(&id);
                        }
                    }
                }
                true
            }
            MouseAction::SessionsListDismiss => {
                self.nodes.sessions_list_open = false;
                true
            }

            MouseAction::SessionInput { text_start } => {
                if let Some(session) = self.nodes.active_session_mut() {
                    if !session.is_waiting && session.session_id.is_some() {
                        let click_offset = mouse.column.saturating_sub(text_start) as usize;
                        session.cursor_pos = click_offset.min(session.input.len());
                    }
                }
                true
            }
            MouseAction::SessionHint(hint) => match hint {
                SessionHintAction::Pause => {
                    self.pause_active_session();
                    true
                }
                SessionHintAction::Close => {
                    self.close_active_session();
                    true
                }
            },

            MouseAction::SessionOptionsRow(row) => {
                if let Some(ref mut opts) = self.nodes.session_options {
                    if row == 0 {
                        opts.yolo = !opts.yolo;
                    } else if row >= 3 {
                        let dir_count = if opts.working_dirs.is_empty() {
                            1
                        } else {
                            1 + opts.working_dirs.len()
                        };
                        let idx = row - 3;
                        if idx < dir_count {
                            opts.selected_dir = idx;
                        }
                    }
                }
                true
            }
            MouseAction::SessionOptionsConfirm => {
                self.confirm_session_options();
                true
            }
            MouseAction::SessionOptionsCancel => {
                self.nodes.session_options = None;
                true
            }

            MouseAction::SettingsContentClick => {
                self.dispatch_settings_content_click(mouse).await;
                true
            }
            MouseAction::SettingsModelField { row, body_x } => {
                self.dispatch_settings_model_field(mouse, row, body_x).await;
                true
            }
            MouseAction::SettingsModelDropdownItem(idx) => {
                if let Some(ref mut form) = self.settings.model_form {
                    if idx < form.available_models.len() {
                        form.model_dropdown_selected = idx;
                        form.model_name = form.available_models[idx].clone();
                        form.model_dropdown_open = false;
                    }
                }
                true
            }
            MouseAction::SettingsModelSave => {
                self.save_model_form().await;
                true
            }
            MouseAction::SettingsModelCancel => {
                self.settings.model_form = None;
                true
            }
            MouseAction::SettingsDropdownRow(idx) => {
                if idx < self.settings.model_definitions.len() {
                    let is_dbl = self.is_double_click(mouse.row, mouse.column);
                    self.settings.dropdown_selected = idx;
                    if is_dbl {
                        self.apply_dropdown_selection().await;
                    }
                }
                true
            }
            MouseAction::SettingsDropdownDismiss => {
                self.settings.dropdown_open = false;
                true
            }

            MouseAction::ChainSave => {
                self.submit_chain_form().await;
                true
            }
            MouseAction::ChainCancel => {
                self.request_close_chain_form();
                true
            }
            MouseAction::ChainAutoLayout => {
                if let Some(form) = self.chain_form.as_mut() {
                    form.positions.clear();
                    super::chain_form::auto_layout(form);
                    form.camera_x = 0;
                    form.camera_y = 0;
                    form.mark_dirty();
                }
                true
            }
            MouseAction::ChainPalette(kind) => {
                self.add_element_at_centre(kind);
                true
            }
            MouseAction::ChainEdit(target) => {
                if let Some(form) = self.chain_form.as_mut() {
                    form.editing = Some(target);
                    // typing starts on the next key; treat field focus as dirty intent
                }
                true
            }
            MouseAction::ChainCycleKind => {
                self.cycle_selected_kind();
                true
            }
            MouseAction::ChainDeleteElement => {
                if let Some(form) = self.chain_form.as_mut() {
                    super::chain_form::delete_selection(form);
                }
                true
            }
            MouseAction::ChainCycleCondition => {
                if let Some(form) = self.chain_form.as_mut() {
                    if let crate::app::Selected::Connection(idx) = form.selected.clone() {
                        if let Some(conn) = form.connections.get_mut(idx) {
                            conn.condition =
                                super::chain_form::cycle_condition(conn.condition, 1);
                            form.mark_dirty();
                        }
                    }
                }
                true
            }
            MouseAction::ChainDeleteConnection => {
                if let Some(form) = self.chain_form.as_mut() {
                    super::chain_form::delete_selection(form);
                }
                true
            }
            MouseAction::ChainPickOp => {
                if let Some(form) = self.chain_form.as_mut() {
                    form.editor = Some(crate::app::ChainFormEditor::PickOpName {
                        cursor: 0,
                        filter: String::new(),
                    });
                }
                true
            }
            MouseAction::ChainPickModel => {
                if let Some(form) = self.chain_form.as_mut() {
                    form.editor = Some(crate::app::ChainFormEditor::PickModel {
                        cursor: 0,
                        filter: String::new(),
                    });
                }
                true
            }
            MouseAction::ChainPickTool => {
                if let Some(form) = self.chain_form.as_mut() {
                    form.editor = Some(crate::app::ChainFormEditor::PickTool {
                        cursor: 0,
                        filter: String::new(),
                    });
                }
                true
            }
            MouseAction::ChainPickPayload => {
                if let Some(form) = self.chain_form.as_mut() {
                    form.editor = Some(crate::app::ChainFormEditor::PickPayload {
                        cursor: 0,
                        filter: String::new(),
                    });
                }
                true
            }
            MouseAction::ChainPickSessionGroup => {
                if let Some(form) = self.chain_form.as_mut() {
                    form.editor = Some(crate::app::ChainFormEditor::PickSessionGroup { cursor: 0 });
                }
                true
            }
            MouseAction::ChainCycleMemoryMode => {
                if let Some(form) = self.chain_form.as_mut() {
                    if let Some(el) = form.selected_block_mut() {
                        el.memory_mode = if el.memory_mode == 0 { 1 } else { 0 };
                        form.mark_dirty();
                    }
                }
                true
            }
            MouseAction::ChainToggleSessionYolo => {
                if let Some(form) = self.chain_form.as_mut() {
                    if let Some(el) = form.selected_block_mut() {
                        el.session_group.yolo_mode = !el.session_group.yolo_mode;
                        form.mark_dirty();
                    }
                }
                true
            }
            MouseAction::ChainCycleBlockYolo => {
                if let Some(form) = self.chain_form.as_mut() {
                    if let Some(el) = form.selected_block_mut() {
                        el.block_config.yolo_mode = match el.block_config.yolo_mode {
                            None => Some(true),
                            Some(true) => Some(false),
                            Some(false) => None,
                        };
                        form.mark_dirty();
                    }
                }
                true
            }
            MouseAction::ChainCycleRequireAll => {
                if let Some(form) = self.chain_form.as_mut() {
                    if let Some(el) = form.selected_block_mut() {
                        el.block_config.require_all_inputs =
                            match el.block_config.require_all_inputs {
                                None | Some(true) => Some(false),
                                Some(false) => None,
                            };
                        form.mark_dirty();
                    }
                }
                true
            }
            MouseAction::ChainCanvas => {
                self.chain_form_canvas_down(mouse).await;
                true
            }
            MouseAction::ChainPropsSurface => {
                //
                // Click on properties modal chrome (title, padding) —
                // absorb so the canvas underneath does not dismiss or pan.
                //
                true
            }
            MouseAction::ChainPickOpItem(idx) => {
                self.dispatch_chain_pick_op_item(mouse, idx);
                true
            }
            MouseAction::ChainEditorDismiss => {
                if let Some(form) = self.chain_form.as_mut() {
                    form.editor = None;
                }
                true
            }

            MouseAction::InterceptRuleField(field) => {
                if let Some(form) = self.intercept.rule_form.as_mut() {
                    form.focus = field;
                    // Toggle/cycle fields activate on click.
                    if field.is_cycleable() {
                        form.cycle_current();
                    }
                }
                true
            }
            MouseAction::InterceptRuleSave => {
                self.submit_rule_form().await;
                true
            }
            MouseAction::InterceptRuleCancel => {
                self.intercept.rule_form = None;
                true
            }

            MouseAction::ReconTab(tab) => {
                self.dispatch_recon_tab(tab);
                true
            }
            MouseAction::ReconLeftPane => {
                if let Some(recon) = self.nodes.recon.as_mut() {
                    recon.right_pane_focused = false;
                    recon.filter_focused = false;
                }
                true
            }
            MouseAction::ReconTreeRow { row } => {
                self.dispatch_recon_tree_row(row, false).await;
                true
            }
            MouseAction::ReconTreeChevron { row } => {
                self.dispatch_recon_tree_row(row, true).await;
                true
            }
            MouseAction::ReconFilterBar => {
                if let Some(recon) = self.nodes.recon.as_mut() {
                    recon.filter_focused = true;
                    recon.right_pane_focused = false;
                }
                true
            }
            MouseAction::ReconRightPane => {
                if let Some(recon) = self.nodes.recon.as_mut() {
                    recon.right_pane_focused = true;
                    recon.filter_focused = false;
                }
                true
            }
            MouseAction::ReconSplitDragStart => {
                if let Some(recon) = self.nodes.recon.as_mut() {
                    recon.recon_dragging = true;
                }
                true
            }
            MouseAction::ReconHint(hint) => {
                self.dispatch_recon_hint(hint).await;
                true
            }

            _ => false,
        }
    }

    fn dispatch_chain_pick_op_item(&mut self, mouse: MouseEvent, idx: usize) {
        let is_dbl = self.is_double_click(mouse.row, mouse.column);
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        let Some(editor) = form.editor.take() else {
            return;
        };

        if let ChainFormEditor::PickSessionGroup { mut cursor } = editor {
            let mut items = vec!["(none)".to_string(), "(new group)".to_string()];
            for g in super::chain_form::collect_session_groups(form) {
                items.push(g.id);
            }
            if idx < items.len() {
                cursor = idx;
                match cursor {
                    0 => {
                        if let Some(el) = form.selected_block_mut() {
                            el.session_group = crate::app::SessionGroupDraft::default();
                            form.mark_dirty();
                        }
                        return;
                    }
                    1 => {
                        let color = form.next_session_group_color();
                        let id = format!("sg_{}", form.element_id_seq + 1);
                        form.element_id_seq += 1;
                        if let Some(el) = form.selected_block_mut() {
                            el.session_group = crate::app::SessionGroupDraft {
                                id,
                                color,
                                yolo_mode: false,
                                working_dir: String::new(),
                            };
                            form.mark_dirty();
                        }
                        return;
                    }
                    n => {
                        let groups = super::chain_form::collect_session_groups(form);
                        if let Some(g) = groups.get(n - 2).cloned() {
                            if let Some(el) = form.selected_block_mut() {
                                el.session_group = g;
                                form.mark_dirty();
                            }
                            return;
                        }
                    }
                }
            }
            form.editor = Some(ChainFormEditor::PickSessionGroup { cursor });
            let _ = is_dbl;
            return;
        }

        let (kind_tag, list, filter) = match editor {
            ChainFormEditor::PickOpName { filter, .. } => {
                ("op", form.available_op_names.clone(), filter)
            }
            ChainFormEditor::PickModel { filter, .. } => {
                ("model", form.available_models.clone(), filter)
            }
            ChainFormEditor::PickTool { filter, .. } => {
                ("tool", form.available_tools.clone(), filter)
            }
            ChainFormEditor::PickPayload { filter, .. } => {
                ("payload", form.available_payloads.clone(), filter)
            }
            ChainFormEditor::PickSessionGroup { .. } => unreachable!(),
        };
        let filtered: Vec<String> = list
            .iter()
            .filter(|n| filter.is_empty() || n.to_lowercase().contains(&filter.to_lowercase()))
            .cloned()
            .collect();
        let mut cursor = idx.min(filtered.len().saturating_sub(1));
        if idx < filtered.len() {
            cursor = idx;
            if is_dbl {
                if let Some(name) = filtered.get(cursor) {
                    if let Some(el) = form.selected_block_mut() {
                        match kind_tag {
                            "op" => el.op_name = name.clone(),
                            "model" => el.model_ref = name.clone(),
                            "tool" => el.tool_name = name.clone(),
                            "payload" => el.payload_id = name.clone(),
                            _ => {}
                        }
                        form.mark_dirty();
                    }
                    return;
                }
            }
        }
        form.editor = Some(match kind_tag {
            "model" => ChainFormEditor::PickModel { cursor, filter },
            "tool" => ChainFormEditor::PickTool { cursor, filter },
            "payload" => ChainFormEditor::PickPayload { cursor, filter },
            _ => ChainFormEditor::PickOpName { cursor, filter },
        });
    }

    fn dispatch_recon_tab(&mut self, tab: ReconTab) {
        self.recon_switch_tab(tab);
    }

    async fn dispatch_recon_tree_row(&mut self, row: usize, chevron_only: bool) {
        let fetch = {
            let Some(recon) = self.nodes.recon.as_mut() else {
                return;
            };
            recon.right_pane_focused = false;
            recon.filter_focused = false;
            let rows = crate::ui::recon::tree::build_visible_rows(recon);
            let Some(target) = rows.get(row) else {
                return;
            };
            let id = target.id.clone();
            let expandable = target.expandable;

            if chevron_only && expandable {
                crate::ui::recon::tree::toggle_expand(recon, &id);
                recon.selected = Some(id);
                recon.selected_right_scroll = 0;
                false
            } else {
                let same = recon.selected.as_ref() == Some(&id);
                recon.selected = Some(id.clone());
                recon.selected_right_scroll = 0;
                recon.config_content_error = None;
                recon.session_content_error = None;
                recon.hovered_row = Some(row);

                if expandable && same {
                    crate::ui::recon::tree::toggle_expand(recon, &id);
                }
                true
            }
        };
        if fetch {
            self.handle_recon_enter().await;
        }
    }

    async fn dispatch_recon_hint(&mut self, hint: ReconHintAction) {
        match hint {
            ReconHintAction::Refresh => self.trigger_recon_refresh(false).await,
            ReconHintAction::Discover => self.trigger_recon_refresh(true).await,
            ReconHintAction::Edit => {
                if self
                    .nodes
                    .recon
                    .as_ref()
                    .is_some_and(|r| r.active_tab == ReconTab::Config)
                {
                    self.edit_recon_config_in_editor().await;
                }
            }
            ReconHintAction::Close => self.close_recon(),
        }
    }

    pub(crate) async fn dispatch_settings_content_click(&mut self, mouse: MouseEvent) {
        // Re-derive content area from terminal layout matching handle_mouse.
        let term_h = crossterm::terminal::size().map(|(_, h)| h).unwrap_or(40);
        let inner = Rect::new(2, 1, self.terminal_width.saturating_sub(4), term_h.saturating_sub(2));
        let frame_chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
        let content_area = frame_chunks[2];
        let settings_chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(content_area);
        let settings_content = crate::ui::settings::content_area(settings_chunks[2]);

        if mouse.row < settings_content.y
            || mouse.row >= settings_content.y.saturating_add(settings_content.height)
        {
            return;
        }

        let rel_row = (mouse.row - settings_content.y) as usize;
        let item_count = self.settings_item_count();

        let clicked_item = match self.settings.tab {
            SettingsTab::Llm => {
                let mc = self.settings.model_definitions.len();
                if rel_row >= 2 && rel_row < 2 + mc {
                    Some(rel_row - 2)
                } else if rel_row == 2 + mc {
                    Some(mc)
                } else if rel_row >= 6 + mc && rel_row < 6 + mc + 7 {
                    Some(mc + 1 + (rel_row - 6 - mc))
                } else {
                    None
                }
            }
            SettingsTab::Agents => {
                let sc = self.settings.agent_scripts.len();
                if rel_row >= 2 && rel_row < 6 {
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
                let tc = self.settings.intercept_targets.len();
                if rel_row == 3 + tc {
                    Some(0)
                } else if rel_row == 4 + tc {
                    Some(1)
                } else {
                    None
                }
            }
            SettingsTab::Service => match rel_row {
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
            },
            SettingsTab::About => {
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

    pub(crate) async fn dispatch_settings_model_field(
        &mut self,
        mouse: MouseEvent,
        row: usize,
        body_x: u16,
    ) {
        let Some(ref mut form) = self.settings.model_form else {
            return;
        };
        let show_base_url = form.shows_base_url();
        let model_row = if show_base_url { 3 } else { 2 };
        let rel_col = mouse.column.saturating_sub(body_x) as usize;

        match row {
            0 => {
                form.focused_field = 0;
                let providers = crate::app::sorted_providers();
                // "▸ provider    " is 14 cols; click past label cycles provider.
                if rel_col > 14 {
                    form.provider_idx = (form.provider_idx + 1) % providers.len();
                    let p = providers[form.provider_idx];
                    form.base_url = if p.api_key_optional() {
                        p.base_url().to_string()
                    } else {
                        String::new()
                    };
                }
            }
            1 => {
                form.focused_field = 1;
                if !form.editing_text {
                    form.editing_text = true;
                    form.cursor_pos = form.api_key.len();
                }
            }
            2 if show_base_url => {
                form.focused_field = 2;
                if !form.editing_text {
                    form.editing_text = true;
                    form.cursor_pos = form.base_url.len();
                }
            }
            r if r == model_row => {
                form.focused_field = if show_base_url { 3 } else { 2 };
                if !form.editing_text {
                    form.editing_text = true;
                    form.cursor_pos = form.model_name.len();
                }
            }
            _ => {}
        }
    }
}
