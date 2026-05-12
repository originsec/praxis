//
// Chain builder modal renderer. Lays out a full-screen modal with a header
// (name, category, timeout, description), a left column of elements and
// connections, a right column of properties for the selected element, and a
// row of action buttons. Returns a `ChainFormHitMap` describing the rects
// the mouse handler tests for clicks.
//

use crate::app::{
    ChainElementDraft, ChainForm, ChainFormEditor, ChainFormSection, ConditionKind, ConnectionDraft,
    ElementKind,
};
use crate::ui::chrome;
use crate::ui::theme::{
    ACCENT, BG, BG_ELEMENT, BG_MENU, BG_SELECTED, BORDER_SUBTLE, DIM, ERROR, MUTED, OK,
    STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

//
// Hit-test geometry returned by the renderer. Stored on App so mouse
// handlers can map clicks back to chain-form actions without re-deriving
// the layout.
//

#[derive(Default, Clone)]
pub struct ChainFormHitMap {
    pub header_fields: Vec<(u8, Rect)>,
    pub elements_panel: Rect,
    pub element_rows: Vec<(usize, Rect)>,
    pub add_element_button: Rect,
    pub properties_panel: Rect,
    pub property_rows: Vec<(u8, Rect)>,
    pub connections_panel: Rect,
    pub connection_rows: Vec<(usize, Rect)>,
    pub add_connection_button: Rect,
    pub save_button: Rect,
    pub cancel_button: Rect,
    pub delete_element_button: Rect,
}

pub fn render_chain_form(f: &mut Frame, area: Rect, form: &ChainForm) -> ChainFormHitMap {
    let chunks = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Length(1), // divider
        Constraint::Length(4), // header fields
        Constraint::Min(1),    // body
        Constraint::Length(1), // buttons row
        Constraint::Length(1), // error / hints
    ])
    .split(area);

    let title = if form.editing_id.is_some() {
        "Edit Chain"
    } else {
        "New Chain"
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title,
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

    let mut hit = ChainFormHitMap::default();

    //
    // Header.
    //
    render_header(f, chunks[2], form, &mut hit);

    //
    // Body: left = elements + connections, right = properties.
    //
    let body = chunks[3];
    let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(body);
    render_left(f, cols[0], form, &mut hit);
    render_right(f, cols[1], form, &mut hit);

    //
    // Buttons.
    //
    render_buttons(f, chunks[4], form, &mut hit);

    //
    // Error / hints row.
    //
    let key_style = Style::default().fg(TEXT_BRIGHT);
    let lbl = Style::default().fg(MUTED);
    let hint_line = if let Some(ref err) = form.error {
        Line::from(vec![
            Span::styled("\u{26A0}", Style::default().fg(ERROR)),
            Span::raw(" "),
            Span::styled(err.clone(), Style::default().fg(ERROR)),
        ])
    } else {
        Line::from(vec![
            Span::styled("tab", key_style),
            Span::styled(" section", lbl),
            Span::raw("   "),
            Span::styled("\u{2191}\u{2193}", key_style),
            Span::styled(" row", lbl),
            Span::raw("   "),
            Span::styled("a", key_style),
            Span::styled(" add", lbl),
            Span::raw("   "),
            Span::styled("d", key_style),
            Span::styled(" delete", lbl),
            Span::raw("   "),
            Span::styled("^s", key_style),
            Span::styled(" save", lbl),
            Span::raw("   "),
            Span::styled("esc", key_style),
            Span::styled(" cancel", lbl),
        ])
    };
    f.render_widget(Paragraph::new(hint_line), chunks[5]);

    //
    // Overlay editors (kind picker / connection editor / op picker).
    //
    if let Some(editor) = form.editor.as_ref() {
        render_editor_overlay(f, area, form, editor);
    }

    hit
}

fn focused_block(focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { ACCENT } else { BORDER_SUBTLE }))
}

