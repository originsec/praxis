use crate::app::{
    AddRemoteNodeForm, Popup, PopupKind, ScheduleKind, TriggerForm, TriggerFormSection, TriggerKind,
};
use crate::ui::chrome;
use crate::ui::common::centered_rect_fixed;
use crate::ui::theme::{
    ACCENT, BG_ELEMENT, BG_MENU, BG_SELECTED, BORDER_SUBTLE, DIM, ERROR, MUTED, OK, STATUS_FAIL,
    STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};
// BG_MENU retained for list select items; confirm body uses BG_ELEMENT.
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

//
// Generic overlays (lists, confirms, cross-cutting dialogs). Domain
// edit forms live with their feature (operations/, intercept/,
// settings/) and share widgets via `form_modal`.
//

pub fn render(f: &mut Frame, popup: &Popup) {
    match popup.kind {
        PopupKind::ModelSelect => render_list_select(f, popup, "Select Model"),
        PopupKind::CommandPalette => render_command_palette(f, popup),
        PopupKind::SaveSession => render_save_session(f, popup),
    }
}

//
// Confirm/notice popup size: one row per message line plus chrome, as wide
// as the longest line. Shared with the mouse hit registration.
//
pub fn confirm_geometry(message: &str, terminal: Rect) -> (u16, u16) {
    let longest = message
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);
    let line_count = message.lines().count().max(1) as u16;
    let width = (longest as u16 + 8)
        .min(terminal.width.saturating_sub(4))
        .max(36);
    (width, line_count + 6)
}

