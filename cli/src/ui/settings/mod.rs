mod about;
mod agents;
mod forms;
mod intercept;
mod llm;
mod service;

use crate::app::{SettingsState, SettingsTab};
use crate::ui::chrome;
use crate::ui::theme::{
    ACCENT, BG_SELECTED, BORDER_SUBTLE, DIM, MUTED, OK, STATUS_FAIL, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub(super) const EDIT_FG: Color = Color::Rgb(225, 228, 232);

pub fn render(f: &mut Frame, area: Rect, state: &SettingsState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // tabs
        Constraint::Length(1), // divider
        Constraint::Min(1),    // content
        Constraint::Length(1), // status
    ])
    .split(area);

    render_tabs(f, chunks[0], state);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(chunks[1].width as usize),
            Style::default().fg(BORDER_SUBTLE),
        ))),
        chunks[1],
    );

    let content = Rect {
        x: area.x + 2,
        width: area.width.saturating_sub(4),
        ..chunks[2]
    };

    match state.tab {
        SettingsTab::Llm => llm::render_llm(f, content, state),
        SettingsTab::Agents => agents::render_agents(f, content, state),
        SettingsTab::Intercept => intercept::render_intercept(f, content, state),
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
        let (icon, style) = if msg.starts_with("Failed") || msg.starts_with("Save failed") {
            (
                chrome::dot(STATUS_FAIL),
                Style::default()
                    .fg(STATUS_FAIL)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (chrome::dot(OK), Style::default().fg(MUTED))
        };
        let line = Line::from(vec![
            icon,
            Span::raw(" "),
            Span::styled(msg.as_str(), style),
        ]);
        f.render_widget(Paragraph::new(line), chunks[3]);
    }
}

fn render_tabs(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut spans: Vec<Span> = Vec::new();
    let pairs: &[(SettingsTab, &str)] = &[
        (SettingsTab::Llm, "LLM"),
        (SettingsTab::Agents, "Agents"),
        (SettingsTab::Intercept, "Intercept"),
        (SettingsTab::Service, "Service"),
        (SettingsTab::About, "About"),
    ];
    for (i, (tab, label)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(chrome::tab_sep());
        }
        spans.extend(chrome::tab(label, None, state.tab == *tab));
    }
    spans.push(Span::raw("      "));
    spans.push(Span::styled("tab", Style::default().fg(TEXT_BRIGHT)));
    spans.push(Span::styled(" switch", Style::default().fg(MUTED)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub(super) fn setting_row<'a>(
    label: &'a str,
    value: &'a str,
    selected: bool,
    editing: bool,
    edit_buffer: &'a str,
) -> Line<'a> {
    let label_style = if selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };

    let val_display = if editing && selected {
        edit_buffer
    } else {
        value
    };

    let val_style = if editing && selected {
        Style::default().fg(EDIT_FG)
    } else if selected {
        Style::default().fg(TEXT_BRIGHT)
    } else {
        Style::default().fg(DIM)
    };

    let cursor = if editing && selected { "\u{2588}" } else { "" };
    let prefix = if selected { "\u{276f} " } else { "  " };

    Line::from(vec![
        Span::styled(prefix, label_style),
        Span::styled(format!("{:<28}", label), label_style),
        Span::styled(val_display.to_string(), val_style),
        Span::styled(cursor, Style::default().fg(ACCENT)),
    ])
}

pub(super) fn toggle_row(label: &str, enabled: bool, selected: bool) -> Line<'_> {
    let label_style = if selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };

    let (indicator_text, indicator_color) = if enabled {
        ("\u{25cf} enabled", OK)
    } else {
        ("\u{25cb} disabled", DIM)
    };

    let prefix = if selected { "\u{276f} " } else { "  " };

    Line::from(vec![
        Span::styled(prefix, label_style),
        Span::styled(format!("{:<28}", label), label_style),
        Span::styled(indicator_text, Style::default().fg(indicator_color)),
    ])
}

pub(super) fn section_header(title: &str) -> Line<'_> {
    Line::from(vec![Span::styled(
        title.to_string(),
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )])
}

fn _unused() {
    let _ = (BG_SELECTED, ACCENT);
}