fn render_header(f: &mut Frame, area: Rect, form: &ChainForm, hit: &mut ChainFormHitMap) {
    let focused = form.section == ChainFormSection::Header;
    let label_fg = |selected: bool| {
        if selected && focused {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        }
    };
    let value_fg = |selected: bool| {
        if selected && focused {
            Style::default().fg(TEXT_BRIGHT).bg(BG_SELECTED)
        } else {
            Style::default().fg(TEXT_BRIGHT)
        }
    };
    let cursor = |selected: bool| {
        if selected && focused {
            Span::styled("\u{2588}", Style::default().fg(ACCENT))
        } else {
            Span::raw("")
        }
    };

    //
    // Row 0: Name.
    //
    let name_y = area.y;
    let name_field = Rect::new(area.x, name_y, area.width, 1);
    hit.header_fields.push((0, name_field));
    let row = Line::from(vec![
        Span::styled("Name: ", label_fg(form.focused_header_field == 0)),
        Span::styled(form.name.clone(), value_fg(form.focused_header_field == 0)),
        cursor(form.focused_header_field == 0),
    ]);
    f.render_widget(Paragraph::new(row), name_field);

    //
    // Row 1: Category and Timeout.
    //
    let row_y = area.y + 1;
    let half = area.width / 2;
    let cat_rect = Rect::new(area.x, row_y, half, 1);
    let to_rect = Rect::new(area.x + half, row_y, area.width - half, 1);
    hit.header_fields.push((1, cat_rect));
    hit.header_fields.push((2, to_rect));
    let cat = Line::from(vec![
        Span::styled("Category: ", label_fg(form.focused_header_field == 1)),
        Span::styled(form.category.clone(), value_fg(form.focused_header_field == 1)),
        cursor(form.focused_header_field == 1),
    ]);
    f.render_widget(Paragraph::new(cat), cat_rect);
    let timeout = Line::from(vec![
        Span::styled("Timeout (s): ", label_fg(form.focused_header_field == 2)),
        Span::styled(
            if form.timeout.is_empty() {
                "(none)".to_string()
            } else {
                form.timeout.clone()
            },
            value_fg(form.focused_header_field == 2),
        ),
        cursor(form.focused_header_field == 2),
    ]);
    f.render_widget(Paragraph::new(timeout), to_rect);

    //
    // Row 2: Description.
    //
    let desc_rect = Rect::new(area.x, area.y + 2, area.width, 1);
    hit.header_fields.push((3, desc_rect));
    let desc = Line::from(vec![
        Span::styled("Description: ", label_fg(form.focused_header_field == 3)),
        Span::styled(
            form.description.clone(),
            value_fg(form.focused_header_field == 3),
        ),
        cursor(form.focused_header_field == 3),
    ]);
    f.render_widget(Paragraph::new(desc), desc_rect);
}

fn render_left(f: &mut Frame, area: Rect, form: &ChainForm, hit: &mut ChainFormHitMap) {
    let rows = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);
    render_elements(f, rows[0], form, hit);
    render_connections(f, rows[1], form, hit);
}

