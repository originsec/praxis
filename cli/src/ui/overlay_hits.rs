//! Hit registration for popups and overlays. Called at the end of each
//! overlay render so clicks dispatch through the shared HitLayer.

use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::{
    AddRemoteNodeForm, App, ConfirmKind, Popup, PopupKind, RunOptions, TriggerForm,
};
use crate::ui::chain_form::{ChainFormHitMap, HitRect};
use crate::ui::chrome;
use crate::ui::common::centered_rect_fixed;
use crate::ui::hits::{HintRegistrar, MouseAction, SessionHintAction};
use crate::ui::nodes::sessions_list_rect;
use crate::ui::popup::trigger_form_section_rows;

pub fn register_confirm_hits(app: &App, terminal: Rect, confirm: &crate::app::ConfirmAction) {
    let is_info = matches!(confirm.action, ConfirmKind::Info);
    let width = (confirm.message.len() as u16 + 8)
        .min(terminal.width.saturating_sub(4))
        .max(36);
    let height = 7u16;
    let area = centered_rect_fixed(width, height, terminal);
    let body = chrome::modal_content_rect(area);

    //
    // Backdrop dismiss first; Yes/No registered later so they sit on top.
    // Confirm body layout: message, blank, hints (y yes / n no).
    //
    //
    // Backdrop dismiss first; full confirm panel absorbs clicks so they
    // do not fall through to the chain builder under the dialog; Yes/No
    // registered last so they sit on top.
    //
    app.hits_register(terminal, MouseAction::ConfirmDismiss);
    app.hits_register(area, MouseAction::ConfirmNo);
    if is_info {
        app.hits_register(body, MouseAction::ConfirmDismiss);
    } else {
        let hints_y = body.y.saturating_add(2);
        // " y " (3) + " yes" (4) = 7
        app.hits_register(
            Rect::new(body.x, hints_y, 7, 1),
            MouseAction::ConfirmYes,
        );
        // + "    " (4) then " n " (3) + " no" (3) = 6
        app.hits_register(
            Rect::new(body.x.saturating_add(11), hints_y, 6, 1),
            MouseAction::ConfirmNo,
        );
    }
}

pub fn register_popup_hits(app: &App, terminal: Rect, popup: &Popup) {
    let filtered = popup.filtered_items();
    let item_count =
        filtered
            .len()
            .min(if matches!(popup.kind, PopupKind::CommandPalette) {
                8
            } else {
                12
            });

    let (popup_area, list_area) = match popup.kind {
        PopupKind::ModelSelect | PopupKind::SaveSession => {
            let ic = item_count as u16;
            let ph = ic + 5;
            let max_lw = filtered
                .iter()
                .map(|(_, item)| item.label.len() + item.description.len() + 4)
                .max()
                .unwrap_or(30);
            let pw = (max_lw as u16 + 6)
                .min(terminal.width.saturating_sub(4))
                .max(36);
            let area = centered_rect_fixed(pw, ph, terminal);
            let body = chrome::modal_content_rect(area);
            (area, Rect::new(body.x, body.y, body.width, ic))
        }
        PopupKind::CommandPalette => {
            let ic = item_count as u16;
            let ph = ic + 5;
            let y = terminal.height.saturating_sub(5 + ph);
            let pw = (terminal.width / 2)
                .max(36)
                .min(terminal.width.saturating_sub(4));
            let area = Rect::new(2, y, pw, ph);
            let body = chrome::modal_content_rect(area);
            (area, Rect::new(body.x, body.y, body.width, ic))
        }
    };

    //
    // Backdrop first (below); list rows last so they win the hit test.
    //
    app.hits_register(terminal, MouseAction::PopupDismiss);
    let _ = popup_area;
    for i in 0..item_count {
        app.hits_register(
            Rect::new(list_area.x, list_area.y + i as u16, list_area.width, 1),
            MouseAction::PopupItem(i),
        );
    }
}



pub fn register_run_options_hits(app: &App, area: Rect, opts: &RunOptions) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    let inner = Rect {
        x: chunks[2].x + 1,
        width: chunks[2].width.saturating_sub(2),
        ..chunks[2]
    };

    let node_count = opts.nodes.len();
    let agent_count = opts.agents.len();
    let nodes_start = 1u16;
    for i in 0..node_count {
        app.hits_register(
            Rect::new(inner.x, inner.y + nodes_start + i as u16, inner.width, 1),
            MouseAction::RunOptionsToggle { section: 0, index: i },
        );
    }
    let agents_start = nodes_start + node_count as u16 + 2;
    for i in 0..agent_count {
        app.hits_register(
            Rect::new(inner.x, inner.y + agents_start + i as u16, inner.width, 1),
            MouseAction::RunOptionsToggle { section: 1, index: i },
        );
    }
    if !opts.is_chain {
        let yolo_row = agents_start + agent_count as u16 + 1;
        app.hits_register(
            Rect::new(inner.x, inner.y + yolo_row, inner.width, 1),
            MouseAction::RunOptionsToggle { section: 2, index: 0 },
        );
    }

    let mut reg = HintRegistrar::new(app, chunks[3]);
    reg.chip("^r", MouseAction::RunOptionsRun);
    reg.chip(" run", MouseAction::RunOptionsRun);
    reg.gap(4);
    reg.chip("esc", MouseAction::RunOptionsCancel);
    reg.chip(" cancel", MouseAction::RunOptionsCancel);
}

