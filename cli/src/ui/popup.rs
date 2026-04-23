use crate::app::{
    Popup, PopupKind, ScheduleKind, TriggerForm, TriggerFormSection, TriggerKind,
};
use crate::ui::common::centered_rect_fixed;
use crate::ui::theme::{ACCENT, DIM, MUTED, POPUP_BG, POPUP_HIGHLIGHT_BG, STATUS_RUNNING, TEXT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

pub fn render(f: &mut Frame, popup: &Popup) {
    match popup.kind {
        PopupKind::ModelSelect => render_list_select(f, popup, " Select Model "),
        PopupKind::CommandPalette => render_command_palette(f, popup),
        PopupKind::SaveSession => render_save_session(f, popup),
        PopupKind::NewOp => {}   // rendered separately via new_op_form
        PopupKind::Confirm => {} // rendered separately via confirm
    }
}

pub fn render_intercept_method_picker(f: &mut Frame, picker: &crate::app::InterceptMethodPicker) {
    let options = picker.options();
    let inner_height = options.len() as u16 + 3; // title + options + hint
    let height = inner_height + 2;
    let width = 54u16.min(f.area().width.saturating_sub(4)).max(40);
    let area = centered_rect_fixed(width, height, f.area());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " Select Interception Method ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(POPUP_BG));

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);
    let inner = block.inner(area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!(" Node: {}", picker.machine_name),
        Style::default().fg(MUTED),
    )));
    lines.push(Line::from(""));
    for (i, opt) in options.iter().enumerate() {
        let is_sel = i == picker.selected;
        let bg = if is_sel { POPUP_HIGHLIGHT_BG } else { POPUP_BG };
        let label_style = if !opt.enabled {
            Style::default().fg(DIM).bg(bg)
        } else if is_sel {
            Style::default().fg(ACCENT).bg(bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT).bg(bg)
        };
        let desc_style = Style::default().fg(DIM).bg(bg);
        let prefix = if is_sel { " \u{25b6} " } else { "   " };
        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), label_style),
            Span::styled(format!("{:12}", opt.label), label_style),
            Span::styled(format!("  {}", opt.description), desc_style),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" \u{2191}\u{2193}", Style::default().fg(ACCENT)),
        Span::styled(" select  ", Style::default().fg(MUTED)),
        Span::styled("\u{23ce}", Style::default().fg(ACCENT)),
        Span::styled(" enable  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

pub fn render_confirm(f: &mut Frame, confirm: &crate::app::ConfirmAction) {
    let is_info = matches!(confirm.action, crate::app::ConfirmKind::Info);
    let width = (confirm.message.len() as u16 + 6)
        .min(f.area().width - 4)
        .max(30);
    let height = 5;
    let area = centered_rect_fixed(width, height, f.area());

    let (title, border_color) = if is_info {
        (" Error ", Color::Rgb(180, 60, 60))
    } else {
        (" Confirm ", Color::Rgb(180, 60, 60))
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title)
        .title_style(Style::default().fg(border_color))
        .style(Style::default().bg(POPUP_BG));

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let hint_line = if is_info {
        Line::from(Span::styled(" press any key", Style::default().fg(MUTED)))
    } else {
        Line::from(vec![
            Span::styled(" y", Style::default().fg(Color::Rgb(180, 60, 60))),
            Span::styled(" yes  ", Style::default().fg(MUTED)),
            Span::styled("n", Style::default().fg(ACCENT)),
            Span::styled(" no", Style::default().fg(MUTED)),
        ])
    };

    let lines = vec![
        Line::from(Span::styled(
            format!(" {}", confirm.message),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        hint_line,
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

pub fn render_new_op_form(f: &mut Frame, area: Rect, form: &crate::app::NewOpForm) {
    use crate::app::NewOpForm;

    let chunks = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(2), // title
        ratatui::layout::Constraint::Min(1),    // fields
        ratatui::layout::Constraint::Length(1), // hints
    ])
    .split(area);

    //
    // Title.
    //
    let title = Paragraph::new(Line::from(Span::styled(
        " New Operation",
        Style::default()
            .fg(TEXT)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )));
    f.render_widget(title, chunks[0]);

    //
    // Fields with proper spacing and labels.
    //
    let inner = ratatui::layout::Rect {
        x: chunks[1].x + 2,
        width: chunks[1].width.saturating_sub(4),
        ..chunks[1]
    };

    let mut lines: Vec<Line> = Vec::new();
    let modes = ["one-shot", "agent"];

    for i in 0..NewOpForm::field_count() {
        //
        // Insert gaps: after Mode (0), before YOLO (7).
        //
        if i == 1 || i == 7 {
            lines.push(Line::from(""));
        }
        let is_focused = i == form.focused_field;
        let label = NewOpForm::field_label(i);
        let label_style = if is_focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(MUTED)
        };
        let value_style = if is_focused {
            Style::default().fg(TEXT)
        } else {
            Style::default().fg(DIM)
        };
        let cursor = if is_focused { "\u{258f}" } else { "" };

        let value = match i {
            0 => {
                //
                // Mode toggle display.
                //
                let mut spans = vec![Span::styled(format!("{}: ", label), label_style)];
                for (mi, m) in modes.iter().enumerate() {
                    if mi == form.mode {
                        spans.push(Span::styled(
                            format!(" {} ", m),
                            Style::default().fg(Color::Black).bg(ACCENT),
                        ));
                    } else {
                        spans.push(Span::styled(format!(" {} ", m), Style::default().fg(DIM)));
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
                //
                // Only show iterations when mode is agent.
                //
                if form.mode == 0 {
                    continue;
                }
                form.iterations.clone()
            }
            6 => form.timeout.clone(),
            7 => {
                //
                // YOLO toggle display.
                //
                let indicator = if form.yolo {
                    Span::styled(
                        " \u{25cf} true ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(STATUS_RUNNING),
                    )
                } else {
                    Span::styled(" \u{25cb} false ", Style::default().fg(DIM))
                };
                let spans = vec![Span::styled(format!("{}: ", label), label_style), indicator];
                lines.push(Line::from(spans));
                continue;
            }
            8 => form.prompt.clone(),
            _ => String::new(),
        };

        //
        // Prompt gets a larger display.
        //
        if i == 8 {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(format!("{}:", label), label_style)));
            if value.is_empty() && is_focused {
                lines.push(Line::from(Span::styled(
                    cursor,
                    Style::default().fg(ACCENT),
                )));
            } else {
                //
                // Split on \n. Render each line, with cursor on the last.
                //
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
            Span::styled(format!("{}: ", label), label_style),
            Span::styled(value, value_style),
            Span::styled(cursor, Style::default().fg(ACCENT)),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);

    //
    // Bottom hints.
    //
    let mut hint_spans = vec![
        Span::raw(" "),
        Span::styled("\u{2191}\u{2193}", Style::default().fg(ACCENT)),
        Span::styled(" fields  ", Style::default().fg(MUTED)),
        Span::styled("space/\u{2190}\u{2192}", Style::default().fg(ACCENT)),
        Span::styled(" toggle  ", Style::default().fg(MUTED)),
        Span::styled("^s", Style::default().fg(ACCENT)),
        Span::styled(" save  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ];
    if form.focused_field == 8 {
        hint_spans.push(Span::styled("  shift+enter", Style::default().fg(ACCENT)));
        hint_spans.push(Span::styled(" newline", Style::default().fg(MUTED)));
    }
    let hints = Line::from(hint_spans);
    f.render_widget(Paragraph::new(hints), chunks[2]);
}

//
// Model select: centered, compact popup sized to content.
//

fn render_list_select(f: &mut Frame, popup: &Popup, title: &str) {
    let filtered = popup.filtered_items();
    let item_count = filtered.len().min(12) as u16;
    let height = item_count + 2; // +2 for borders

    let max_label_width = filtered
        .iter()
        .map(|(_, item)| item.label.len() + item.description.len() + 4)
        .max()
        .unwrap_or(30);
    let width = (max_label_width as u16 + 4).min(f.area().width - 4).max(30);

    let area = centered_rect_fixed(width, height, f.area());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(title)
        .title_style(Style::default().fg(MUTED))
        .style(Style::default().bg(POPUP_BG));

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);

    let inner = block.inner(area);
    render_list(f, inner, popup, &filtered);
}

//
// Command palette: anchored above the input area at the bottom of the screen.
//

fn render_command_palette(f: &mut Frame, popup: &Popup) {
    let filtered = popup.filtered_items();
    let item_count = filtered.len().min(8) as u16;
    let height = item_count + 2;

    //
    // Position above the bottom input area (status bar + spacer + tokens +
    // input + model = ~7 lines from bottom).
    //
    let bottom_offset = 5u16;
    let y = f.area().height.saturating_sub(bottom_offset + height);
    let width = (f.area().width / 2).max(30).min(f.area().width - 4);
    let x = 1;

    let area = Rect::new(x, y, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(" Commands ")
        .title_style(Style::default().fg(MUTED))
        .style(Style::default().bg(POPUP_BG));

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);

    let inner = block.inner(area);
    render_list(f, inner, popup, &filtered);
}

//
// Save session: centered input box for the file path.
//

fn render_save_session(f: &mut Frame, popup: &Popup) {
    let width = 60u16.min(f.area().width - 4).max(30);
    let height = 3u16;
    let area = centered_rect_fixed(width, height, f.area());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(" Save Session ")
        .title_style(Style::default().fg(MUTED))
        .style(Style::default().bg(POPUP_BG));

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let input_line = Line::from(vec![
        Span::styled(&popup.filter, Style::default().fg(TEXT)),
        Span::styled("_", Style::default().fg(ACCENT)),
    ]);
    let paragraph = Paragraph::new(input_line);
    f.render_widget(paragraph, inner);
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
                Span::styled(format!(" {}", item.label), Style::default().fg(TEXT)),
                Span::styled(format!("  {}", item.description), Style::default().fg(DIM)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(POPUP_HIGHLIGHT_BG)
            .fg(ACCENT)
            .add_modifier(Modifier::BOLD),
    );

    let mut list_state = ListState::default();
    if !filtered.is_empty() {
        list_state.select(Some(popup.selected));
    }

    f.render_stateful_widget(list, area, &mut list_state);
}

pub fn render_run_options(f: &mut Frame, area: Rect, opts: &crate::app::RunOptions) {
    let chunks = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(2),
        ratatui::layout::Constraint::Min(1),
        ratatui::layout::Constraint::Length(1),
    ])
    .split(area);

    //
    // Title.
    //
    let title_text = if opts.is_chain {
        format!(" Run Chain: {}", opts.op_name)
    } else {
        format!(" Run Operation: {}", opts.op_name)
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title_text,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ))),
        chunks[0],
    );

    let inner = ratatui::layout::Rect {
        x: chunks[1].x + 2,
        width: chunks[1].width.saturating_sub(4),
        ..chunks[1]
    };

    let mut lines: Vec<Line> = Vec::new();

    //
    // Target Nodes — multi-select.
    //
    let nodes_focused = opts.focused_section == 0;
    let nodes_label = if nodes_focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(MUTED)
    };
    let all_nodes_selected = opts.nodes.iter().all(|(_, _, s)| *s);
    lines.push(Line::from(vec![
        Span::styled("Target Nodes", nodes_label),
        if all_nodes_selected {
            Span::styled("  (all)", Style::default().fg(DIM))
        } else {
            let count = opts.nodes.iter().filter(|(_, _, s)| *s).count();
            Span::styled(
                format!("  ({}/{})", count, opts.nodes.len()),
                Style::default().fg(DIM),
            )
        },
    ]));

    for (i, (_, name, selected)) in opts.nodes.iter().enumerate() {
        let is_cursor = nodes_focused && i == opts.cursor;
        let check = if *selected { "[\u{2713}]" } else { "[ ]" };
        let style = if is_cursor {
            Style::default()
                .fg(TEXT)
                .bg(POPUP_HIGHLIGHT_BG)
                .add_modifier(Modifier::BOLD)
        } else if *selected {
            Style::default().fg(TEXT)
        } else {
            Style::default().fg(DIM)
        };
        lines.push(Line::from(Span::styled(
            format!("  {} {}", check, name),
            style,
        )));
    }

    //
    // Target Agents — multi-select.
    //
    lines.push(Line::from(""));
    let agents_focused = opts.focused_section == 1;
    let agents_label = if agents_focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(MUTED)
    };
    let all_agents_selected = opts.agents.iter().all(|(_, s)| *s);
    lines.push(Line::from(vec![
        Span::styled("Target Agents", agents_label),
        if all_agents_selected {
            Span::styled("  (all)", Style::default().fg(DIM))
        } else {
            let count = opts.agents.iter().filter(|(_, s)| *s).count();
            Span::styled(
                format!("  ({}/{})", count, opts.agents.len()),
                Style::default().fg(DIM),
            )
        },
    ]));

    for (i, (name, selected)) in opts.agents.iter().enumerate() {
        let is_cursor = agents_focused && i == opts.cursor;
        let check = if *selected { "[\u{2713}]" } else { "[ ]" };
        let style = if is_cursor {
            Style::default()
                .fg(TEXT)
                .bg(POPUP_HIGHLIGHT_BG)
                .add_modifier(Modifier::BOLD)
        } else if *selected {
            Style::default().fg(TEXT)
        } else {
            Style::default().fg(DIM)
        };
        lines.push(Line::from(Span::styled(
            format!("  {} {}", check, name),
            style,
        )));
    }

    //
    // YOLO mode (only for ops, not chains).
    //
    if !opts.is_chain {
        lines.push(Line::from(""));
        let yolo_focused = opts.focused_section == 2;
        let yolo_label = if yolo_focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(MUTED)
        };
        let indicator = if opts.yolo {
            Span::styled(
                " \u{25cf} enabled ",
                Style::default()
                    .fg(Color::Black)
                    .bg(STATUS_RUNNING),
            )
        } else {
            Span::styled(" \u{25cb} disabled ", Style::default().fg(DIM))
        };
        lines.push(Line::from(vec![
            Span::styled("YOLO Mode: ", yolo_label),
            indicator,
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);

    //
    // Hints.
    //
    let hints = Line::from(vec![
        Span::styled("  \u{2191}\u{2193}", Style::default().fg(ACCENT)),
        Span::styled(" navigate  ", Style::default().fg(MUTED)),
        Span::styled("enter", Style::default().fg(ACCENT)),
        Span::styled(" toggle  ", Style::default().fg(MUTED)),
        Span::styled("tab", Style::default().fg(ACCENT)),
        Span::styled(" section  ", Style::default().fg(MUTED)),
        Span::styled("^r", Style::default().fg(ACCENT)),
        Span::styled(" run  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[2]);
}

//
// Trigger create/edit form. Layout drives both render and mouse hit
// testing; `trigger_form_section_rows` emits the same row→section mapping
// without drawing so the mouse handler stays in sync.
//

pub fn render_trigger_form(f: &mut Frame, area: Rect, form: &TriggerForm) {
    let chunks = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(2),
        ratatui::layout::Constraint::Min(1),
        ratatui::layout::Constraint::Length(1),
    ])
    .split(area);

    let title_text = if form.editing_id.is_some() {
        " Edit Trigger"
    } else {
        " New Trigger"
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title_text,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ))),
        chunks[0],
    );

    let inner = Rect {
        x: chunks[1].x + 2,
        width: chunks[1].width.saturating_sub(4),
        ..chunks[1]
    };

    let lines = trigger_form_lines(form);
    f.render_widget(Paragraph::new(lines), inner);

    let hints = Line::from(vec![
        Span::styled(" ^s", Style::default().fg(ACCENT)),
        Span::styled(" save  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" cancel  ", Style::default().fg(MUTED)),
        Span::styled("space", Style::default().fg(ACCENT)),
        Span::styled(" toggle  ", Style::default().fg(MUTED)),
        Span::styled("\u{2190}\u{2192}", Style::default().fg(ACCENT)),
        Span::styled(" cycle  ", Style::default().fg(MUTED)),
        Span::styled("\u{2191}\u{2193}", Style::default().fg(ACCENT)),
        Span::styled(" field", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[2]);
}

//
// Emit (row_index, section, cursor_index_within_section) for every visual
// row in the trigger form body. Cursor index is 0 unless the row is part
// of a list pane (Nodes / Agents), in which case it points at the
// rendered item.
//
pub fn trigger_form_section_rows(
    form: &TriggerForm,
) -> Vec<(usize, TriggerFormSection, usize)> {
    let mut rows = Vec::new();
    let mut row: usize = 0;

    //
    // Chain row.
    //
    rows.push((row, TriggerFormSection::Chain, 0));
    row += 1;
    row += 1; // spacer

    //
    // Type row.
    //
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

    //
    // "Target" header (non-clickable).
    //
    row += 1;

    //
    // Nodes header + one row per node.
    //
    row += 1; // header row
    for i in 0..form.nodes.len() {
        rows.push((row, TriggerFormSection::Nodes, i));
        row += 1;
    }
    row += 1; // spacer

    //
    // OS filter row.
    //
    rows.push((row, TriggerFormSection::OsFilter, 0));
    row += 1;
    row += 1; // spacer

    //
    // Agents header + one row per agent.
    //
    row += 1; // header row
    for i in 0..form.agents.len() {
        rows.push((row, TriggerFormSection::Agents, i));
        row += 1;
    }

    //
    // Include triggering node (only for event-style triggers).
    //
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

    //
    // Row 0: Chain picker.
    //
    lines.push(chain_line(form, section_focus(TriggerFormSection::Chain)));
    lines.push(Line::from(""));

    //
    // Type picker.
    //
    lines.push(type_line(form, section_focus(TriggerFormSection::Type)));

    //
    // Type-specific rows.
    //
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

    //
    // Target.
    //
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Target",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )));

    //
    // Nodes.
    //
    lines.push(section_header(
        "Nodes",
        section_focus(TriggerFormSection::Nodes),
        &format!(
            "({}/{})",
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

    //
    // OS filter text field.
    //
    {
        let focus = section_focus(TriggerFormSection::OsFilter);
        let label = if focus {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(MUTED)
        };
        let value = if form.os_filter.is_empty() {
            Span::styled("(none)", Style::default().fg(DIM))
        } else {
            Span::styled(form.os_filter.clone(), Style::default().fg(TEXT))
        };
        let cursor = if focus { "\u{258f}" } else { "" };
        lines.push(Line::from(vec![
            Span::styled("OS filter: ", label),
            value,
            Span::styled(cursor.to_string(), Style::default().fg(ACCENT)),
        ]));
    }
    lines.push(Line::from(""));

    //
    // Agents.
    //
    lines.push(section_header(
        "Agents",
        section_focus(TriggerFormSection::Agents),
        &format!(
            "({}/{})",
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

    //
    // Include triggering node.
    //
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
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(MUTED)
    };
    let name = form
        .chains
        .get(form.chain_cursor)
        .map(|(_, n)| n.as_str())
        .unwrap_or("(none)");
    Line::from(vec![
        Span::styled("Chain: ", label_style),
        Span::styled(
            format!("\u{25c0} {} \u{25b6}", name),
            Style::default().fg(TEXT),
        ),
    ])
}

fn type_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(MUTED)
    };
    let variants = [
        (TriggerKind::Scheduled, "Scheduled"),
        (TriggerKind::InterceptMatch, "InterceptMatch"),
        (TriggerKind::NewNode, "NewNode"),
    ];
    let mut spans = vec![Span::styled("Type: ", label_style)];
    for (kind, label) in variants {
        if kind == form.kind {
            spans.push(Span::styled(
                format!(" {} ", label),
                Style::default().fg(Color::Black).bg(ACCENT),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {} ", label),
                Style::default().fg(DIM),
            ));
        }
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

fn schedule_kind_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(MUTED)
    };
    let mut spans = vec![Span::styled("Schedule: ", label_style)];
    for (variant, label) in [
        (ScheduleKind::Interval, "Interval"),
        (ScheduleKind::DailyAt, "DailyAt"),
    ] {
        if variant == form.schedule_kind {
            spans.push(Span::styled(
                format!(" {} ", label),
                Style::default().fg(Color::Black).bg(ACCENT),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {} ", label),
                Style::default().fg(DIM),
            ));
        }
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

fn schedule_value_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(MUTED)
    };
    match form.schedule_kind {
        ScheduleKind::Interval => Line::from(vec![
            Span::styled("Interval (min): ", label_style),
            Span::styled(
                format!("{}", form.interval_minutes),
                Style::default().fg(TEXT),
            ),
        ]),
        ScheduleKind::DailyAt => Line::from(vec![
            Span::styled("At: ", label_style),
            Span::styled(
                format!("{:02}:{:02}", form.hour, form.minute),
                Style::default().fg(TEXT),
            ),
        ]),
    }
}

fn recurring_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    toggle_line("Recurring", form.recurring, focused)
}

fn rule_line(form: &TriggerForm, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT)
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
        Span::styled("Rule: ", label_style),
        Span::styled(
            format!("\u{25c0} {} \u{25b6}", value),
            Style::default().fg(TEXT),
        ),
    ])
}

fn section_header(label: &str, focused: bool, suffix: &str) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT)
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
            .fg(TEXT)
            .bg(POPUP_HIGHLIGHT_BG)
            .add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(DIM)
    }
}

fn toggle_line(label: &str, value: bool, focused: bool) -> Line<'static> {
    let label_style = if focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(MUTED)
    };
    let indicator = if value {
        Span::styled(
            " \u{25cf} yes ",
            Style::default()
                .fg(Color::Black)
                .bg(STATUS_RUNNING),
        )
    } else {
        Span::styled(" \u{25cb} no ", Style::default().fg(DIM))
    };
    Line::from(vec![
        Span::styled(format!("{}: ", label), label_style),
        indicator,
    ])
}