pub fn render_confirm(f: &mut Frame, confirm: &crate::app::ConfirmAction) {
    use crate::app::ConfirmKind;
    let is_info = confirm.action.is_dismiss_only();
    let (width, height) = confirm_geometry(&confirm.message, f.area());
    let area = centered_rect_fixed(width, height, f.area());

    let title = match confirm.action {
        ConfirmKind::Info => "Error",
        ConfirmKind::Notice => "Notice",
        _ => "Confirm",
    };
    let body = chrome::modal_panel(f, area, title, "");

    let line_count = height - 6;
    let body_chunks = Layout::vertical([
        Constraint::Length(line_count),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(body);

    let lead = match confirm.action {
        ConfirmKind::Info => chrome::dot(ERROR),
        ConfirmKind::Notice => chrome::dot(OK),
        _ => chrome::dot(STATUS_RUNNING),
    };
    let mut message_lines: Vec<Line> = Vec::new();
    for (i, text) in confirm.message.lines().enumerate() {
        let prefix = if i == 0 { lead.clone() } else { Span::raw(" ") };
        message_lines.push(Line::from(vec![
            prefix,
            Span::raw(" "),
            Span::styled(text.to_string(), Style::default().fg(TEXT_BRIGHT)),
        ]));
    }
    f.render_widget(
        Paragraph::new(message_lines).style(Style::default().bg(BG_ELEMENT)),
        body_chunks[0],
    );

    let hint_line = if is_info {
        Line::from(Span::styled(
            "press any key",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        ))
    } else {
        Line::from(vec![
            chrome::pill("y", ERROR),
            Span::styled(" yes", Style::default().fg(MUTED)),
            Span::raw("    "),
            chrome::pill("n", ACCENT),
            Span::styled(" no", Style::default().fg(MUTED)),
        ])
    };
    f.render_widget(
        Paragraph::new(hint_line).style(Style::default().bg(BG_ELEMENT)),
        body_chunks[2],
    );
}

pub fn render_add_remote_node_form(f: &mut Frame, area: Rect, form: &AddRemoteNodeForm) {
    let kinds = common::REMOTE_NODE_KINDS;
    let kind_name = kinds
        .get(form.kind_idx)
        .map(|k| k.display_name)
        .unwrap_or("?");

    let height = (AddRemoteNodeForm::FIELD_COUNT as u16) + 6;
    let width = 60u16.min(area.width.saturating_sub(4));
    let popup_area = centered_rect_fixed(width, height, area);

    let body = chrome::modal_panel(f, popup_area, "Add Remote Node", "esc");

    let body_chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(body);

    let mut lines: Vec<Line> = Vec::new();
    let edit_style = Style::default()
        .fg(TEXT_BRIGHT)
        .add_modifier(Modifier::BOLD);
    let cursor_style = Style::default().fg(ACCENT);

    let kind_sel = form.focused_field == AddRemoteNodeForm::KIND_FIELD;
    let prefix = if kind_sel { "\u{276f} " } else { "  " };
    let label_style = if kind_sel {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let value_style = if kind_sel {
        edit_style
    } else {
        Style::default().fg(TEXT_BRIGHT)
    };
    lines.push(Line::from(vec![
        Span::styled(prefix, label_style),
        Span::styled(
            format!("{:<14}", AddRemoteNodeForm::field_label(0)),
            label_style,
        ),
        Span::styled(format!("\u{25c0} {} \u{25b6}", kind_name), value_style),
    ]));

    let build_field = |idx: usize, text: &str, cursor_pos: usize| -> Line {
        let selected = form.focused_field == idx;
        let editing = selected && form.editing_text;
        let prefix = if selected { "\u{276f} " } else { "  " };
        let label_color = if selected { ACCENT } else { MUTED };
        let label = format!("{:<14}", AddRemoteNodeForm::field_label(idx));

        if editing {
            let cursor_byte = text
                .char_indices()
                .nth(cursor_pos)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            let (before, after) = text.split_at(cursor_byte.min(text.len()));
            Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default()
                        .fg(label_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    label,
                    Style::default()
                        .fg(label_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(before.to_string(), edit_style),
                Span::styled("\u{2588}", cursor_style),
                Span::styled(after.to_string(), edit_style),
            ])
        } else {
            let display = if text.is_empty() && idx == AddRemoteNodeForm::URL_FIELD {
                "ws://host:port".to_string()
            } else if text.is_empty() && idx == AddRemoteNodeForm::TOKEN_FIELD {
                "(none)".to_string()
            } else if idx == AddRemoteNodeForm::TOKEN_FIELD {
                "\u{2022}".repeat(text.chars().count())
            } else {
                text.to_string()
            };
            let style = if text.is_empty() {
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC)
            } else {
                Style::default().fg(TEXT_BRIGHT)
            };
            Line::from(vec![
                Span::styled(prefix, Style::default().fg(label_color)),
                Span::styled(label, Style::default().fg(label_color)),
                Span::styled(display, style),
            ])
        }
    };

    lines.push(build_field(
        AddRemoteNodeForm::URL_FIELD,
        &form.url,
        form.url_cursor,
    ));
    lines.push(build_field(
        AddRemoteNodeForm::TOKEN_FIELD,
        &form.token,
        form.token_cursor,
    ));

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(BG_MENU)),
        body_chunks[0],
    );

    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let hints = Line::from(vec![
        Span::styled("^s", key),
        Span::styled(" save", label),
        Span::raw("    "),
        Span::styled("\u{2190}\u{2192}", key),
        Span::styled(" pick type", label),
        Span::raw("    "),
        Span::styled("esc", key),
        Span::styled(" cancel", label),
    ]);
    f.render_widget(
        Paragraph::new(hints).style(Style::default().bg(BG_MENU)),
        body_chunks[2],
    );
}

fn render_list_select(f: &mut Frame, popup: &Popup, title: &str) {
    let filtered = popup.filtered_items();
    let item_count = filtered.len().min(12) as u16;
    let height = item_count + 5;

    let max_label_width = filtered
        .iter()
        .map(|(_, item)| item.label.len() + item.description.len() + 4)
        .max()
        .unwrap_or(30);
    let width = (max_label_width as u16 + 6).min(f.area().width - 4).max(36);

    let area = centered_rect_fixed(width, height, f.area());
    let body = chrome::modal_panel(f, area, title, "esc");
    render_list(f, body, popup, &filtered);
}