pub fn register_trigger_form_hits(app: &App, area: Rect, form: &TriggerForm) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    let inner = Rect {
        x: chunks[2].x + 1,
        width: chunks[2].width.saturating_sub(2),
        ..chunks[2]
    };

    for (row, section, cursor) in trigger_form_section_rows(form) {
        if (row as u16) < inner.height {
            app.hits_register(
                Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
                MouseAction::TriggerField { section, cursor },
            );
        }
    }

    let mut reg = HintRegistrar::new(app, chunks[3]);
    reg.chip("^s", MouseAction::TriggerSave);
    reg.chip(" save", MouseAction::TriggerSave);
    reg.gap(4);
    reg.chip("esc", MouseAction::TriggerCancel);
    reg.chip(" cancel", MouseAction::TriggerCancel);
}

pub fn register_add_remote_hits(app: &App, terminal: Rect, _form: &AddRemoteNodeForm) {
    let height = (AddRemoteNodeForm::FIELD_COUNT as u16) + 6;
    let width = 60u16.min(terminal.width.saturating_sub(4));
    let popup_area = centered_rect_fixed(width, height, terminal);
    let body = chrome::modal_content_rect(popup_area);

    for i in 0..AddRemoteNodeForm::FIELD_COUNT {
        app.hits_register(
            Rect::new(body.x, body.y + i as u16, body.width, 1),
            MouseAction::AddRemoteField(i),
        );
    }
    //
    // Hints sit on the last content row of the modal body.
    //
    let hints = Rect {
        y: body.y.saturating_add(body.height.saturating_sub(1)),
        height: 1,
        ..body
    };
    let mut reg = HintRegistrar::new(app, hints);
    reg.chip("^s", MouseAction::AddRemoteSave);
    reg.chip(" save", MouseAction::AddRemoteSave);
}

pub fn register_sessions_list_hits(app: &App, area: Rect, count: usize) {
    let panel = sessions_list_rect(area, count);
    let rows_start = panel.y + 3;
    let rows_end = panel.y + panel.height.saturating_sub(2);
    for i in 0..count {
        let row = rows_start + i as u16;
        if row < rows_end {
            app.hits_register(
                Rect::new(panel.x, row, panel.width, 1),
                MouseAction::SessionsListRow(i),
            );
        }
    }
    app.hits_register(area, MouseAction::SessionsListDismiss);
}

pub fn register_session_chat_hits(app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // messages
        Constraint::Length(1), // spacer
        Constraint::Length(3), // input
        Constraint::Length(1), // hints
    ])
    .split(area);
    //
    // Input: LEFT border (1) + pad (1) + "▸ " (2) → text starts at x+4.
    //
    let text_start = chunks[4].x.saturating_add(4);
    app.hits_register(
        chunks[4],
        MouseAction::SessionInput { text_start },
    );
    // Match render: "↵ send    ^w suspend    ^c close"
    let mut reg = HintRegistrar::new(app, chunks[5]);
    reg.chip("\u{21b5}", MouseAction::SessionHint(SessionHintAction::Send));
    reg.chip(" send", MouseAction::SessionHint(SessionHintAction::Send));
    reg.gap(4);
    reg.chip("^w", MouseAction::SessionHint(SessionHintAction::Pause));
    reg.chip(" suspend", MouseAction::SessionHint(SessionHintAction::Pause));
    reg.gap(4);
    reg.chip("^c", MouseAction::SessionHint(SessionHintAction::Close));
    reg.chip(" close", MouseAction::SessionHint(SessionHintAction::Close));
}

