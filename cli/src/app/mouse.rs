use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;


use crate::app::{
    log_query::LogQueryFocus, AddRemoteNodeForm, App, ConfirmAction, ConfirmKind, OpsTab, Window,
};
use crate::ui::common::{drag_split_percent, drag_top_height, table_row_at};
use crate::ui::hits::{MouseAction, NodesHintAction, OpsHintAction, RowSelectKind};
use crate::app::log_query::{EDITOR_HEIGHT_MAX, EDITOR_HEIGHT_MIN};

impl App {
    fn is_overlay_action(action: &MouseAction) -> bool {
        matches!(
            action,
            MouseAction::ConfirmYes
                | MouseAction::ConfirmNo
                | MouseAction::ConfirmDismiss
                | MouseAction::PopupItem(_)
                | MouseAction::PopupDismiss
                | MouseAction::NewOpField(_)
                | MouseAction::NewOpSave
                | MouseAction::NewOpCancel
                | MouseAction::RunOptionsToggle { .. }
                | MouseAction::RunOptionsRun
                | MouseAction::RunOptionsCancel
                | MouseAction::TriggerSave
                | MouseAction::TriggerCancel
                | MouseAction::TriggerField { .. }
                | MouseAction::AddRemoteField(_)
                | MouseAction::AddRemoteSave
                | MouseAction::SessionsListRow(_)
                | MouseAction::SessionsListDismiss
                | MouseAction::SessionInput { .. }
                | MouseAction::SessionHint(_)
                | MouseAction::SessionOptionsRow(_)
                | MouseAction::SessionOptionsConfirm
                | MouseAction::SessionOptionsCancel
                | MouseAction::SettingsContentClick
                | MouseAction::SettingsModelField { .. }
                | MouseAction::SettingsModelDropdownItem(_)
                | MouseAction::SettingsModelSave
                | MouseAction::SettingsModelCancel
                | MouseAction::SettingsDropdownRow(_)
                | MouseAction::SettingsDropdownDismiss
                | MouseAction::ChainSave
                | MouseAction::ChainCancel
                | MouseAction::ChainAutoLayout
                | MouseAction::ChainPalette(_)
                | MouseAction::ChainEdit(_)
                | MouseAction::ChainCycleKind
                | MouseAction::ChainDeleteElement
                | MouseAction::ChainCycleCondition
                | MouseAction::ChainDeleteConnection
                | MouseAction::ChainPickOp
                | MouseAction::ChainPickModel
                | MouseAction::ChainPickTool
                | MouseAction::ChainPickPayload
                | MouseAction::ChainPickSessionGroup
                | MouseAction::ChainCycleMemoryMode
                | MouseAction::ChainToggleSessionYolo
                | MouseAction::ChainCycleBlockYolo
                | MouseAction::ChainCycleRequireAll
                | MouseAction::ChainPickOpItem(_)
                | MouseAction::ChainCanvas
                | MouseAction::ChainPropsSurface
                | MouseAction::ChainEditorDismiss
                | MouseAction::InterceptRuleField(_)
                | MouseAction::InterceptRuleSave
                | MouseAction::InterceptRuleCancel
                | MouseAction::ReconTab(_)
                | MouseAction::ReconLeftPane
                | MouseAction::ReconRightPane
                | MouseAction::ReconSplitDragStart
                | MouseAction::ReconHint(_)
                | MouseAction::ReconTreeRow { .. }
                | MouseAction::ReconTreeChevron { .. }
                | MouseAction::ReconFilterBar
        )
    }

