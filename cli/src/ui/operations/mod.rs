mod executions;
mod library;
mod triggers;

use crate::app::{OperationsState, OpsTab};
use crate::ui::chrome;
use crate::ui::theme::{ACCENT, BORDER_SUBTLE, DIM, MUTED, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub use executions::execution_detail_section_at_row;

pub(super) const CHAIN_COLOR: Color = Color::Rgb(95, 195, 195);
pub(super) const OP_COLOR: Color = Color::Rgb(180, 130, 215);

pub fn render(f: &mut Frame, area: Rect, state: &OperationsState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // tabs
        Constraint::Length(1), // divider
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);

    render_tabs(f, chunks[0], state);
    render_divider(f, chunks[1]);

    match state.tab {
        OpsTab::Library => library::render_library(f, chunks[2], state),
        OpsTab::Executions => executions::render_executions(f, chunks[2], state),
        OpsTab::Triggers => triggers::render_triggers(f, chunks[2], state),
    }

    render_hints(f, chunks[3], state);
}

fn render_tabs(f: &mut Frame, area: Rect, state: &OperationsState) {
    let lib_count = state.op_definitions.iter().filter(|d| !d.disabled).count()
        + state
            .chain_definitions
            .iter()
            .filter(|c| !c.disabled)
            .count();
    let exec_count = state.operations.len() + state.chain_executions.len();
    let trig_count = state.triggers.len();

    let mut spans: Vec<Span> = Vec::new();
    spans.extend(chrome::tab(
        "Executions",
        Some(exec_count),
        state.tab == OpsTab::Executions,
    ));
    spans.push(chrome::tab_sep());
    spans.extend(chrome::tab(
        "Library",
        Some(lib_count),
        state.tab == OpsTab::Library,
    ));
    spans.push(chrome::tab_sep());
    spans.extend(chrome::tab(
        "Triggers",
        Some(trig_count),
        state.tab == OpsTab::Triggers,
    ));

    spans.push(Span::raw("      "));
    spans.push(Span::styled("tab", Style::default().fg(TEXT_BRIGHT)));
    spans.push(Span::styled(" switch", Style::default().fg(MUTED)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_divider(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(area.width as usize),
            Style::default().fg(BORDER_SUBTLE),
        ))),
        area,
    );
}

fn render_hints(f: &mut Frame, area: Rect, state: &OperationsState) {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let gap = Span::raw("    ");

    let hints = match state.tab {
        OpsTab::Library => {
            let mut spans = vec![
                Span::styled("^r", key),
                Span::styled(" execute", label),
                gap.clone(),
                Span::styled("^n", key),
                Span::styled(" new op", label),
                gap.clone(),
                Span::styled("^!", key),
                Span::styled(" newchain", label),
                gap.clone(),
                Span::styled("^e", key),
                Span::styled(" edit", label),
                gap.clone(),
                Span::styled("^d", key),
                Span::styled(" delete", label),
                gap.clone(),
            ];
            append_filter_hint(&mut spans, state);
            Line::from(spans)
        }
        OpsTab::Triggers => {
            let mut spans = vec![
                Span::styled("\u{21B5}", key),
                Span::styled(" toggle", label),
                gap.clone(),
                Span::styled("^n", key),
                Span::styled(" new", label),
                gap.clone(),
                Span::styled("^e", key),
                Span::styled(" edit", label),
                gap.clone(),
                Span::styled("^d", key),
                Span::styled(" delete", label),
            ];
            if !state.filter.is_empty() {
                spans.push(gap.clone());
                spans.push(Span::styled("filter ", Style::default().fg(DIM)));
                spans.push(Span::styled(
                    state.filter.clone(),
                    Style::default().fg(ACCENT),
                ));
            }
            Line::from(spans)
        }
        OpsTab::Executions => {
            let mut spans: Vec<Span> = Vec::new();
            let sorted = crate::app::App::sorted_exec_static(
                &state.operations,
                &state.chain_executions,
                &state.filter,
            );
            let selected_active = sorted
                .get(state.exec_selected)
                .map(|(is_op, idx)| {
                    if *is_op {
                        state.operations.get(*idx).is_some_and(|o| {
                            matches!(
                                o.status,
                                common::SemanticOpStatus::Running
                                    | common::SemanticOpStatus::Queued
                            )
                        })
                    } else {
                        state.chain_executions.get(*idx).is_some_and(|c| {
                            matches!(
                                c.status,
                                common::ChainExecutionStatus::Running
                                    | common::ChainExecutionStatus::Queued
                            )
                        })
                    }
                })
                .unwrap_or(false);

            if selected_active {
                spans.push(Span::styled("^c", key));
                spans.push(Span::styled(" cancel", label));
                spans.push(gap.clone());
            }
            spans.push(Span::styled("^d", key));
            spans.push(Span::styled(" delete", label));
            spans.push(gap.clone());
            spans.push(Span::styled("^x", key));
            spans.push(Span::styled(" clear all", label));
            spans.push(gap.clone());
            append_filter_hint(&mut spans, state);
            Line::from(spans)
        }
    };

    f.render_widget(Paragraph::new(hints), area);
}

fn append_filter_hint(spans: &mut Vec<Span<'static>>, state: &OperationsState) {
    if state.filter_focused {
        spans.push(Span::styled("/", Style::default().fg(ACCENT)));
        spans.push(Span::styled(
            state.filter.clone(),
            Style::default().fg(ACCENT),
        ));
        spans.push(Span::styled("\u{2588}", Style::default().fg(ACCENT)));
        spans.push(Span::styled(
            "    \u{21B5} apply  esc dismiss",
            Style::default().fg(DIM),
        ));
    } else if !state.filter.is_empty() {
        spans.push(Span::styled("filter ", Style::default().fg(DIM)));
        spans.push(Span::styled(
            state.filter.clone(),
            Style::default().fg(ACCENT),
        ));
        spans.push(Span::styled("    esc clear", Style::default().fg(DIM)));
    } else {
        spans.push(Span::styled(
            "/ to filter",
            Style::default().fg(DIM).add_modifier(ratatui::style::Modifier::ITALIC),
        ));
    }
}