pub fn register_session_options_hits(app: &App, area: Rect, dir_count: usize) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Length(1), // divider
        Constraint::Min(1),    // body
        Constraint::Length(1), // hints
    ])
    .split(area);
    let body = chunks[2];
    app.hits_register(
        Rect::new(body.x, body.y, body.width, 1),
        MouseAction::SessionOptionsRow(0),
    );
    for i in 0..dir_count {
        app.hits_register(
            Rect::new(body.x, body.y + 3 + i as u16, body.width, 1),
            MouseAction::SessionOptionsRow(3 + i),
        );
    }
    // Match render: "↑↓ navigate    tab toggle    ↵ start    esc cancel"
    let mut reg = HintRegistrar::new(app, chunks[3]);
    reg.gap(11); // "↑↓ navigate"
    reg.gap(4);
    reg.gap(10); // "tab toggle"
    reg.gap(4);
    reg.chip("\u{21b5}", MouseAction::SessionOptionsConfirm);
    reg.chip(" start", MouseAction::SessionOptionsConfirm);
    reg.gap(4);
    reg.chip("esc", MouseAction::SessionOptionsCancel);
    reg.chip(" cancel", MouseAction::SessionOptionsCancel);
}

pub fn register_settings_content_hits(app: &App, content: Rect) {
    app.hits_register(content, MouseAction::SettingsContentClick);
}

pub fn register_settings_model_form_hits(
    app: &App,
    area: Rect,
    form: &crate::app::ModelEditForm,
) {
    let show_base_url = form.shows_base_url();
    let field_count = if show_base_url { 4u16 } else { 3u16 };
    //
    // Match render_model_form: base_lines = field_count + 6 (chrome +
    // fields + blank + hints), plus optional dropdown rows.
    //
    let base_lines = field_count + 6;
    let dropdown_extra = if form.model_dropdown_open {
        1 + form.available_models.len() as u16
    } else if form.loading_models {
        1
    } else {
        0
    };
    let popup_h = (base_lines + dropdown_extra).min(area.height.saturating_sub(4));
    let popup_w = 60u16.min(area.width.saturating_sub(4));
    let px = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let py = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup_area = Rect::new(px, py, popup_w, popup_h);
    let body = chrome::modal_content_rect(popup_area);

    let model_row = if show_base_url { 3u16 } else { 2 };
    let hints_row = model_row + 2; // blank after fields, then hints

    for row in 0..field_count {
        app.hits_register(
            Rect::new(body.x, body.y + row, body.width, 1),
            MouseAction::SettingsModelField {
                row: row as usize,
                body_x: body.x,
            },
        );
    }
    // "^s save    esc cancel"
    app.hits_register(
        Rect::new(body.x, body.y + hints_row, 7, 1),
        MouseAction::SettingsModelSave,
    );
    app.hits_register(
        Rect::new(body.x.saturating_add(11), body.y + hints_row, 10, 1),
        MouseAction::SettingsModelCancel,
    );
    if form.model_dropdown_open && !form.available_models.is_empty() {
        // fields + blank + hints [+ loading] + blank → dropdown rows
        let mut header_lines = field_count + 2; // fields + blank + hints
        if form.loading_models {
            header_lines += 1;
        }
        header_lines += 1; // blank before dropdown
        let dropdown_y = body.y + header_lines;
        let visible_h = body
            .height
            .saturating_sub(header_lines)
            .max(1) as usize;
        let scroll = form.model_dropdown_scroll as usize;
        for vis in 0..visible_h {
            let i = scroll + vis;
            if i >= form.available_models.len() {
                break;
            }
            app.hits_register(
                Rect::new(body.x, dropdown_y + vis as u16, body.width, 1),
                MouseAction::SettingsModelDropdownItem(i),
            );
        }
    }
}

pub fn register_settings_dropdown_hits(app: &App, area: Rect, state: &crate::app::SettingsState) {
    let item_count = state.model_definitions.len();
    if item_count == 0 {
        return;
    }
    // Match render_model_dropdown: height = items + 4 chrome rows.
    let popup_h = (item_count as u16 + 4).min(area.height.saturating_sub(4));
    let max_label = state
        .model_definitions
        .iter()
        .map(|d| {
            if d.name.is_empty() {
                d.provider.len() + 2 + d.model.len()
            } else {
                d.name.len()
            }
        })
        .max()
        .unwrap_or(20) as u16;
    let popup_w = (max_label + 6).max(20).min(area.width.saturating_sub(4));
    let px = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let py = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup_area = Rect::new(px, py, popup_w, popup_h);
    let body = chrome::modal_content_rect(popup_area);

    app.hits_register(area, MouseAction::SettingsDropdownDismiss);
    for i in 0..item_count {
        app.hits_register(
            Rect::new(body.x, body.y + i as u16, body.width, 1),
            MouseAction::SettingsDropdownRow(i),
        );
    }
}