    pub(crate) async fn dispatch_mouse_action(
        &mut self,
        mouse: MouseEvent,
        action: MouseAction,
    ) -> bool {
        if Self::is_overlay_action(&action) {
            return self.dispatch_overlay_action(mouse, action).await;
        }

        match action {
            MouseAction::SwitchWindow(win) => {
                self.active_window = win;
                match win {
                    Window::Nodes => self.refresh_node_sessions(),
                    Window::Operations => self.refresh_operations(),
                    Window::Intercept => self.enter_intercept().await,
                    _ => {}
                }
                true
            }
            MouseAction::Quit => {
                self.should_quit = true;
                true
            }

            MouseAction::InterceptTab(tab) => {
                self.intercept.tab = tab;
                // Drop other tabs' detail focus so wheel scroll cannot
                // target a pane that is not on screen.
                self.intercept.detail_focus = false;
                self.intercept.match_detail_focus = false;
                self.intercept.rule_detail_focus = false;
                true
            }
            MouseAction::InterceptLogDetailFocus => {
                self.intercept.detail_focus = true;
                self.intercept.match_detail_focus = false;
                self.intercept.rule_detail_focus = false;
                self.fetch_body_for_selected().await;
                true
            }
            MouseAction::InterceptMatchDetailFocus => {
                self.intercept.match_detail_focus = true;
                self.intercept.detail_focus = false;
                self.intercept.rule_detail_focus = false;
                self.fetch_body_for_match_selected().await;
                true
            }
            MouseAction::InterceptRuleDetailFocus => {
                self.intercept.rule_detail_focus = true;
                self.intercept.detail_focus = false;
                self.intercept.match_detail_focus = false;
                true
            }
            MouseAction::InterceptLogSplitDragStart => {
                self.intercept.log_dragging = true;
                true
            }
            MouseAction::InterceptMatchSplitDragStart => {
                self.intercept.match_dragging = true;
                true
            }
            MouseAction::InterceptRuleSplitDragStart => {
                self.intercept.rule_dragging = true;
                true
            }
            MouseAction::InterceptCycleNodeFilter => {
                self.cycle_node_filter();
                true
            }
            MouseAction::InterceptCycleAgentFilter => {
                self.cycle_agent_filter();
                true
            }
            MouseAction::InterceptCycleBodyMode => {
                self.intercept.body_mode = self.intercept.body_mode.cycle();
                true
            }

            MouseAction::OpsTab(tab) => {
                let prev = self.operations.tab;
                self.operations.tab = tab;
                if tab != prev {
                    self.operations.filter.clear();
                    if tab == OpsTab::Triggers {
                        self.refresh_triggers_after(std::time::Duration::ZERO);
                    }
                }
                true
            }
            MouseAction::OpsDetailFocus => {
                self.operations.detail_focus = true;
                true
            }
            MouseAction::OpsExecDetail { inner } => {
                self.operations.detail_focus = true;
                if self.operations.tab == OpsTab::Executions
                    && mouse.column >= inner.x
                    && mouse.column < inner.x.saturating_add(inner.width)
                    && mouse.row >= inner.y
                    && mouse.row < inner.y.saturating_add(inner.height)
                {
                    let visual_row = mouse
                        .row
                        .saturating_sub(inner.y)
                        .saturating_add(self.operations.detail_scroll);
                    if let Some(section_idx) =
                        crate::ui::operations::execution_detail_section_at_row(
                            &self.operations,
                            inner.width,
                            visual_row,
                        )
                    {
                        self.operations.collapsed.focused_section = section_idx;
                        if section_idx < self.operations.collapsed.sections.len() {
                            self.operations.collapsed.sections[section_idx] =
                                !self.operations.collapsed.sections[section_idx];
                        }
                    }
                }
                true
            }
            MouseAction::OpsSplitDragStart => {
                self.operations.dragging = true;
                true
            }
            MouseAction::OpsHint(hint) => self.dispatch_ops_hint(hint).await,

            MouseAction::LogQueryEditorSplitDragStart => {
                self.log_query.editor_dragging = true;
                true
            }
            MouseAction::LogQueryResultsSplitDragStart => {
                self.log_query.results_dragging = true;
                true
            }

            MouseAction::NodesDetailFocus => {
                self.nodes.detail_focus = true;
                true
            }
            MouseAction::NodesAgentRow { agents_start } => {
                self.nodes.detail_focus = true;
                let is_dbl = self.is_double_click(mouse.row, mouse.column);
                let agent_count = self
                    .nodes
                    .nodes
                    .get(self.nodes.selected)
                    .map(|n| n.discovered_agents.len())
                    .unwrap_or(0);
                if mouse.row >= agents_start && mouse.row < agents_start + agent_count as u16 {
                    self.nodes.agent_selected = (mouse.row - agents_start) as usize;
                    if is_dbl {
                        self.start_session_with_selected_agent();
                    }
                }
                true
            }
            MouseAction::NodesSplitDragStart => {
                self.nodes.dragging = true;
                true
            }
            MouseAction::NodesHint(hint) => self.dispatch_nodes_hint(hint),

            MouseAction::SettingsTab(tab) => {
                self.switch_settings_tab(tab).await;
                true
            }

            MouseAction::LogQueryFocus(focus) => {
                self.log_query.focus = focus;
                true
            }
            MouseAction::LogQuerySchemaDismiss => {
                self.log_query.schema_open = false;
                true
            }

            MouseAction::OrchestratorTab(i) => {
                self.orchestrator.active_session_index = Some(i);
                true
            }
            MouseAction::OrchestratorModelSelect => {
                self.open_model_select().await;
                true
            }
            MouseAction::OrchestratorToolsCycle => {
                self.cycle_tools_display();
                true
            }
            MouseAction::OrchestratorSaveSession => {
                self.open_save_session();
                true
            }
            MouseAction::OrchestratorPlanSplitDragStart => {
                self.orchestrator.plan_dragging = true;
                true
            }
            MouseAction::OrchestratorInputCursor { text_start } => {
                let is_streaming = self
                    .orchestrator
                    .active_session()
                    .map(|s| s.is_streaming)
                    .unwrap_or(false);
                if !is_streaming {
                    let click_offset = mouse.column.saturating_sub(text_start) as usize;
                    let len = self.orchestrator.input.len();
                    self.orchestrator.cursor_pos = click_offset.min(len);
                }
                true
            }

            MouseAction::SelectRow(sel) => self.dispatch_row_select(mouse, sel).await,

            // Overlay / recon actions are handled via is_overlay_action.
            _ => false,
        }
    }