fn render_command_palette(f: &mut Frame, popup: &Popup) {
    let filtered = popup.filtered_items();
    let item_count = filtered.len().min(8) as u16;
    let height = item_count + 5;

    let bottom_offset = 5u16;
    let y = f.area().height.saturating_sub(bottom_offset + height);
    let width = (f.area().width / 2).max(36).min(f.area().width - 4);
    let x = 2;

    let area = Rect::new(x, y, width, height);
    let body = chrome::modal_panel(f, area, "Commands", "esc");
    render_list(f, body, popup, &filtered);
}

fn render_save_session(f: &mut Frame, popup: &Popup) {
    let width = 64u16.min(f.area().width - 4).max(40);
    let height = 6u16;
    let area = centered_rect_fixed(width, height, f.area());

    let body = chrome::modal_panel(f, area, "Save Session", "esc");

    let block = Block::default().style(Style::default().bg(BG_ELEMENT));
    let inner = Rect {
        x: body.x + 1,
        y: body.y,
        width: body.width.saturating_sub(2),
        height: 1,
    };
    let frame = Rect {
        x: body.x,
        y: body.y,
        width: body.width,
        height: 1,
    };
    f.render_widget(block, frame);

    let input_line = Line::from(vec![
        Span::styled(
            "\u{276f} ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(&popup.filter, Style::default().fg(TEXT_BRIGHT)),
        Span::styled("\u{2588}", Style::default().fg(ACCENT)),
    ]);
    f.render_widget(
        Paragraph::new(input_line).style(Style::default().bg(BG_ELEMENT)),
        inner,
    );
}

fn render_list(
    f: &mut Frame,
    area: Rect,
    popup: &Popup,
    filtered: &[(usize, &crate::app::PopupItem)],
) {
    let items: Vec<ListItem> = filtered
        .iter()
        .map(|(_, item)| {
            let line = Line::from(vec![
                Span::raw("  "),
                Span::styled(item.label.clone(), Style::default().fg(TEXT_BRIGHT)),
                Span::styled(
                    format!("    {}", item.description),
                    Style::default().fg(DIM),
                ),
            ]);
            ListItem::new(line).style(Style::default().bg(BG_MENU))
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(BG_SELECTED)
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    );

    let mut list_state = ListState::default();
    if !filtered.is_empty() {
        list_state.select(Some(popup.selected));
    }

    f.render_stateful_widget(list, area, &mut list_state);
}

pub fn render_run_options(f: &mut Frame, area: Rect, opts: &crate::app::RunOptions) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);

    let title_text = if opts.is_chain {
        format!("Run Chain: {}", opts.op_name)
    } else {
        format!("Run Operation: {}", opts.op_name)
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title_text,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ))),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(chunks[1].width as usize),
            Style::default().fg(BORDER_SUBTLE),
        ))),
        chunks[1],
    );

    let inner = Rect {
        x: chunks[2].x + 1,
        width: chunks[2].width.saturating_sub(2),
        ..chunks[2]
    };

    let mut lines: Vec<Line> = Vec::new();

    let nodes_focused = opts.focused_section == 0;
    let nodes_label_style = if nodes_focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let all_nodes_selected = opts.nodes.iter().all(|(_, _, s)| *s);
    lines.push(Line::from(vec![
        Span::styled("Target Nodes", nodes_label_style),
        if all_nodes_selected {
            Span::styled("    (all)", Style::default().fg(DIM))
        } else {
            let count = opts.nodes.iter().filter(|(_, _, s)| *s).count();
            Span::styled(
                format!("    ({}/{})", count, opts.nodes.len()),
                Style::default().fg(DIM),
            )
        },
    ]));

    for (i, (_, name, selected)) in opts.nodes.iter().enumerate() {
        let is_cursor = nodes_focused && i == opts.cursor;
        let check = if *selected { "[\u{2713}]" } else { "[ ]" };
        let style = row_style(is_cursor, *selected);
        lines.push(Line::from(Span::styled(
            format!("  {} {}", check, name),
            style,
        )));
    }

    lines.push(Line::from(""));
    let agents_focused = opts.focused_section == 1;
    let agents_label_style = if agents_focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let all_agents_selected = opts.agents.iter().all(|(_, s)| *s);
    lines.push(Line::from(vec![
        Span::styled("Target Agents", agents_label_style),
        if all_agents_selected {
            Span::styled("    (all)", Style::default().fg(DIM))
        } else {
            let count = opts.agents.iter().filter(|(_, s)| *s).count();
            Span::styled(
                format!("    ({}/{})", count, opts.agents.len()),
                Style::default().fg(DIM),
            )
        },
    ]));

    for (i, (name, selected)) in opts.agents.iter().enumerate() {
        let is_cursor = agents_focused && i == opts.cursor;
        let check = if *selected { "[\u{2713}]" } else { "[ ]" };
        let style = row_style(is_cursor, *selected);
        lines.push(Line::from(Span::styled(
            format!("  {} {}", check, name),
            style,
        )));
    }

    if !opts.is_chain {
        lines.push(Line::from(""));
        let yolo_focused = opts.focused_section == 2;
        let yolo_label_style = if yolo_focused {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        let indicator = if opts.yolo {
            chrome::pill("ON", STATUS_RUNNING)
        } else {
            Span::styled(" off ", Style::default().fg(DIM).bg(BG_ELEMENT))
        };
        lines.push(Line::from(vec![
            Span::styled("YOLO Mode  ", yolo_label_style),
            indicator,
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);

    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let hints = Line::from(vec![
        Span::styled("\u{2191}\u{2193}", key),
        Span::styled(" navigate", label),
        Span::raw("    "),
        Span::styled("\u{21B5}", key),
        Span::styled(" toggle", label),
        Span::raw("    "),
        Span::styled("tab", key),
        Span::styled(" section", label),
        Span::raw("    "),
        Span::styled("^r", key),
        Span::styled(" run", label),
        Span::raw("    "),
        Span::styled("esc", key),
        Span::styled(" cancel", label),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[3]);
}

pub fn render_trigger_form(f: &mut Frame, area: Rect, form: &TriggerForm) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);

    let title_text = if form.editing_id.is_some() {
        "Edit Trigger"
    } else {
        "New Trigger"
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title_text,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ))),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(chunks[1].width as usize),
            Style::default().fg(BORDER_SUBTLE),
        ))),
        chunks[1],
    );

    let inner = Rect {
        x: chunks[2].x + 1,
        width: chunks[2].width.saturating_sub(2),
        ..chunks[2]
    };

    let lines = trigger_form_lines(form);
    f.render_widget(Paragraph::new(lines), inner);

    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let hints = Line::from(vec![
        Span::styled("^s", key),
        Span::styled(" save", label),
        Span::raw("    "),
        Span::styled("esc", key),
        Span::styled(" cancel", label),
        Span::raw("    "),
        Span::styled("space", key),
        Span::styled(" toggle", label),
        Span::raw("    "),
        Span::styled("\u{2190}\u{2192}", key),
        Span::styled(" cycle", label),
        Span::raw("    "),
        Span::styled("\u{2191}\u{2193}", key),
        Span::styled(" field", label),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[3]);
}

