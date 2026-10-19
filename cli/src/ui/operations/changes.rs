use crate::app::{App, OperationsState};
use crate::ui::chrome;
use crate::ui::common::focused_panel;
use crate::ui::theme::{BG_SELECTED, DIM, ERROR, MUTED, OK, TERTIARY, TEXT, TEXT_BRIGHT, WARN};
use common::config_change::{ChangeKind, ChangeState, ConfigChangeRecord, DriftStatus};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

pub(super) fn render_changes(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.operations;
    let panes = crate::ui::list_detail::layout(area, state.split_percent);
    let filtered = app.filtered_changes();
    render_list(f, panes.list, state, &filtered);
    render_detail(f, panes.detail, app);
}

//
// Status label and colour for a ledger row: revert records, reverted
// changes, then active changes by last drift check.
//

pub fn status_style(c: &ConfigChangeRecord) -> (&'static str, Color) {
    match (c.kind, c.state, c.drift_status) {
        (ChangeKind::Revert, _, _) => ("revert", TERTIARY),
        (_, ChangeState::Reverted, _) => ("reverted", DIM),
        (_, _, Some(DriftStatus::InSync)) | (_, _, None) => ("in sync", OK),
        (_, _, Some(DriftStatus::Superseded)) => ("superseded", MUTED),
        (_, _, Some(DriftStatus::Modified)) => ("modified", WARN),
        (_, _, Some(DriftStatus::Missing)) => ("missing", ERROR),
        (_, _, Some(DriftStatus::Unreachable)) => ("unreachable", DIM),
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn render_list(f: &mut Frame, area: Rect, state: &OperationsState, filtered: &[usize]) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("TIME"),
        Cell::from("NODE"),
        Cell::from("AGENT"),
        Cell::from("STATUS"),
        Cell::from("CHANGE"),
        Cell::from("FILE"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let rows: Vec<Row> = filtered
        .iter()
        .filter_map(|&i| state.changes.entries.get(i))
        .map(|c| {
            let (label, color) = status_style(c);
            Row::new(vec![
                Cell::from(chrome::dot(color)),
                Cell::from(c.created_at.format("%m-%d %H:%M").to_string())
                    .style(Style::default().fg(MUTED)),
                Cell::from(common::short_id(&c.node_id).to_string())
                    .style(Style::default().fg(MUTED)),
                Cell::from(c.agent_short_name.clone().unwrap_or_else(|| "-".into()))
                    .style(Style::default().fg(TEXT)),
                Cell::from(label).style(Style::default().fg(color)),
                Cell::from(c.summary.clone()).style(Style::default().fg(TEXT_BRIGHT)),
                Cell::from(file_name(&c.path).to_string()).style(Style::default().fg(TEXT)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(2),
        Constraint::Length(11),
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(11),
        Constraint::Min(14),
        Constraint::Min(10),
    ];

    if rows.is_empty() {
        let block = focused_panel(!state.detail_focus);
        let inner = block.inner(area);
        f.render_widget(block, area);
        let text = if state.changes.loading {
            " loading\u{2026}"
        } else if state.changes.entries.is_empty() {
            " No config changes recorded yet. Edits made through Praxis (recon ^e, write_file, change apply) appear here."
        } else {
            " No changes match the current filter"
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(text, Style::default().fg(DIM))))
                .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(focused_panel(!state.detail_focus))
        .row_highlight_style(
            Style::default()
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        );
    let mut table_state = TableState::default();
    table_state.select(Some(state.changes.selected.min(filtered.len() - 1)));
    f.render_stateful_widget(table, area, &mut table_state);
}

fn diff_lines(diff: &str) -> Vec<Line<'static>> {
    diff.lines()
        .map(|l| {
            let color = if l.starts_with("+++") || l.starts_with("---") {
                MUTED
            } else if l.starts_with('+') {
                OK
            } else if l.starts_with('-') {
                ERROR
            } else if l.starts_with("@@") {
                TERTIARY
            } else {
                TEXT
            };
            Line::from(Span::styled(l.to_string(), Style::default().fg(color)))
        })
        .collect()
}

fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.operations;
    let block = focused_panel(state.detail_focus);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(c) = app.selected_change() else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No change selected",
                Style::default().fg(DIM),
            ))),
            inner,
        );
        return;
    };

    let mut lines: Vec<Line> = Vec::new();

    //
    // A pending conflict for this change replaces the normal detail with
    // the diff of what changed since Praxis wrote the file.
    //

    if let Some(conflict) = state
        .changes
        .conflict
        .as_ref()
        .filter(|o| o.change_id == c.id)
    {
        lines.push(Line::from(vec![
            chrome::pill("CONFLICT", WARN),
            Span::raw(" "),
            Span::styled(conflict.message.clone(), Style::default().fg(TEXT_BRIGHT)),
        ]));
        lines.push(Line::from(""));
        let mut choices = Vec::new();
        if conflict.structured_undo_available {
            choices.extend(chrome::dim_hint("u", "undo only this change"));
            choices.push(Span::raw("    "));
        }
        choices.extend(chrome::dim_hint("f", "force-restore original"));
        choices.push(Span::raw("    "));
        choices.extend(chrome::dim_hint("esc", "leave as is"));
        lines.push(Line::from(choices));
        lines.push(Line::from(""));
        lines.push(chrome::section_title("Changed since Praxis wrote it", true));
        lines.extend(diff_lines(conflict.diff.as_deref().unwrap_or("")));
    } else {
        let (label, color) = status_style(c);
        lines.push(Line::from(vec![
            chrome::pill(&label.to_uppercase(), color),
            Span::raw(" "),
            Span::styled(
                c.summary.clone(),
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(""));
        lines.push(chrome::kv("id", &c.id));
        lines.push(chrome::kv(
            "time",
            &c.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        ));
        lines.push(chrome::kv("node", &c.node_id));
        lines.push(chrome::kv(
            "agent",
            c.agent_short_name.as_deref().unwrap_or("-"),
        ));
        lines.push(chrome::kv("path", &c.path));
        let file = match &c.config_type {
            Some(t) => format!("{} ({})", c.file_kind.as_str(), t),
            None => c.file_kind.as_str().to_string(),
        };
        lines.push(chrome::kv("file", &file));
        lines.push(chrome::kv("operator", &c.operator));
        lines.push(chrome::kv(
            "engagement",
            c.engagement.as_deref().unwrap_or("-"),
        ));
        if let Some(note) = &c.note {
            lines.push(chrome::kv("note", note));
        }
        lines.push(chrome::kv(
            "before",
            if c.before_exists {
                c.before_sha256.as_deref().unwrap_or("-")
            } else {
                "(file did not exist)"
            },
        ));
        lines.push(chrome::kv(
            "after",
            if c.after_exists {
                c.after_sha256.as_deref().unwrap_or("-")
            } else {
                "(file removed)"
            },
        ));
        if let Some(r) = &c.reverts_change_id {
            lines.push(chrome::kv("reverts", r));
        }
        if let Some(r) = &c.reverted_by {
            let when = c
                .reverted_at
                .map(|t| t.format(" at %Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default();
            lines.push(chrome::kv("reverted by", &format!("{}{}", r, when)));
        }
        if let Some(checked) = c.drift_checked_at {
            lines.push(chrome::kv(
                "drift checked",
                &checked.format("%Y-%m-%d %H:%M:%S").to_string(),
            ));
        }
        lines.push(Line::from(""));
        lines.push(chrome::section_title("Diff (before \u{2192} after)", true));
        match state
            .changes
            .detail
            .as_ref()
            .filter(|d| d.change.id == c.id)
        {
            Some(d) if d.diff.is_empty() => lines.push(Line::from(Span::styled(
                "(no content difference)",
                Style::default().fg(DIM),
            ))),
            Some(d) => lines.extend(diff_lines(&d.diff)),
            None => lines.push(Line::from(Span::styled(
                "loading\u{2026}",
                Style::default().fg(DIM),
            ))),
        }
    }

    f.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .scroll((state.changes.detail_scroll, 0)),
        inner,
    );
}