fn render_elements(f: &mut Frame, area: Rect, form: &ChainForm, hit: &mut ChainFormHitMap) {
    let focused = form.section == ChainFormSection::Elements;
    let block = focused_block(focused).title(Span::styled(
        " Elements ",
        Style::default()
            .fg(if focused { ACCENT } else { MUTED })
            .add_modifier(Modifier::BOLD),
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    hit.elements_panel = inner;

    //
    // Element rows take all but the last line; last line is [+ Add].
    //
    let list_height = inner.height.saturating_sub(1);
    let visible_rows = list_height as usize;
    let start = if form.element_selected >= visible_rows {
        form.element_selected + 1 - visible_rows
    } else {
        0
    };

    for (display_row, idx) in (start..form.elements.len().min(start + visible_rows)).enumerate() {
        let row_y = inner.y + display_row as u16;
        let row_rect = Rect::new(inner.x, row_y, inner.width, 1);
        let el = &form.elements[idx];
        let selected = idx == form.element_selected && focused;
        let style = if selected {
            Style::default().bg(BG_SELECTED).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let line = Line::from(vec![
            chrome::pill(el.kind.short(), kind_color(el.kind)),
            Span::raw(" "),
            Span::styled(
                element_label(el),
                style.fg(if selected { TEXT_BRIGHT } else { TEXT }),
            ),
        ]);
        f.render_widget(
            Paragraph::new(line).style(if selected {
                Style::default().bg(BG_SELECTED)
            } else {
                Style::default()
            }),
            row_rect,
        );
        hit.element_rows.push((idx, row_rect));
    }

    let add_y = inner.y + list_height;
    let add_rect = Rect::new(inner.x, add_y, inner.width, 1);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "[+ Add Element]",
            Style::default().fg(OK),
        ))),
        add_rect,
    );
    hit.add_element_button = add_rect;
}

fn render_connections(f: &mut Frame, area: Rect, form: &ChainForm, hit: &mut ChainFormHitMap) {
    let focused = form.section == ChainFormSection::Connections;
    let block = focused_block(focused).title(Span::styled(
        " Connections ",
        Style::default()
            .fg(if focused { ACCENT } else { MUTED })
            .add_modifier(Modifier::BOLD),
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    hit.connections_panel = inner;

    let list_height = inner.height.saturating_sub(1);
    let visible_rows = list_height as usize;
    let start = if form.connection_selected >= visible_rows {
        form.connection_selected + 1 - visible_rows
    } else {
        0
    };

    for (display_row, idx) in
        (start..form.connections.len().min(start + visible_rows)).enumerate()
    {
        let row_y = inner.y + display_row as u16;
        let row_rect = Rect::new(inner.x, row_y, inner.width, 1);
        let selected = idx == form.connection_selected && focused;
        let conn = &form.connections[idx];
        let line = Line::from(connection_spans(conn, selected));
        f.render_widget(
            Paragraph::new(line).style(if selected {
                Style::default().bg(BG_SELECTED)
            } else {
                Style::default()
            }),
            row_rect,
        );
        hit.connection_rows.push((idx, row_rect));
    }

    let add_y = inner.y + list_height;
    let add_rect = Rect::new(inner.x, add_y, inner.width, 1);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "[+ Add Connection]",
            Style::default().fg(OK),
        ))),
        add_rect,
    );
    hit.add_connection_button = add_rect;
}