pub fn trigger_form_section_rows(form: &TriggerForm) -> Vec<(usize, TriggerFormSection, usize)> {
    let mut rows = Vec::new();
    let mut row: usize = 0;

    rows.push((row, TriggerFormSection::Chain, 0));
    row += 1;
    row += 1; // spacer

    rows.push((row, TriggerFormSection::Type, 0));
    row += 1;

    match form.kind {
        TriggerKind::Scheduled => {
            row += 1;
            rows.push((row, TriggerFormSection::ScheduleKindRow, 0));
            row += 1;
            rows.push((row, TriggerFormSection::ScheduleValueRow, 0));
            row += 1;
            rows.push((row, TriggerFormSection::Recurring, 0));
            row += 1;
        }
        TriggerKind::InterceptMatch => {
            row += 1;
            rows.push((row, TriggerFormSection::Rule, 0));
            row += 1;
        }
        TriggerKind::ReconChange | TriggerKind::AgentChange => {
            row += 1; // spacer
            row += 1; // Event kinds header
            for i in 0..form.event_kinds.len() {
                rows.push((row, TriggerFormSection::EventKinds, i));
                row += 1;
            }
            row += 1; // Event agents header
            for i in 0..form.event_agents.len() {
                rows.push((row, TriggerFormSection::EventAgents, i));
                row += 1;
            }
            if form.kind == TriggerKind::ReconChange {
                rows.push((row, TriggerFormSection::ConfigTypes, 0));
                row += 1;
                rows.push((row, TriggerFormSection::PathGlob, 0));
                row += 1;
            }
        }
        TriggerKind::SessionActivity => {
            row += 1; // spacer
            row += 1; // Event agents header
            for i in 0..form.event_agents.len() {
                rows.push((row, TriggerFormSection::EventAgents, i));
                row += 1;
            }
            rows.push((row, TriggerFormSection::NewSessionsOnly, 0));
            row += 1;
            rows.push((row, TriggerFormSection::QuietPeriod, 0));
            row += 1;
        }
        TriggerKind::Webhook => {
            row += 2; // spacer + info line
        }
        TriggerKind::NewNode => {}
    }

    row += 1; // spacer before Target header
    row += 1; // "Target" header
    row += 1; // Nodes header
    for i in 0..form.nodes.len() {
        rows.push((row, TriggerFormSection::Nodes, i));
        row += 1;
    }
    row += 1; // spacer

    rows.push((row, TriggerFormSection::OsFilter, 0));
    row += 1;
    row += 1; // spacer

    row += 1; // Agents header
    for i in 0..form.agents.len() {
        rows.push((row, TriggerFormSection::Agents, i));
        row += 1;
    }

    if form.kind.is_node_event() {
        row += 1; // spacer
        rows.push((row, TriggerFormSection::IncludeTriggering, 0));
        row += 1;
        rows.push((row, TriggerFormSection::TriggeringNodeOnly, 0));
    }

    rows
}

