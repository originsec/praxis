use crate::app::{Popup, PopupKind};
use crate::ui::common::centered_rect_fixed;
use crate::ui::theme::{ACCENT, DIM, MUTED, POPUP_BG, POPUP_HIGHLIGHT_BG, TEXT};
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
                            .bg(Color::Rgb(180, 160, 60)),
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
                .bg(Color::Rgb(35, 40, 35))
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
                .bg(Color::Rgb(35, 40, 35))
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
                    .bg(Color::Rgb(180, 160, 60)),
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
