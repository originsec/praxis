mod executions;
mod library;
mod triggers;

use crate::app::{OperationsState, OpsTab};
use crate::ui::theme::{ACCENT, DIM, MUTED};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub use executions::execution_detail_section_at_row;

pub(super) const CHAIN_COLOR: Color = Color::Rgb(80, 180, 180);
pub(super) const OP_COLOR: Color = Color::Rgb(160, 120, 200);

pub fn render(f: &mut Frame, area: Rect, state: &OperationsState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // tabs
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);

    render_tabs(f, chunks[0], state);

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

    let tab_style = |active: bool| -> Style {
        if active {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        }
    };

    let count_style = Style::default().fg(DIM);

    let tabs = Line::from(vec![
        Span::raw("  "),
        Span::styled(
            " Executions ",
            tab_style(state.tab == OpsTab::Executions),
        ),
        Span::styled(format!("{} ", exec_count), count_style),
        Span::styled("  \u{2502}  ", Style::default().fg(DIM)),
        Span::styled(" Library ", tab_style(state.tab == OpsTab::Library)),
        Span::styled(format!("{} ", lib_count), count_style),
        Span::styled("  \u{2502}  ", Style::default().fg(DIM)),
        Span::styled(" Triggers ", tab_style(state.tab == OpsTab::Triggers)),
        Span::styled(format!("{} ", trig_count), count_style),
        Span::raw("      "),
        Span::styled("tab", Style::default().fg(DIM)),
        Span::styled(" switch", Style::default().fg(MUTED)),
    ]);

    let paragraph = Paragraph::new(tabs);
    f.render_widget(paragraph, area);
}

fn render_hints(f: &mut Frame, area: Rect, state: &OperationsState) {
    let hints = match state.tab {
        OpsTab::Library => {
            let mut spans = vec![
                Span::raw(" "),
                Span::styled("^r", Style::default().fg(ACCENT)),
                Span::styled(" execute  ", Style::default().fg(MUTED)),
                Span::styled("^n", Style::default().fg(ACCENT)),
                Span::styled(" new  ", Style::default().fg(MUTED)),
                Span::styled("^e", Style::default().fg(ACCENT)),
                Span::styled(" edit  ", Style::default().fg(MUTED)),
                Span::styled("^d", Style::default().fg(ACCENT)),
                Span::styled(" delete  ", Style::default().fg(MUTED)),
            ];
            if state.filter_focused {
                spans.push(Span::styled("/", Style::default().fg(ACCENT)));
                spans.push(Span::styled(&state.filter, Style::default().fg(ACCENT)));
                spans.push(Span::styled("▏", Style::default().fg(ACCENT)));
                spans.push(Span::styled("  enter apply  esc dismiss", Style::default().fg(DIM)));
            } else if !state.filter.is_empty() {
                spans.push(Span::styled("filter: ", Style::default().fg(DIM)));
                spans.push(Span::styled(&state.filter, Style::default().fg(ACCENT)));
                spans.push(Span::styled("  esc clear", Style::default().fg(DIM)));
            } else {
                spans.push(Span::styled("/ to filter", Style::default().fg(DIM)));
            }
            Line::from(spans)
        }
        OpsTab::Triggers => {
            let mut spans = vec![
                Span::raw(" "),
                Span::styled("enter", Style::default().fg(ACCENT)),
                Span::styled(" toggle  ", Style::default().fg(MUTED)),
                Span::styled("^n", Style::default().fg(ACCENT)),
                Span::styled(" new  ", Style::default().fg(MUTED)),
                Span::styled("^e", Style::default().fg(ACCENT)),
                Span::styled(" edit  ", Style::default().fg(MUTED)),
                Span::styled("^d", Style::default().fg(ACCENT)),
                Span::styled(" delete  ", Style::default().fg(MUTED)),
            ];
            if !state.filter.is_empty() {
                spans.push(Span::styled("filter: ", Style::default().fg(DIM)));
                spans.push(Span::styled(&state.filter, Style::default().fg(ACCENT)));
                spans.push(Span::styled("  esc clear", Style::default().fg(DIM)));
            }
            Line::from(spans)
        }
        OpsTab::Executions => {
            let mut spans = vec![Span::raw(" ")];

            //
            // Show ^c only if selected item is running/queued.
            //
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
                spans.push(Span::styled("^c", Style::default().fg(ACCENT)));
                spans.push(Span::styled(" cancel  ", Style::default().fg(MUTED)));
            }
            spans.push(Span::styled("^d", Style::default().fg(ACCENT)));
            spans.push(Span::styled(" delete  ", Style::default().fg(MUTED)));
            spans.push(Span::styled("^x", Style::default().fg(ACCENT)));
            spans.push(Span::styled(" clear all  ", Style::default().fg(MUTED)));
            if state.filter_focused {
                spans.push(Span::styled("/", Style::default().fg(ACCENT)));
                spans.push(Span::styled(&state.filter, Style::default().fg(ACCENT)));
                spans.push(Span::styled("▏", Style::default().fg(ACCENT)));
                spans.push(Span::styled("  enter apply  esc dismiss", Style::default().fg(DIM)));
            } else if !state.filter.is_empty() {
                spans.push(Span::styled("filter: ", Style::default().fg(DIM)));
                spans.push(Span::styled(&state.filter, Style::default().fg(ACCENT)));
                spans.push(Span::styled("  esc clear", Style::default().fg(DIM)));
            } else {
                spans.push(Span::styled("/ to filter", Style::default().fg(DIM)));
            }
            Line::from(spans)
        }
    };

    f.render_widget(Paragraph::new(hints), area);
}