fn trigger_form_lines(form: &TriggerForm) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    let section_focus = |section: TriggerFormSection| -> bool { form.focused_section == section };

    lines.push(chain_line(form, section_focus(TriggerFormSection::Chain)));
    lines.push(Line::from(""));

    lines.push(type_line(form, section_focus(TriggerFormSection::Type)));

    let checklist = |lines: &mut Vec<Line<'static>>,
                     label: &str,
                     section: TriggerFormSection,
                     items: &[(String, bool)]| {
        let selected = items.iter().filter(|(_, s)| *s).count();
        let suffix = if selected == 0 {
            "any".to_string()
        } else {
            format!("{}/{}", selected, items.len())
        };
        lines.push(section_header(label, section_focus(section), &suffix));
        for (i, (name, sel)) in items.iter().enumerate() {
            let is_cursor = section_focus(section) && i == form.cursor;
            let check = if *sel { "[\u{2713}]" } else { "[ ]" };
            lines.push(Line::from(Span::styled(
                format!("  {} {}", check, name),
                row_style(is_cursor, *sel),
            )));
        }
    };

    match form.kind {
        TriggerKind::Scheduled => {
            lines.push(Line::from(""));
            lines.push(schedule_kind_line(
                form,
                section_focus(TriggerFormSection::ScheduleKindRow),
            ));
            lines.push(schedule_value_line(
                form,
                section_focus(TriggerFormSection::ScheduleValueRow),
            ));
            lines.push(recurring_line(
                form,
                section_focus(TriggerFormSection::Recurring),
            ));
        }
        TriggerKind::InterceptMatch => {
            lines.push(Line::from(""));
            lines.push(rule_line(form, section_focus(TriggerFormSection::Rule)));
        }
        TriggerKind::ReconChange | TriggerKind::AgentChange => {
            lines.push(Line::from(""));
            checklist(
                &mut lines,
                "Changes",
                TriggerFormSection::EventKinds,
                &form.event_kinds,
            );
            checklist(
                &mut lines,
                "Event agents",
                TriggerFormSection::EventAgents,
                &form.event_agents,
            );
            if form.kind == TriggerKind::ReconChange {
                lines.push(text_line(
                    "Config types",
                    &form.config_types,
                    "(any, comma-separated)",
                    section_focus(TriggerFormSection::ConfigTypes),
                ));
                lines.push(text_line(
                    "Path glob",
                    &form.path_glob,
                    "(any)",
                    section_focus(TriggerFormSection::PathGlob),
                ));
            }
        }
        TriggerKind::SessionActivity => {
            lines.push(Line::from(""));
            checklist(
                &mut lines,
                "Event agents",
                TriggerFormSection::EventAgents,
                &form.event_agents,
            );
            lines.push(toggle_line(
                "New sessions only",
                form.new_sessions_only,
                section_focus(TriggerFormSection::NewSessionsOnly),
            ));
            lines.push(text_line(
                "Quiet period (s)",
                &form.quiet_secs.to_string(),
                "",
                section_focus(TriggerFormSection::QuietPeriod),
            ));
        }
        TriggerKind::Webhook => {
            lines.push(Line::from(""));
            let info = match form.editing_id {
                Some(ref id) => format!("POST {}  (Bearer secret)", common::webhook_path(id)),
                None => "A bearer secret is generated on save and shown once".to_string(),
            };
            lines.push(Line::from(Span::styled(
                info,
                Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
            )));
        }
        TriggerKind::NewNode => {}
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Target",
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )));

    lines.push(section_header(
        "Nodes",
        section_focus(TriggerFormSection::Nodes),
        &format!(
            "{}/{}",
            form.nodes.iter().filter(|(_, _, s)| *s).count(),
            form.nodes.len()
        ),
    ));
    for (i, (_, label, selected)) in form.nodes.iter().enumerate() {
        let is_cursor = section_focus(TriggerFormSection::Nodes) && i == form.cursor;
        let check = if *selected { "[\u{2713}]" } else { "[ ]" };
        let style = row_style(is_cursor, *selected);
        lines.push(Line::from(Span::styled(
            format!("  {} {}", check, label),
            style,
        )));
    }

    lines.push(Line::from(""));
    lines.push(text_line(
        "OS filter",
        &form.os_filter,
        "(none)",
        section_focus(TriggerFormSection::OsFilter),
    ));
    lines.push(Line::from(""));

    lines.push(section_header(
        "Agents",
        section_focus(TriggerFormSection::Agents),
        &format!(
            "{}/{}",
            form.agents.iter().filter(|(_, s)| *s).count(),
            form.agents.len()
        ),
    ));
    for (i, (name, selected)) in form.agents.iter().enumerate() {
        let is_cursor = section_focus(TriggerFormSection::Agents) && i == form.cursor;
        let check = if *selected { "[\u{2713}]" } else { "[ ]" };
        let style = row_style(is_cursor, *selected);
        lines.push(Line::from(Span::styled(
            format!("  {} {}", check, name),
            style,
        )));
    }

    if form.kind.is_node_event() {
        lines.push(Line::from(""));
        lines.push(toggle_line(
            "Include triggering node",
            form.include_triggering_node,
            section_focus(TriggerFormSection::IncludeTriggering),
        ));
        lines.push(toggle_line(
            "Triggering node only",
            form.triggering_node_only,
            section_focus(TriggerFormSection::TriggeringNodeOnly),
        ));
    }

    lines
}

