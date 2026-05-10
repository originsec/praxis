use crate::app::{
    AddRemoteNodeForm, Popup, PopupKind, ScheduleKind, TriggerForm, TriggerFormSection,
    TriggerKind,
};
use crate::ui::chrome;
use crate::ui::common::centered_rect_fixed;
use crate::ui::theme::{
    ACCENT, BG_ELEMENT, BG_MENU, BG_SELECTED, BORDER_SUBTLE, DIM, ERROR, MUTED, OK, STATUS_FAIL,
    STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

//
// Shared dialog chrome lives in `chrome::modal_panel`. Use that for any
// new modal so they share the same title/divider/background treatment.
//

pub fn render(f: &mut Frame, popup: &Popup) {
    match popup.kind {
        PopupKind::ModelSelect => render_list_select(f, popup, "Select Model"),
        PopupKind::CommandPalette => render_command_palette(f, popup),
        PopupKind::SaveSession => render_save_session(f, popup),
        PopupKind::NewOp => {}   // rendered separately via new_op_form
        PopupKind::Confirm => {} // rendered separately via confirm
    }
}

pub fn render_confirm(f: &mut Frame, confirm: &crate::app::ConfirmAction) {
    let is_info = matches!(confirm.action, crate::app::ConfirmKind::Info);
    let width = (confirm.message.len() as u16 + 8)
        .min(f.area().width - 4)
        .max(36);
    let height = 7;
    let area = centered_rect_fixed(width, height, f.area());

    let title = if is_info { "Error" } else { "Confirm" };
    let body = chrome::modal_panel(f, area, title, "");

    let body_chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(body);

    let lead = if is_info {
        chrome::dot(ERROR)
    } else {
        chrome::dot(STATUS_RUNNING)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            lead,
            Span::raw(" "),
            Span::styled(
                confirm.message.clone(),
                Style::default().fg(TEXT_BRIGHT),
            ),
        ]))
        .style(Style::default().bg(BG_MENU)),
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
        Paragraph::new(hint_line).style(Style::default().bg(BG_MENU)),
        body_chunks[2],
    );
}

pub fn render_new_op_form(f: &mut Frame, area: Rect, form: &crate::app::NewOpForm) {
    use crate::app::NewOpForm;

    let chunks = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Length(1), // divider
        Constraint::Min(1),    // fields
        Constraint::Length(1), // hints
    ])
    .split(area);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "New Operation",
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
    let modes = ["one-shot", "agent"];

    for i in 0..NewOpForm::field_count() {
        if i == 1 || i == 7 {
            lines.push(Line::from(""));
        }
        let is_focused = i == form.focused_field;
        let label = NewOpForm::field_label(i);
        let label_style = if is_focused {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        let value_style = if is_focused {
            Style::default().fg(TEXT_BRIGHT)
        } else {
            Style::default().fg(TEXT)
        };
        let cursor = if is_focused { "\u{2588}" } else { "" };

        let value = match i {
            0 => {
                let mut spans = vec![Span::styled(format!("{:<14}", label), label_style)];
                for (mi, m) in modes.iter().enumerate() {
                    if mi == form.mode {
                        spans.push(chrome::pill(m, ACCENT));
                    } else {
                        spans.push(Span::styled(
                            format!(" {} ", m),
                            Style::default().fg(DIM).bg(BG_ELEMENT),
                        ));
                    }
                    spans.push(Span::raw(" "));
                }
                lines.push(Line::from(spans));
                continue;
            }
            1 => form.name.clone(),
            2 => form.short_name.clone(),
            3 => form.category.clone(),
            4 => form.description.clone(),
            5 => {
                if form.mode == 0 {
                    continue;
                }
                form.iterations.clone()
            }
            6 => form.timeout.clone(),
            7 => {
                let indicator = if form.yolo {
                    chrome::pill("ON", STATUS_RUNNING)
                } else {
                    Span::styled(
                        " off ",
                        Style::default().fg(DIM).bg(BG_ELEMENT),
                    )
                };
                let spans = vec![
                    Span::styled(format!("{:<14}", label), label_style),
                    indicator,
                ];
                lines.push(Line::from(spans));
                continue;
            }
            8 => form.prompt.clone(),
            _ => String::new(),
        };

        if i == 8 {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("{}:", label),
                label_style,
            )));
            if value.is_empty() && is_focused {
                lines.push(Line::from(Span::styled(
                    cursor,
                    Style::default().fg(ACCENT),
                )));
            } else {
                let split: Vec<&str> = value.split('\n').collect();
                let last_idx = split.len() - 1;
                for (li, line) in split.iter().enumerate() {
                    if li == last_idx && is_focused {
                        lines.push(Line::from(vec![
                            Span::styled(line.to_string(), value_style),
                            Span::styled(cursor, Style::default().fg(ACCENT)),
                        ]));
                    } else {
                        lines.push(Line::from(Span::styled(line.to_string(), value_style)));
                    }
                }
            }
            continue;
        }

        lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", label), label_style),
            Span::styled(value, value_style),
            Span::styled(cursor, Style::default().fg(ACCENT)),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);

    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let mut hint_spans = vec![
        Span::styled("\u{2191}\u{2193}", key),
        Span::styled(" fields", label),
        Span::raw("    "),
        Span::styled("space/\u{2190}\u{2192}", key),
        Span::styled(" toggle", label),
        Span::raw("    "),
        Span::styled("^s", key),
        Span::styled(" save", label),
        Span::raw("    "),
        Span::styled("esc", key),
        Span::styled(" cancel", label),
    ];
    if form.focused_field == 8 {
        hint_spans.push(Span::raw("    "));
        hint_spans.push(Span::styled("shift+\u{21B5}", key));
        hint_spans.push(Span::styled(" newline", label));
    }
    f.render_widget(Paragraph::new(Line::from(hint_spans)), chunks[3]);
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
    let edit_style = Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD);
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
        Span::styled(
            format!("\u{25c0} {} \u{25b6}", kind_name),
            value_style,
        ),
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
                Span::styled(prefix, Style::default().fg(label_color).add_modifier(Modifier::BOLD)),
                Span::styled(label, Style::default().fg(label_color).add_modifier(Modifier::BOLD)),
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
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
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
                Span::styled(
                    item.label.clone(),
                    Style::default().fg(TEXT_BRIGHT),
                ),
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