fn render_right(f: &mut Frame, area: Rect, form: &ChainForm, hit: &mut ChainFormHitMap) {
    let focused = form.section == ChainFormSection::Properties;
    let block = focused_block(focused).title(Span::styled(
        " Properties ",
        Style::default()
            .fg(if focused { ACCENT } else { MUTED })
            .add_modifier(Modifier::BOLD),
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    hit.properties_panel = inner;

    let Some(el) = form.selected_element() else {
        f.render_widget(
            Paragraph::new(Span::styled(
                "No element selected",
                Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
            )),
            inner,
        );
        return;
    };

    let prop_focused_idx = form.focused_prop_field;

    let mut lines: Vec<Line> = Vec::new();
    let mut field_rects: Vec<(u8, Rect)> = Vec::new();
    let mut y = inner.y;

    // Read-only: ID
    lines.push(Line::from(vec![
        Span::styled("ID:   ", Style::default().fg(DIM)),
        Span::styled(el.id.clone(), Style::default().fg(TEXT_BRIGHT)),
    ]));
    y += 1;

    // Row 0 (focusable): Kind
    let kind_rect = Rect::new(inner.x, y, inner.width, 1);
    field_rects.push((0, kind_rect));
    lines.push(prop_row_label_value(
        "Kind: ",
        Span::styled(
            format!("\u{25c0} {} \u{25b6}", el.kind.label()),
            Style::default().fg(TEXT_BRIGHT),
        ),
        focused && prop_focused_idx == 0,
    ));
    y += 1;

    // Per-kind fields.
    let extra = match el.kind {
        ElementKind::Operation => vec![
            ("Operation", el.op_name.clone(), false, true),
            ("Model ref", el.model_ref.clone(), false, false),
        ],
        ElementKind::Transform => vec![
            ("Prompt", el.prompt.clone(), false, false),
            ("Model ref", el.model_ref.clone(), false, false),
        ],
        ElementKind::GenericPrompt => {
            vec![("Prompt", el.prompt.clone(), false, false)]
        }
        ElementKind::Memory => vec![
            ("Key", el.memory_key.clone(), false, false),
            (
                "Mode",
                if el.memory_mode == 0 {
                    "Store".to_string()
                } else {
                    "Retrieve".to_string()
                },
                true,
                false,
            ),
        ],
        ElementKind::Loop => vec![("Max iterations", el.max_iterations.clone(), false, false)],
        ElementKind::Tool => vec![
            ("Tool name", el.tool_name.clone(), false, false),
            ("Params (JSON)", el.tool_params.clone(), false, false),
        ],
        ElementKind::Payload => vec![("Payload id", el.payload_id.clone(), false, false)],
        ElementKind::Trigger | ElementKind::Termination => vec![],
    };
    for (i, (label, value, toggle, op_picker)) in extra.iter().enumerate() {
        let row_rect = Rect::new(inner.x, y, inner.width, 1);
        let field_idx = (i + 1) as u8;
        field_rects.push((field_idx, row_rect));
        let is_focused = focused && prop_focused_idx == field_idx;
        let value_span = if *toggle {
            Span::styled(
                format!("\u{25c0} {} \u{25b6}", value),
                Style::default().fg(TEXT_BRIGHT),
            )
        } else if *op_picker {
            Span::styled(
                format!(
                    "{}  (\u{25c0}\u{25b6} pick)",
                    if value.is_empty() { "(empty)" } else { value }
                ),
                Style::default().fg(if value.is_empty() { DIM } else { TEXT_BRIGHT }),
            )
        } else if value.is_empty() {
            Span::styled("(empty)", Style::default().fg(DIM))
        } else {
            Span::styled(value.clone(), Style::default().fg(TEXT_BRIGHT))
        };
        lines.push(prop_row_label_value(
            &format!("{}: ", label),
            value_span,
            is_focused,
        ));
        y += 1;
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    hit.property_rows = field_rects;

    // Delete element button on last row of properties panel.
    if inner.height > 0 {
        let del_y = inner.y + inner.height.saturating_sub(1);
        let del_rect = Rect::new(inner.x, del_y, inner.width, 1);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "[Delete Element]",
                Style::default().fg(ERROR),
            ))),
            del_rect,
        );
        hit.delete_element_button = del_rect;
    }
}

fn render_buttons(f: &mut Frame, area: Rect, form: &ChainForm, hit: &mut ChainFormHitMap) {
    let save_focused = form.section == ChainFormSection::Buttons;
    let save_rect = Rect::new(area.x, area.y, 10, 1);
    let cancel_rect = Rect::new(area.x + 12, area.y, 10, 1);
    hit.save_button = save_rect;
    hit.cancel_button = cancel_rect;
    let save_style = if save_focused {
        Style::default().fg(BG).bg(OK).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(OK).bg(BG_ELEMENT)
    };
    let cancel_style = Style::default().fg(MUTED).bg(BG_ELEMENT);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled("  Save  ", save_style))),
        save_rect,
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(" Cancel ", cancel_style))),
        cancel_rect,
    );
}

