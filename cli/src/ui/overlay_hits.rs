//! Hit registration for popups and overlays. Called at the end of each
//! overlay render so clicks dispatch through the shared HitLayer.

use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::{
    AddRemoteNodeForm, App, ConfirmKind, NewOpForm, Popup, PopupKind, RunOptions, TriggerForm,
};
use crate::ui::chain_form::{ChainFormHitMap, HitRect};
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

    if is_info {
        app.hits_register(terminal, MouseAction::ConfirmDismiss);
    } else {
        app.hits_register(terminal, MouseAction::ConfirmDismiss);
        let body = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(4),
            width: area.width.saturating_sub(2),
            height: 1,
        };
        app.hits_register(
            Rect::new(body.x.saturating_add(1), body.y, 6, 1),
            MouseAction::ConfirmYes,
        );
        app.hits_register(
            Rect::new(body.x.saturating_add(8), body.y, 5, 1),
            MouseAction::ConfirmNo,
        );
    }
}

pub fn register_popup_hits(app: &App, terminal: Rect, popup: &Popup) {
    let filtered = popup.filtered_items();
    let item_count = filtered.len().min(if matches!(popup.kind, PopupKind::CommandPalette) { 8 } else { 12 });

    let (list_area, backdrop) = match popup.kind {
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
            let x = (terminal.width.saturating_sub(pw)) / 2;
            let y = (terminal.height.saturating_sub(ph)) / 2;
            let inner_y = y + 2;
            (
                Rect::new(x + 1, inner_y, pw.saturating_sub(2), ic),
                terminal,
            )
        }
        PopupKind::CommandPalette => {
            let ic = item_count as u16;
            let ph = ic + 5;
            let y = terminal.height.saturating_sub(5 + ph);
            let pw = (terminal.width / 2).max(36).min(terminal.width.saturating_sub(4));
            let inner_y = y + 2;
            (
                Rect::new(3, inner_y, pw.saturating_sub(2), ic),
                terminal,
            )
        }
    };

    for i in 0..item_count {
        app.hits_register(
            Rect::new(list_area.x, list_area.y + i as u16, list_area.width, 1),
            MouseAction::PopupItem(i),
        );
    }
    app.hits_register(backdrop, MouseAction::PopupDismiss);
}