pub fn trigger_form_section_rows(
    form: &TriggerForm,
) -> Vec<(usize, TriggerFormSection, usize)> {
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

    if matches!(
        form.kind,
        TriggerKind::InterceptMatch | TriggerKind::NewNode
    ) {
        row += 1; // spacer
        rows.push((row, TriggerFormSection::IncludeTriggering, 0));
    }

    rows
}

fn trigger_form_lines(form: &TriggerForm) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    let section_focus =
        |section: TriggerFormSection| -> bool { form.focused_section == section };

    lines.push(chain_line(form, section_focus(TriggerFormSection::Chain)));
    lines.push(Line::from(""));

    lines.push(type_line(form, section_focus(TriggerFormSection::Type)));

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

    {
        let focus = section_focus(TriggerFormSection::OsFilter);
        let label_style = if focus {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        let value = if form.os_filter.is_empty() {
            Span::styled(
                "(none)",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )
        } else {
            Span::styled(
                form.os_filter.clone(),
                Style::default().fg(TEXT_BRIGHT),
            )
        };
        let cursor = if focus { "\u{2588}" } else { "" };
        lines.push(Line::from(vec![
            Span::styled("OS filter  ", label_style),
            value,
            Span::styled(cursor.to_string(), Style::default().fg(ACCENT)),
        ]));
    }
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

    if matches!(
        form.kind,
        TriggerKind::InterceptMatch | TriggerKind::NewNode
    ) {
        lines.push(Line::from(""));
        lines.push(toggle_line(
            "Include triggering node",
            form.include_triggering_node,
            section_focus(TriggerFormSection::IncludeTriggering),
        ));
    }

    lines
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
    let variants = [
        (TriggerKind::Scheduled, "Scheduled"),
        (TriggerKind::InterceptMatch, "InterceptMatch"),
        (TriggerKind::NewNode, "NewNode"),
    ];
    let mut spans = vec![Span::styled("Type   ", label_style)];
    for (kind, label) in variants {
        if kind == form.kind {
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

#[allow(dead_code)]
fn _silence_unused() {
    let _ = (OK, STATUS_FAIL, BG_MENU, TEXT);
}