fn render_editor_overlay(
    f: &mut Frame,
    area: Rect,
    form: &ChainForm,
    editor: &ChainFormEditor,
) {
    let popup_w = 60u16.min(area.width.saturating_sub(4));
    let popup_h = 16u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width - popup_w) / 2;
    let y = area.y + (area.height - popup_h) / 2;
    let rect = Rect::new(x, y, popup_w, popup_h);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG_MENU));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    match editor {
        ChainFormEditor::PickElementKind { cursor } => {
            let mut lines = vec![Line::from(Span::styled(
                "Select element type",
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ))];
            lines.push(Line::from(""));
            for (i, k) in ElementKind::ALL.iter().enumerate() {
                let selected = i == *cursor;
                let style = if selected {
                    Style::default().fg(TEXT_BRIGHT).bg(BG_SELECTED)
                } else {
                    Style::default().fg(TEXT)
                };
                lines.push(Line::from(vec![
                    Span::raw(if selected { " \u{276f} " } else { "   " }),
                    chrome::pill(k.short(), kind_color(*k)),
                    Span::raw(" "),
                    Span::styled(k.label().to_string(), style),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "enter add    esc cancel",
                Style::default().fg(DIM),
            )));
            f.render_widget(Paragraph::new(lines), inner);
        }
        ChainFormEditor::EditConnection {
            from_idx,
            to_idx,
            from_port,
            to_port,
            condition,
            focus,
            editing_idx,
        } => {
            let title = if editing_idx.is_some() {
                "Edit Connection"
            } else {
                "Add Connection"
            };
            let mut lines = vec![
                Line::from(Span::styled(
                    title,
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ];
            let from_label = form
                .elements
                .get(*from_idx)
                .map(|e| element_label(e))
                .unwrap_or_else(|| "(none)".to_string());
            let to_label = form
                .elements
                .get(*to_idx)
                .map(|e| element_label(e))
                .unwrap_or_else(|| "(none)".to_string());
            let row =
                |label: &str, value: String, focused: bool, with_arrows: bool| -> Line<'static> {
                    let label_style = if focused {
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(MUTED)
                    };
                    let value_style = if focused {
                        Style::default().fg(TEXT_BRIGHT).bg(BG_SELECTED)
                    } else {
                        Style::default().fg(TEXT_BRIGHT)
                    };
                    let value_text = if with_arrows {
                        format!("\u{25c0} {} \u{25b6}", value)
                    } else {
                        value
                    };
                    Line::from(vec![
                        Span::styled(format!("  {:<10}", label), label_style),
                        Span::styled(value_text, value_style),
                    ])
                };
            lines.push(row("From:", from_label, *focus == 0, true));
            lines.push(row("To:", to_label, *focus == 1, true));
            lines.push(row("From port:", from_port.clone(), *focus == 2, true));
            lines.push(row("To port:", to_port.clone(), *focus == 3, true));
            lines.push(row(
                "Condition:",
                condition_label(*condition).to_string(),
                *focus == 4,
                true,
            ));
            lines.push(Line::from(""));
            let save_style = if *focus == 5 {
                Style::default().fg(BG).bg(OK).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(OK).bg(BG_ELEMENT)
            };
            let cancel_style = if *focus == 6 {
                Style::default().fg(BG).bg(MUTED).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED).bg(BG_ELEMENT)
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("  Save  ", save_style),
                Span::raw("  "),
                Span::styled(" Cancel ", cancel_style),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "tab field   \u{2190}\u{2192} cycle   enter save",
                Style::default().fg(DIM),
            )));
            f.render_widget(Paragraph::new(lines), inner);
        }
        ChainFormEditor::PickOpName { cursor, filter } => {
            let mut lines = vec![Line::from(Span::styled(
                "Pick Operation",
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ))];
            lines.push(Line::from(vec![
                Span::styled("filter: ", Style::default().fg(MUTED)),
                Span::styled(filter.clone(), Style::default().fg(ACCENT)),
                Span::styled("\u{2588}", Style::default().fg(ACCENT)),
            ]));
            lines.push(Line::from(""));
            let filtered: Vec<&String> = form
                .available_op_names
                .iter()
                .filter(|n| {
                    filter.is_empty() || n.to_lowercase().contains(&filter.to_lowercase())
                })
                .collect();
            let max_rows = (inner.height as usize).saturating_sub(5);
            let start = if *cursor >= max_rows && max_rows > 0 {
                *cursor + 1 - max_rows
            } else {
                0
            };
            for (i, name) in filtered.iter().enumerate().skip(start).take(max_rows) {
                let selected = i == *cursor;
                let style = if selected {
                    Style::default().fg(TEXT_BRIGHT).bg(BG_SELECTED)
                } else {
                    Style::default().fg(TEXT)
                };
                lines.push(Line::from(vec![
                    Span::raw(if selected { " \u{276f} " } else { "   " }),
                    Span::styled((*name).clone(), style),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "enter pick    esc cancel",
                Style::default().fg(DIM),
            )));
            f.render_widget(Paragraph::new(lines), inner);
        }
    }
}