    async fn dispatch_row_select(&mut self, mouse: MouseEvent, sel: crate::ui::hits::RowSelect) -> bool {
        let Some(clicked) = table_row_at(sel.table_area, sel.data_start, mouse.row) else {
            return false;
        };

        let is_dbl = self.is_double_click(mouse.row, mouse.column);

        match sel.kind {
            RowSelectKind::InterceptLog => {
                self.intercept.detail_focus = false;
                if clicked < self.intercept.display_rows.len() {
                    self.intercept.selected = clicked;
                    self.intercept.detail_scroll = 0;
                    self.intercept.group_frame_selected = 0;
                    self.fetch_body_for_selected().await;
                }
                true
            }
            RowSelectKind::InterceptMatch => {
                self.intercept.match_detail_focus = false;
                let total = self.intercept.filtered_matches_len();
                if clicked < total {
                    self.intercept.match_selected = clicked;
                    self.intercept.match_detail_scroll = 0;
                    self.fetch_body_for_match_selected().await;
                }
                true
            }
            RowSelectKind::InterceptRule => {
                self.intercept.rule_detail_focus = false;
                let ids = self.intercept.filtered_rule_ids();
                if clicked < ids.len() {
                    self.intercept.rule_selected_id = Some(ids[clicked]);
                    self.intercept.rule_detail_scroll = 0;
                }
                true
            }
            RowSelectKind::NodesList => {
                if clicked < self.nodes.nodes.len() {
                    self.nodes.selected = clicked;
                    self.nodes.detail_focus = false;
                }
                true
            }
            RowSelectKind::OpsLibrary => {
                let total = self.ops_library_count();
                if clicked < total {
                    self.operations.library_selected = clicked;
                    self.operations.detail_focus = false;
                    if is_dbl {
                        self.open_run_target_popup();
                    }
                }
                true
            }
            RowSelectKind::OpsExecutions => {
                let total = self.sorted_executions().len();
                if clicked < total {
                    self.operations.exec_selected = clicked;
                    self.operations.detail_scroll = 0;
                    self.operations.detail_focus = false;
                }
                true
            }
            RowSelectKind::OpsTriggers => {
                let total = self.operations.triggers.len();
                if clicked < total {
                    self.operations.trigger_selected = clicked;
                    self.operations.detail_focus = false;
                    if is_dbl {
                        self.edit_selected_trigger();
                    }
                }
                true
            }
            RowSelectKind::LogQueryResults => {
                self.log_query.focus = LogQueryFocus::Results;
                let n = self.log_query.visible_row_count();
                if clicked < n {
                    self.log_query.selected_row = clicked;
                }
                true
            }
        }
    }

