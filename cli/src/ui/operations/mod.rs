mod executions;
mod library;
pub mod new_op_form;
mod triggers;

use crate::app::{App, OperationsState, OpsTab};
use crate::ui::chrome;
use crate::ui::common::table_data_start_margin_header;
use crate::ui::hits::{HintRegistrar, MouseAction, OpsHintAction, RowSelect, RowSelectKind};
use crate::ui::theme::{ACCENT, BORDER_SUBTLE, DIM, MUTED, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub use executions::execution_detail_section_at_row;

pub(super) const CHAIN_COLOR: Color = Color::Rgb(95, 195, 195);
pub(super) const OP_COLOR: Color = Color::Rgb(180, 130, 215);

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.operations;
    let chunks = Layout::vertical([
        Constraint::Length(1), // tabs
        Constraint::Length(1), // divider
        Constraint::Length(1), // filter
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);

    render_tabs(f, chunks[0], app, state);
    render_divider(f, chunks[1]);
    render_filter_row(f, chunks[2], state);

    match state.tab {
        OpsTab::Library => library::render_library(f, chunks[3], state),
        OpsTab::Executions => executions::render_executions(f, chunks[3], state),
        OpsTab::Triggers => triggers::render_triggers(f, chunks[3], state),
    }

    //
    // Skip list hits while the new-op modal is open so clicks go to
    // the form (later HitLayer entries still win, but this keeps the
    // backdrop inert like settings).
    //
    if app.new_op_form.is_none() {
        register_content_hits(app, chunks[3], state);
    }
    render_hints(f, chunks[4], app, state);
}

fn render_filter_row(f: &mut Frame, area: Rect, state: &OperationsState) {
    use crate::ui::filter_bar::{self, FilterBarModel};
    filter_bar::render(
        f,
        area,
        &FilterBarModel {
            focused: state.filter_focused,
            query: &state.filter,
            placeholder: "filter",
            extra_pills: Vec::new(),
            meta: None,
        },
    );
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App, state: &OperationsState) {
    let lib_count = state.op_definitions.iter().filter(|d| !d.disabled).count()
        + state
            .chain_definitions
            .iter()
            .filter(|c| !c.disabled)
            .count();
    let exec_count = state.operations.len() + state.chain_executions.len();
    let trig_count = state.triggers.len();

    let specs = [
        (OpsTab::Executions, "Executions", exec_count),
        (OpsTab::Library, "Library", lib_count),
        (OpsTab::Triggers, "Triggers", trig_count),
    ];
    let mut x = 0u16;
    for (i, (tab, label, n)) in specs.iter().enumerate() {
        let w = chrome::tab_width(label, Some(*n));
        app.hits_register(
            Rect::new(area.x.saturating_add(x), area.y, w, 1),
            MouseAction::OpsTab(*tab),
        );
        x += w;
        if i + 1 < specs.len() {
            x += chrome::tab_sep_width();
        }
    }

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

fn render_hints(f: &mut Frame, area: Rect, app: &App, state: &OperationsState) {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let gap = Span::raw("    ");
    let mut reg = HintRegistrar::new(app, area);

    let hints = match state.tab {
        OpsTab::Library => {
            reg.chip("^r", MouseAction::OpsHint(OpsHintAction::Execute));
            reg.chip(" execute", MouseAction::OpsHint(OpsHintAction::Execute));
            reg.gap(4);
            reg.chip("^n", MouseAction::OpsHint(OpsHintAction::NewOp));
            reg.chip(" new op", MouseAction::OpsHint(OpsHintAction::NewOp));
            reg.gap(4);
            reg.chip("^!n", MouseAction::OpsHint(OpsHintAction::NewChain));
            reg.chip(" new chain", MouseAction::OpsHint(OpsHintAction::NewChain));
            reg.gap(4);
            reg.chip("^e", MouseAction::OpsHint(OpsHintAction::Edit));
            reg.chip(" edit", MouseAction::OpsHint(OpsHintAction::Edit));
            reg.gap(4);
            reg.chip("^d", MouseAction::OpsHint(OpsHintAction::Delete));
            reg.chip(" delete", MouseAction::OpsHint(OpsHintAction::Delete));
            reg.gap(4);

            let mut spans = vec![
                Span::styled("^r", key),
                Span::styled(" execute", label),
                gap.clone(),
                Span::styled("^n", key),
                Span::styled(" new op", label),
                gap.clone(),
                Span::styled("^!n", key),
                Span::styled(" new chain", label),
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
            reg.chip("\u{21B5}", MouseAction::OpsHint(OpsHintAction::ToggleTrigger));
            reg.chip(" toggle", MouseAction::OpsHint(OpsHintAction::ToggleTrigger));
            reg.gap(4);
            reg.chip("^n", MouseAction::OpsHint(OpsHintAction::NewTrigger));
            reg.chip(" new", MouseAction::OpsHint(OpsHintAction::NewTrigger));
            reg.gap(4);
            reg.chip("^e", MouseAction::OpsHint(OpsHintAction::EditTrigger));
            reg.chip(" edit", MouseAction::OpsHint(OpsHintAction::EditTrigger));
            reg.gap(4);
            reg.chip("^d", MouseAction::OpsHint(OpsHintAction::DeleteTrigger));
            reg.chip(" delete", MouseAction::OpsHint(OpsHintAction::DeleteTrigger));

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

            let mut spans: Vec<Span> = Vec::new();
            if selected_active {
                reg.chip("^c", MouseAction::OpsHint(OpsHintAction::CancelExecution));
                reg.chip(" cancel", MouseAction::OpsHint(OpsHintAction::CancelExecution));
                reg.gap(4);
                spans.push(Span::styled("^c", key));
                spans.push(Span::styled(" cancel", label));
                spans.push(gap.clone());
            }
            reg.chip("^d", MouseAction::OpsHint(OpsHintAction::DeleteExecution));
            reg.chip(" delete", MouseAction::OpsHint(OpsHintAction::DeleteExecution));
            reg.gap(4);
            reg.chip("^x", MouseAction::OpsHint(OpsHintAction::ClearAllExecutions));
            reg.chip(" clear all", MouseAction::OpsHint(OpsHintAction::ClearAllExecutions));
            reg.gap(4);

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

fn register_content_hits(app: &App, main_area: Rect, state: &OperationsState) {
    let panes = crate::ui::list_detail::layout(main_area, state.split_percent);
    app.text_selection_region_register(panes.list);
    app.text_selection_region_register(panes.detail);
    let list_area = panes.list;
    let detail_area = panes.detail;
    let detail_inner = Rect::new(
        detail_area.x.saturating_add(1),
        detail_area.y.saturating_add(1),
        detail_area.width.saturating_sub(2),
        detail_area.height.saturating_sub(2),
    );

    let row_kind = match state.tab {
        OpsTab::Library => RowSelectKind::OpsLibrary,
        OpsTab::Executions => RowSelectKind::OpsExecutions,
        OpsTab::Triggers => RowSelectKind::OpsTriggers,
    };
    app.hits_register(
        list_area,
        MouseAction::SelectRow(RowSelect {
            kind: row_kind,
            table_area: list_area,
            data_start: table_data_start_margin_header(list_area),
        }),
    );
    app.hits_register(detail_area, MouseAction::OpsDetailFocus);
    if state.tab == OpsTab::Executions {
        app.hits_register(
            detail_inner,
            MouseAction::OpsExecDetail { inner: detail_inner },
        );
    }
    //
    // Split border last so drag wins hit-test on the divider.
    //
    app.hits_register(panes.border, MouseAction::OpsSplitDragStart);
}

fn append_filter_hint(spans: &mut Vec<Span<'static>>, state: &OperationsState) {
    //
    // Filter chrome lives in the dedicated filter row; only show active
    // query status here when focused or non-empty.
    //
    if state.filter_focused {
        spans.push(Span::styled(
            "typing filter\u{2026}  \u{21B5} apply  esc dismiss",
            Style::default().fg(DIM),
        ));
    } else if !state.filter.is_empty() {
        spans.push(Span::styled("filter ", Style::default().fg(DIM)));
        spans.push(Span::styled(
            state.filter.clone(),
            Style::default().fg(ACCENT),
        ));
        spans.push(Span::styled("    esc clear", Style::default().fg(DIM)));
    }
}