fn prop_row_label_value<'a>(label: &str, value: Span<'a>, focused: bool) -> Line<'a> {
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let cursor = if focused {
        Span::styled("\u{2588}", Style::default().fg(ACCENT))
    } else {
        Span::raw("")
    };
    Line::from(vec![Span::styled(label.to_string(), label_style), value, cursor])
}

fn kind_color(kind: ElementKind) -> ratatui::style::Color {
    use crate::ui::theme;
    match kind {
        ElementKind::Trigger => STATUS_RUNNING,
        ElementKind::Operation => theme::ACCENT,
        ElementKind::Transform => OK,
        ElementKind::GenericPrompt => MUTED,
        ElementKind::Memory => ACCENT,
        ElementKind::Loop => STATUS_RUNNING,
        ElementKind::Tool => OK,
        ElementKind::Payload => ACCENT,
        ElementKind::Termination => ERROR,
    }
}

fn element_label(el: &ChainElementDraft) -> String {
    let detail = match el.kind {
        ElementKind::Operation if !el.op_name.is_empty() => format!(" ({})", el.op_name),
        ElementKind::Tool if !el.tool_name.is_empty() => format!(" ({})", el.tool_name),
        ElementKind::Memory if !el.memory_key.is_empty() => format!(" ({})", el.memory_key),
        _ => String::new(),
    };
    format!("{}{}", el.id, detail)
}

fn connection_spans(conn: &ConnectionDraft, selected: bool) -> Vec<Span<'static>> {
    let text_style = if selected {
        Style::default().fg(TEXT_BRIGHT).bg(BG_SELECTED)
    } else {
        Style::default().fg(TEXT)
    };
    let arrow = Style::default().fg(DIM);
    let mut spans = vec![
        Span::styled(conn.from_element.clone(), text_style),
        Span::styled(format!(":{}", conn.from_port), arrow),
        Span::raw(" "),
        Span::styled("\u{2192}", arrow),
        Span::raw(" "),
        Span::styled(conn.to_element.clone(), text_style),
        Span::styled(format!(":{}", conn.to_port), arrow),
    ];
    if conn.condition != ConditionKind::None {
        spans.push(Span::raw("  "));
        spans.push(chrome::pill(
            condition_label(conn.condition),
            condition_color(conn.condition),
        ));
    }
    spans
}

fn condition_label(c: ConditionKind) -> &'static str {
    match c {
        ConditionKind::None => "any",
        ConditionKind::OnSuccess => "on success",
        ConditionKind::OnFailure => "on failure",
    }
}

fn condition_color(c: ConditionKind) -> ratatui::style::Color {
    match c {
        ConditionKind::None => MUTED,
        ConditionKind::OnSuccess => OK,
        ConditionKind::OnFailure => ERROR,
    }
}