pub fn register_chain_form_hits(app: &App, hit: &ChainFormHitMap) {
    fn reg(app: &App, rect: &HitRect, action: MouseAction) {
        app.hits_register(
            Rect::new(rect.x, rect.y, rect.w, rect.h),
            action,
        );
    }

    //
    // Bottom → top. Canvas first so later registrations (header, palette,
    // and especially the properties modal) win when they overlap.
    //
    reg(app, &hit.canvas, MouseAction::ChainCanvas);
    reg(app, &hit.save_button, MouseAction::ChainSave);
    reg(app, &hit.cancel_button, MouseAction::ChainCancel);
    reg(app, &hit.auto_layout_button, MouseAction::ChainAutoLayout);
    for (kind, rect) in &hit.palette_buttons {
        reg(app, rect, MouseAction::ChainPalette(*kind));
    }
    for (target, rect) in &hit.header_fields {
        reg(app, rect, MouseAction::ChainEdit(target.clone()));
    }
    //
    // Properties modal surface + fields last so they always beat the
    // canvas under the centered popup.
    //
    reg(app, &hit.props_modal_rect, MouseAction::ChainPropsSurface);
    for (target, rect) in &hit.property_fields {
        reg(app, rect, MouseAction::ChainEdit(target.clone()));
    }
    reg(app, &hit.kind_cycle_button, MouseAction::ChainCycleKind);
    reg(app, &hit.delete_element_button, MouseAction::ChainDeleteElement);
    reg(app, &hit.cycle_condition_button, MouseAction::ChainCycleCondition);
    reg(app, &hit.delete_connection_button, MouseAction::ChainDeleteConnection);
    reg(app, &hit.pick_op_button, MouseAction::ChainPickOp);
    reg(app, &hit.pick_model_button, MouseAction::ChainPickModel);
    reg(app, &hit.pick_tool_button, MouseAction::ChainPickTool);
    reg(app, &hit.pick_payload_button, MouseAction::ChainPickPayload);
    reg(app, &hit.pick_session_group_button, MouseAction::ChainPickSessionGroup);
    reg(app, &hit.cycle_memory_mode_button, MouseAction::ChainCycleMemoryMode);
    reg(app, &hit.toggle_session_yolo_button, MouseAction::ChainToggleSessionYolo);
    reg(app, &hit.cycle_block_yolo_button, MouseAction::ChainCycleBlockYolo);
    reg(app, &hit.cycle_require_all_button, MouseAction::ChainCycleRequireAll);
}

pub fn register_chain_editor_hits(
    app: &App,
    area: Rect,
    form: &crate::app::ChainForm,
    editor: &crate::app::ChainFormEditor,
) {
    use crate::app::ChainFormEditor;

    let (cursor, filter, list, has_filter): (usize, String, Vec<String>, bool) = match editor {
        ChainFormEditor::PickOpName { cursor, filter } => (
            *cursor,
            filter.clone(),
            form.available_op_names.clone(),
            true,
        ),
        ChainFormEditor::PickModel { cursor, filter } => {
            (*cursor, filter.clone(), form.available_models.clone(), true)
        }
        ChainFormEditor::PickTool { cursor, filter } => {
            (*cursor, filter.clone(), form.available_tools.clone(), true)
        }
        ChainFormEditor::PickPayload { cursor, filter } => (
            *cursor,
            filter.clone(),
            form.available_payloads.clone(),
            true,
        ),
        ChainFormEditor::PickSessionGroup { cursor } => {
            let mut items = vec!["(none)".to_string(), "(new group)".to_string()];
            for g in crate::app::collect_session_groups(form) {
                items.push(g.id);
            }
            (*cursor, String::new(), items, false)
        }
    };

    let popup_w = 60u16.min(area.width.saturating_sub(4));
    let popup_h = 16u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width - popup_w) / 2;
    let y = area.y + (area.height - popup_h) / 2;
    let rect = Rect::new(x, y, popup_w, popup_h);
    let inner = Rect {
        x: rect.x + 1,
        y: rect.y + 1,
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    };

    app.hits_register(area, MouseAction::ChainEditorDismiss);

    let filtered: Vec<&String> = list
        .iter()
        .filter(|n| filter.is_empty() || n.to_lowercase().contains(&filter.to_lowercase()))
        .collect();
    // title (+ optional filter) + blank, then rows — matches render_picker.
    let list_start_y = if has_filter {
        inner.y + 3
    } else {
        inner.y + 2
    };
    let max_rows = (inner.height as usize).saturating_sub(5);
    let start = if cursor >= max_rows && max_rows > 0 {
        cursor + 1 - max_rows
    } else {
        0
    };
    for vis in 0..max_rows {
        let i = start + vis;
        if i >= filtered.len() {
            break;
        }
        app.hits_register(
            Rect::new(inner.x, list_start_y + vis as u16, inner.width, 1),
            MouseAction::ChainPickOpItem(i),
        );
    }
}