//
// Editable single-line text field with a block cursor when focused and a
// dim placeholder when empty.
//
fn text_line(label: &str, value: &str, placeholder: &str, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let value = if value.is_empty() {
        Span::styled(
            placeholder.to_string(),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(value.to_string(), Style::default().fg(TEXT_BRIGHT))
    };
    let cursor = if focused { "\u{2588}" } else { "" };
    Line::from(vec![
        Span::styled(format!("{}  ", label), label_style),
        value,
        Span::styled(cursor.to_string(), Style::default().fg(ACCENT)),
    ])
}

fn chain_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let name = form
        .chains
        .get(form.chain_cursor)
        .map(|(_, n)| n.as_str())
        .unwrap_or("(none)");
    Line::from(vec![
        Span::styled("Chain  ", label_style),
        Span::styled(
            format!("\u{25c0} {} \u{25b6}", name),
            Style::default().fg(TEXT_BRIGHT),
        ),
    ])
}

fn type_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    Line::from(vec![
        Span::styled("Type   ", label_style),
        Span::styled(
            format!("\u{25c0} {} \u{25b6}", form.kind.label()),
            Style::default().fg(TEXT_BRIGHT),
        ),
    ])
}

fn schedule_kind_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let mut spans = vec![Span::styled("Schedule  ", label_style)];
    for (variant, label) in [
        (ScheduleKind::Interval, "Interval"),
        (ScheduleKind::DailyAt, "DailyAt"),
    ] {
        if variant == form.schedule_kind {
            spans.push(chrome::pill(label, ACCENT));
        } else {
            spans.push(Span::styled(
                format!(" {} ", label),
                Style::default().fg(DIM).bg(BG_ELEMENT),
            ));
        }
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

fn schedule_value_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    match form.schedule_kind {
        ScheduleKind::Interval => Line::from(vec![
            Span::styled("Interval (min)  ", label_style),
            Span::styled(
                format!("{}", form.interval_minutes),
                Style::default().fg(TEXT_BRIGHT),
            ),
        ]),
        ScheduleKind::DailyAt => Line::from(vec![
            Span::styled("At  ", label_style),
            Span::styled(
                format!("{:02}:{:02}", form.hour, form.minute),
                Style::default().fg(TEXT_BRIGHT),
            ),
        ]),
    }
}

