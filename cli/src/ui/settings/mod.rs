mod about;
mod agents;
mod forms;
mod llm;
mod service;

use crate::app::{SettingsState, SettingsTab};
use crate::ui::theme::{ACCENT, BG, DIM, MUTED, SETTINGS_HIGHLIGHT_BG, TEXT};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub(super) const EDIT_FG: Color = Color::Rgb(220, 220, 220);

pub fn render(f: &mut Frame, area: Rect, state: &SettingsState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // tabs
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // content
        Constraint::Length(1), // status
    ])
    .split(area);

    render_tabs(f, chunks[0], state);

    let content = Rect {
        x: area.x + 2,
        width: area.width.saturating_sub(4),
        ..chunks[2]
    };

    match state.tab {
        SettingsTab::Llm => llm::render_llm(f, content, state),
        SettingsTab::Agents => agents::render_agents(f, content, state),
        SettingsTab::Service => service::render_service(f, content, state),
        SettingsTab::About => about::render_about(f, content, state),
    }

    if state.dropdown_open {
        forms::render_model_dropdown(f, area, state);
    }

    if let Some(ref form) = state.model_form {
        forms::render_model_form(f, area, form);
    }

    if let Some(ref msg) = state.status_message {
        let style = if msg.starts_with("Failed") || msg.starts_with("Save failed") {
            Style::default().fg(Color::Rgb(180, 60, 60))
        } else {
            Style::default().fg(MUTED)
        };
        let line = Line::from(vec![Span::raw("  "), Span::styled(msg.as_str(), style)]);
        f.render_widget(Paragraph::new(line), chunks[3]);
    }
}

fn render_tabs(f: &mut Frame, area: Rect, state: &SettingsState) {
    let tab_style = |tab: SettingsTab| -> Style {
        if state.tab == tab {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        }
    };

    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled(" LLM ", tab_style(SettingsTab::Llm)),
        Span::styled("  \u{2502}  ", Style::default().fg(DIM)),
        Span::styled(" Agents ", tab_style(SettingsTab::Agents)),
        Span::styled("  \u{2502}  ", Style::default().fg(DIM)),
        Span::styled(" Service ", tab_style(SettingsTab::Service)),
        Span::styled("  \u{2502}  ", Style::default().fg(DIM)),
        Span::styled(" About ", tab_style(SettingsTab::About)),
        Span::raw("      "),
        Span::styled("tab", Style::default().fg(DIM)),
        Span::styled(" switch", Style::default().fg(MUTED)),
    ]);

    f.render_widget(Paragraph::new(line), area);
}

pub(super) fn setting_row<'a>(
    label: &'a str,
    value: &'a str,
    selected: bool,
    editing: bool,
    edit_buffer: &'a str,
) -> Line<'a> {
    let label_style = if selected {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(TEXT)
    };

    let val_display = if editing && selected {
        edit_buffer
    } else {
        value
    };

    let val_style = if editing && selected {
        Style::default().fg(EDIT_FG)
    } else if selected {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(MUTED)
    };

    let cursor = if editing && selected { "\u{258f}" } else { "" };

    Line::from(vec![
        Span::styled(if selected { "\u{25b8} " } else { "  " }, label_style),
        Span::styled(format!("{:<28}", label), label_style),
        Span::styled(val_display.to_string(), val_style),
        Span::styled(cursor, Style::default().fg(ACCENT)),
    ])
}

pub(super) fn toggle_row(label: &str, enabled: bool, selected: bool) -> Line<'_> {
    let label_style = if selected {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(TEXT)
    };

    let (indicator, indicator_style) = if enabled {
        (
            "\u{25cf} enabled",
            Style::default().fg(Color::Rgb(80, 160, 80)),
        )
    } else {
        (
            "\u{25cb} disabled",
            Style::default().fg(Color::Rgb(160, 80, 80)),
        )
    };

    let bg = if selected { SETTINGS_HIGHLIGHT_BG } else { BG };

    Line::from(vec![
        Span::styled(if selected { "\u{25b8} " } else { "  " }, label_style),
        Span::styled(format!("{:<28}", label), label_style),
        Span::styled(indicator, indicator_style.bg(bg)),
    ])
}

pub(super) fn section_header(title: &str) -> Line<'_> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(160, 160, 160))
                .add_modifier(Modifier::BOLD),
        ),
    ])
}