    async fn dispatch_ops_hint(&mut self, hint: OpsHintAction) -> bool {
        match hint {
            OpsHintAction::Execute => self.open_run_target_popup(),
            OpsHintAction::NewOp => self.open_new_op_form(),
            OpsHintAction::NewChain => self.open_new_chain_form(),
            OpsHintAction::Edit => {
                let filtered = self.filtered_library();
                match filtered.get(self.operations.library_selected) {
                    Some(&(_, true)) => self.edit_selected_chain(),
                    Some(&(_, false)) => self.edit_selected_op(),
                    None => {}
                }
            }
            OpsHintAction::Delete => self.delete_selected_library_row().await,
            OpsHintAction::CancelExecution => self.cancel_selected_execution().await,
            OpsHintAction::DeleteExecution => self.delete_selected_execution().await,
            OpsHintAction::ClearAllExecutions => {
                self.confirm = Some(ConfirmAction {
                    message: "Clear all executions?".to_string(),
                    action: ConfirmKind::ClearAllExecutions,
                });
            }
            OpsHintAction::ToggleTrigger => self.toggle_selected_trigger_enabled().await,
            OpsHintAction::NewTrigger => self.open_new_trigger_form(),
            OpsHintAction::EditTrigger => self.edit_selected_trigger(),
            OpsHintAction::DeleteTrigger => self.delete_selected_trigger().await,
        }
        true
    }

    fn dispatch_nodes_hint(&mut self, hint: NodesHintAction) -> bool {
        match hint {
            NodesHintAction::SelectDetail => {
                self.nodes.detail_focus = true;
                self.nodes.agent_selected = 0;
            }
            NodesHintAction::StartSession => self.start_session_with_selected_agent(),
            NodesHintAction::Recon => {
                if let Some(node) = self.nodes.nodes.get(self.nodes.selected) {
                    if let Some(agent) = node.discovered_agents.get(self.nodes.agent_selected) {
                        self.open_recon(node.node_id.clone(), agent.short_name.clone());
                    }
                }
            }
            NodesHintAction::Reset => self.confirm_reset_node(),
            NodesHintAction::Remove => self.confirm_delete_node(),
            NodesHintAction::AddRemote => {
                self.add_remote_node_form = Some(AddRemoteNodeForm {
                    focused_field: AddRemoteNodeForm::URL_FIELD,
                    editing_text: true,
                    ..AddRemoteNodeForm::default()
                });
            }
            NodesHintAction::Terminal => self.open_terminal(),
            NodesHintAction::Sessions => self.toggle_sessions_list(),
        }
        true
    }