fn recurring_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    toggle_line("Recurring", form.recurring, focused)
}

fn rule_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let value = if form.rules.is_empty() {
        "(no intercept rules — create one first)".to_string()
    } else {
        form.rules
            .get(form.rule_cursor)
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
    };
    Line::from(vec![
        Span::styled("Rule  ", label_style),
        Span::styled(
            format!("\u{25c0} {} \u{25b6}", value),
            Style::default().fg(TEXT_BRIGHT),
        ),
    ])
}

fn section_header(label: &str, focused: bool, suffix: &str) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    Line::from(vec![
        Span::styled(label.to_string(), label_style),
        Span::raw("  "),
        Span::styled(suffix.to_string(), Style::default().fg(DIM)),
    ])
}

fn row_style(is_cursor: bool, selected: bool) -> Style {
    if is_cursor {
        Style::default()
            .fg(TEXT_BRIGHT)
            .bg(BG_SELECTED)
            .add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(TEXT_BRIGHT)
    } else {
        Style::default().fg(DIM)
    }
}

fn toggle_line(label: &str, value: bool, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let indicator = if value {
        chrome::pill("ON", STATUS_RUNNING)
    } else {
        Span::styled(" off ", Style::default().fg(DIM).bg(BG_ELEMENT))
    };
    Line::from(vec![
        Span::styled(format!("{}  ", label), label_style),
        indicator,
    ])
}

fn _silence_unused() {
    let _ = (OK, STATUS_FAIL, BG_MENU, TEXT);
}

#[cfg(test)]
mod trigger_form_tests {
    use super::*;
    use crate::app::ScheduleKind;

