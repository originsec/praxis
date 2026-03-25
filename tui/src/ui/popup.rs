use crate::app::{Popup, PopupKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

const ACCENT: Color = Color::Rgb(100, 180, 100);
const DIM: Color = Color::Rgb(80, 80, 80);
const MUTED: Color = Color::Rgb(120, 120, 120);
const TEXT: Color = Color::Rgb(180, 180, 180);
const HIGHLIGHT_BG: Color = Color::Rgb(35, 40, 35);
const POPUP_BG: Color = Color::Rgb(30, 30, 35);

pub fn render(f: &mut Frame, popup: &Popup) {
    match popup.kind {
        PopupKind::ModelSelect => render_list_select(f, popup, " Select Model "),
        PopupKind::CommandPalette => render_command_palette(f, popup),
        PopupKind::SaveSession => render_save_session(f, popup),
        PopupKind::RunTarget => render_list_select(f, popup, " Select Target "),
        PopupKind::NewOp => {} // rendered separately via new_op_form
        PopupKind::Confirm => {} // rendered separately via confirm
    }
}

pub fn render_confirm(f: &mut Frame, confirm: &crate::app::ConfirmAction) {
    let width = (confirm.message.len() as u16 + 6).min(f.area().width - 4).max(30);
    let height = 5;
    let area = centered_rect_fixed(width, height, f.area());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(180, 60, 60)))
        .title(" Confirm ")
        .title_style(Style::default().fg(Color::Rgb(180, 60, 60)))
        .style(Style::default().bg(POPUP_BG));

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let lines = vec![
        Line::from(Span::styled(
            format!(" {}", confirm.message),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" y", Style::default().fg(Color::Rgb(180, 60, 60))),
            Span::styled(" yes  ", Style::default().fg(MUTED)),
            Span::styled("n", Style::default().fg(ACCENT)),
            Span::styled(" no", Style::default().fg(MUTED)),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

pub fn render_new_op_form(f: &mut Frame, form: &crate::app::NewOpForm) {
    let height = (form.fields.len() as u16 + 3).min(20);
    let width = 60u16.min(f.area().width - 4);
    let area = centered_rect_fixed(width, height, f.area());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(" New Operation ")
        .title_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(POPUP_BG));

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let mut lines: Vec<Line> = Vec::new();

    for (i, (name, value)) in form.fields.iter().enumerate() {
        let is_focused = i == form.focused_field;
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

        lines.push(Line::from(vec![
            Span::styled(format!(" {}: ", name), label_style),
            Span::styled(value.clone(), value_style),
            Span::styled(cursor, Style::default().fg(ACCENT)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Tab/\u{2191}\u{2193} fields  Enter submit  Esc cancel",
        Style::default().fg(DIM),
    )));

    f.render_widget(Paragraph::new(lines), inner);
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
                Span::styled(
                    format!(" {}", item.label),
                    Style::default().fg(TEXT),
                ),
                Span::styled(
                    format!("  {}", item.description),
                    Style::default().fg(DIM),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(HIGHLIGHT_BG)
            .fg(ACCENT)
            .add_modifier(Modifier::BOLD),
    );

    let mut list_state = ListState::default();
    if !filtered.is_empty() {
        list_state.select(Some(popup.selected));
    }

    f.render_stateful_widget(list, area, &mut list_state);
}

fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