    pub(crate) async fn handle_window_mouse_drag(&mut self, mouse: MouseEvent, content_area: Rect) {
        use crate::ui::intercept::{chrome_layout, filter_split, show_footer};

        match mouse.kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.intercept.log_dragging {
                    let chrome = chrome_layout(content_area, show_footer(self));
                    let panes = filter_split(chrome.body, self.intercept.log_split_percent);
                    self.intercept.log_split_percent = drag_split_percent(
                        panes.filter.x,
                        panes.filter.width,
                        mouse.column,
                    );
                } else if self.intercept.match_dragging {
                    let chrome = chrome_layout(content_area, show_footer(self));
                    let panes = filter_split(chrome.body, self.intercept.match_split_percent);
                    self.intercept.match_split_percent = drag_split_percent(
                        panes.filter.x,
                        panes.filter.width,
                        mouse.column,
                    );
                } else if self.intercept.rule_dragging {
                    let chrome = chrome_layout(content_area, show_footer(self));
                    let panes = filter_split(chrome.body, self.intercept.rule_split_percent);
                    self.intercept.rule_split_percent = drag_split_percent(
                        panes.filter.x,
                        panes.filter.width,
                        mouse.column,
                    );
                } else if self.nodes.dragging {
                    use ratatui::layout::{Constraint, Layout};
                    let outer =
                        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(content_area);
                    let panes = crate::ui::list_detail::layout(outer[0], self.nodes.split_percent);
                    self.nodes.split_percent = drag_split_percent(
                        panes.list.x,
                        panes.list.width.saturating_add(panes.detail.width),
                        mouse.column,
                    );
                    self.nodes.split_percent_user_set = true;
                } else if self.operations.dragging {
                    use ratatui::layout::{Constraint, Layout};
                    let ops_chunks = Layout::vertical([
                        Constraint::Length(1), // tabs
                        Constraint::Length(1), // divider
                        Constraint::Length(1), // filter
                        Constraint::Min(1),    // content
                        Constraint::Length(1), // hints
                    ])
                    .split(content_area);
                    let main_area = ops_chunks[3];
                    self.operations.split_percent = drag_split_percent(
                        main_area.x,
                        main_area.width,
                        mouse.column,
                    );
                } else if self
                    .nodes
                    .recon
                    .as_ref()
                    .is_some_and(|r| r.recon_dragging)
                {
                    let areas = crate::ui::recon::recon_areas(content_area);
                    if let Some(recon) = self.nodes.recon.as_mut() {
                        recon.recon_split_percent = drag_split_percent(
                            areas.content.x,
                            areas.content.width,
                            mouse.column,
                        );
                    }
                } else if self.orchestrator.plan_dragging {
                    //
                    // Conversation | plan: same chrome as the render path
                    // (tabs may take a row; main area is chunks[1]).
                    //
                    use ratatui::layout::{Constraint, Layout};
                    let show_tabs = self.orchestrator.sessions.len() > 1;
                    let tab_h = if show_tabs { 1u16 } else { 0 };
                    let input_lines = crate::ui::orchestrator::input_content_rows(&self.orchestrator)
                        .min(12);
                    let input_h = (input_lines + 2).max(3);
                    let chunks = Layout::vertical([
                        Constraint::Length(tab_h),
                        Constraint::Min(1),
                        Constraint::Length(1),
                        Constraint::Length(input_h),
                        Constraint::Length(1),
                        Constraint::Length(1),
                    ])
                    .split(content_area);
                    let main = chunks[1];
                    self.orchestrator.plan_split_percent =
                        drag_split_percent(main.x, main.width, mouse.column);
                } else if self.log_query.editor_dragging {
                    //
                    // Vertical drag: editor height from content top.
                    // Leave room for results (min ~4) + hints (1).
                    //
                    let max_h = content_area
                        .height
                        .saturating_sub(5)
                        .min(EDITOR_HEIGHT_MAX)
                        .max(EDITOR_HEIGHT_MIN);
                    self.log_query.editor_height = drag_top_height(
                        content_area.y,
                        mouse.row,
                        EDITOR_HEIGHT_MIN,
                        max_h,
                    );
                } else if self.log_query.results_dragging {
                    use ratatui::layout::{Constraint, Layout};
                    let show_error = self.log_query.last_error.is_some();
                    let editor_h = self
                        .log_query
                        .editor_height
                        .clamp(EDITOR_HEIGHT_MIN, EDITOR_HEIGHT_MAX);
                    let chunks = Layout::vertical([
                        Constraint::Length(editor_h),
                        Constraint::Length(if show_error { 1 } else { 0 }),
                        Constraint::Min(1),
                        Constraint::Length(1),
                    ])
                    .split(content_area);
                    let results = chunks[2];
                    self.log_query.results_split_percent =
                        drag_split_percent(results.x, results.width, mouse.column);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.intercept.log_dragging = false;
                self.intercept.match_dragging = false;
                self.intercept.rule_dragging = false;
                self.nodes.dragging = false;
                self.operations.dragging = false;
                self.log_query.editor_dragging = false;
                self.log_query.results_dragging = false;
                self.orchestrator.plan_dragging = false;
                if let Some(recon) = self.nodes.recon.as_mut() {
                    recon.recon_dragging = false;
                }
            }
            _ => {}
        }
    }

    pub(crate) async fn handle_hit_layer_mouse(
        &mut self,
        mouse: MouseEvent,
        content_area: Rect,
        _terminal_area: Rect,
    ) {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            if let Some(action) = self.hits_lookup(mouse.column, mouse.row) {
                let _ = self.dispatch_mouse_action(mouse, action).await;
            }
            return;
        }

        if matches!(mouse.kind, MouseEventKind::Moved) {
            let hover = match self.hits_lookup(mouse.column, mouse.row) {
                Some(MouseAction::ReconTreeRow { row })
                | Some(MouseAction::ReconTreeChevron { row }) => Some(row),
                _ => None,
            };
            if let Some(recon) = self.nodes.recon.as_mut() {
                recon.hovered_row = hover;
            }
            if self.chain_form.is_some() {
                self.handle_chain_form_motion(mouse);
            }
            return;
        }

        if self.chain_form.is_some() {
            self.handle_chain_form_motion(mouse);
            return;
        }

        if matches!(
            mouse.kind,
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        ) {
            self.handle_window_mouse_drag(mouse, content_area).await;
        }
    }
}