    fn form(kind: TriggerKind) -> TriggerForm {
        TriggerForm {
            editing_id: None,
            chains: vec![("c1".into(), "Chain".into())],
            chain_cursor: 0,
            kind,
            schedule_kind: ScheduleKind::Interval,
            hour: 0,
            minute: 0,
            interval_minutes: 60,
            recurring: true,
            rules: vec![(1, "rule".into())],
            rule_cursor: 0,
            nodes: vec![("n1".into(), "host".into(), false)],
            agents: vec![("codex".into(), false), ("claudecode".into(), true)],
            os_filter: String::new(),
            include_triggering_node: false,
            triggering_node_only: true,
            event_kinds: kind
                .event_kind_options()
                .into_iter()
                .map(|k| (k, false))
                .collect(),
            event_agents: vec![("codex".into(), true)],
            config_types: "settings".into(),
            path_glob: "**/*.json".into(),
            new_sessions_only: true,
            quiet_secs: 30,
            focused_section: TriggerFormSection::Chain,
            cursor: 0,
        }
    }

    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    //
    // Mouse hit rows must land on the rendered line of the same field.
    //
    #[test]
    fn hit_rows_align_with_rendered_lines() {
        for kind in TriggerKind::ALL {
            let f = form(kind);
            let lines = trigger_form_lines(&f);
            for (row, section, cursor) in trigger_form_section_rows(&f) {
                let text = line_text(&lines[row]);
                let expect = match section {
                    TriggerFormSection::Chain => "Chain".to_string(),
                    TriggerFormSection::Type => "Type".to_string(),
                    TriggerFormSection::Nodes => f.nodes[cursor].1.clone(),
                    TriggerFormSection::Agents => f.agents[cursor].0.clone(),
                    TriggerFormSection::EventKinds => f.event_kinds[cursor].0.clone(),
                    TriggerFormSection::EventAgents => f.event_agents[cursor].0.clone(),
                    TriggerFormSection::OsFilter => "OS filter".to_string(),
                    TriggerFormSection::ConfigTypes => "Config types".to_string(),
                    TriggerFormSection::PathGlob => "Path glob".to_string(),
                    TriggerFormSection::NewSessionsOnly => "New sessions only".to_string(),
                    TriggerFormSection::QuietPeriod => "Quiet period".to_string(),
                    TriggerFormSection::IncludeTriggering => "Include triggering".to_string(),
                    TriggerFormSection::TriggeringNodeOnly => "Triggering node only".to_string(),
                    TriggerFormSection::ScheduleKindRow => "Schedule".to_string(),
                    TriggerFormSection::ScheduleValueRow => "Interval".to_string(),
                    TriggerFormSection::Recurring => "Recurring".to_string(),
                    TriggerFormSection::Rule => "Rule".to_string(),
                };
                assert!(
                    text.contains(&expect),
                    "{:?}: row {} is '{}' but expected '{}'",
                    kind.label(),
                    row,
                    text,
                    expect
                );
            }
        }
    }

    #[test]
    fn builds_configs_for_new_types() {
        let mut f = form(TriggerKind::ReconChange);
        f.event_kinds[0].1 = true;
        match f.build_config().unwrap() {
            common::TriggerConfig::ReconChange {
                agent_short_names,
                kinds,
                config_types,
                path_glob,
            } => {
                assert_eq!(agent_short_names, vec!["codex"]);
                assert_eq!(kinds, vec![common::ReconChangeKind::McpServerAdded]);
                assert_eq!(config_types, vec!["settings"]);
                assert_eq!(path_glob.as_deref(), Some("**/*.json"));
            }
            other => panic!("unexpected {:?}", other),
        }

        f.path_glob = "[".into();
        assert!(f.build_config().is_err());

        let f = form(TriggerKind::SessionActivity);
        assert!(matches!(
            f.build_config().unwrap(),
            common::TriggerConfig::SessionActivity {
                new_sessions_only: true,
                quiet_period_secs: 30,
                ..
            }
        ));
        assert!(matches!(
            form(TriggerKind::Webhook).build_config().unwrap(),
            common::TriggerConfig::Webhook
        ));
    }
}