pub fn register_new_op_form_hits(app: &App, area: Rect, form: &NewOpForm) {
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

    let is_agent = form.mode == 1;
    let field_rows: &[(usize, u16)] = if is_agent {
        &[(0, 0), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7), (7, 9), (8, 10)]
    } else {
        &[(0, 0), (1, 2), (2, 3), (3, 4), (4, 5), (6, 6), (7, 8), (8, 9)]
    };

    for &(field, row) in field_rows {
        if row < inner.height {
            app.hits_register(
                Rect::new(inner.x, inner.y + row, inner.width, 1),
                MouseAction::NewOpField(field),
            );
        }
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

pub fn register_add_remote_hits(app: &App, terminal: Rect, form: &AddRemoteNodeForm) {
    let height = (AddRemoteNodeForm::FIELD_COUNT as u16) + 6;
    let width = 60u16.min(terminal.width.saturating_sub(4));
    let popup_area = centered_rect_fixed(width, height, terminal);
    let body = Rect {
        x: popup_area.x + 1,
        y: popup_area.y + 2,
        width: popup_area.width.saturating_sub(2),
        height: popup_area.height.saturating_sub(4),
    };

    for i in 0..AddRemoteNodeForm::FIELD_COUNT {
        app.hits_register(
            Rect::new(body.x, body.y + i as u16, body.width, 1),
            MouseAction::AddRemoteField(i),
        );
    }
    let hints = Rect {
        y: popup_area.y + popup_area.height.saturating_sub(2),
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
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(area);
    let text_start = chunks[4].x + 5;
    app.hits_register(
        chunks[4],
        MouseAction::SessionInput { text_start },
    );
    let mut reg = HintRegistrar::new(app, chunks[5]);
    reg.gap(2);
    reg.chip("\u{21b5}", MouseAction::SessionHint(SessionHintAction::Send));
    reg.chip(" send", MouseAction::SessionHint(SessionHintAction::Send));
    reg.gap(4);
    reg.chip("^w", MouseAction::SessionHint(SessionHintAction::Pause));
    reg.chip(" pause", MouseAction::SessionHint(SessionHintAction::Pause));
    reg.gap(4);
    reg.chip("^c", MouseAction::SessionHint(SessionHintAction::Close));
    reg.chip(" close", MouseAction::SessionHint(SessionHintAction::Close));
}

pub fn register_session_options_hits(app: &App, area: Rect, dir_count: usize) {
    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    let inner = Rect {
        x: chunks[1].x + 2,
        width: chunks[1].width.saturating_sub(4),
        ..chunks[1]
    };
    app.hits_register(
        Rect::new(inner.x, inner.y, inner.width, 1),
        MouseAction::SessionOptionsRow(0),
    );
    for i in 0..dir_count {
        app.hits_register(
            Rect::new(inner.x, inner.y + 3 + i as u16, inner.width, 1),
            MouseAction::SessionOptionsRow(3 + i),
        );
    }
    let mut reg = HintRegistrar::new(app, chunks[2]);
    reg.gap(27);
    reg.chip("enter", MouseAction::SessionOptionsConfirm);
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
    let base_lines = field_count + 2 + 2;
    let dropdown_extra = if form.model_dropdown_open {
        1 + form.available_models.len() as u16
    } else if form.loading_models {
        1
    } else {
        0
    };
    let popup_h = (base_lines + dropdown_extra).min(area.height.saturating_sub(4));
    let popup_w = 60u16.min(area.width.saturating_sub(4));
    let px = (area.width.saturating_sub(popup_w)) / 2;
    let py = (area.height.saturating_sub(popup_h)) / 2;
    let inner_x = area.x + px + 1;
    let inner_y = area.y + py + 1;

    let model_row = if show_base_url { 3 } else { 2 };
    let hints_row = model_row + 2;
    let dropdown_start = hints_row + 2;

    for row in 0..field_count {
        app.hits_register(
            Rect::new(inner_x, inner_y + row, popup_w.saturating_sub(2), 1),
            MouseAction::SettingsModelField(row as usize),
        );
    }
    app.hits_register(
        Rect::new(inner_x.saturating_add(2), inner_y + hints_row, 8, 1),
        MouseAction::SettingsModelSave,
    );
    app.hits_register(
        Rect::new(inner_x.saturating_add(11), inner_y + hints_row, 8, 1),
        MouseAction::SettingsModelCancel,
    );
    if form.model_dropdown_open && !form.available_models.is_empty() {
        for i in 0..form.available_models.len() {
            app.hits_register(
                Rect::new(
                    inner_x,
                    inner_y + dropdown_start + i as u16,
                    popup_w.saturating_sub(2),
                    1,
                ),
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
    let popup_h = (item_count as u16 + 2).min(area.height.saturating_sub(4));
    let max_name = state
        .model_definitions
        .iter()
        .map(|d| d.name.len())
        .max()
        .unwrap_or(20);
    let popup_w = (max_name as u16 + 6).min(area.width.saturating_sub(4));
    let px = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let py = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let inner_x = px + 1;
    let inner_y = py + 1;
    let inner_h = popup_h.saturating_sub(2);

    for i in 0..item_count {
        app.hits_register(
            Rect::new(inner_x, inner_y + i as u16, popup_w.saturating_sub(2), 1),
            MouseAction::SettingsDropdownRow(i),
        );
    }
    app.hits_register(area, MouseAction::SettingsDropdownDismiss);
}

pub fn register_chain_form_hits(app: &App, hit: &ChainFormHitMap) {
    fn reg(app: &App, rect: &HitRect, action: MouseAction) {
        app.hits_register(
            Rect::new(rect.x, rect.y, rect.w, rect.h),
            action,
        );
    }

    reg(app, &hit.save_button, MouseAction::ChainSave);
    reg(app, &hit.cancel_button, MouseAction::ChainCancel);
    reg(app, &hit.auto_layout_button, MouseAction::ChainAutoLayout);
    for (kind, rect) in &hit.palette_buttons {
        reg(app, rect, MouseAction::ChainPalette(*kind));
    }
    for (target, rect) in &hit.header_fields {
        reg(app, rect, MouseAction::ChainEdit(target.clone()));
    }
    for (target, rect) in &hit.property_fields {
        reg(app, rect, MouseAction::ChainEdit(target.clone()));
    }
    reg(app, &hit.kind_cycle_button, MouseAction::ChainCycleKind);
    reg(app, &hit.delete_element_button, MouseAction::ChainDeleteElement);
    reg(app, &hit.cycle_condition_button, MouseAction::ChainCycleCondition);
    reg(app, &hit.delete_connection_button, MouseAction::ChainDeleteConnection);
    reg(app, &hit.pick_op_button, MouseAction::ChainPickOp);
    reg(app, &hit.canvas, MouseAction::ChainCanvas);
}

