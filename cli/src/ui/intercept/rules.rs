//
// Rules tab: list of intercept rules with name, pattern, direction,
// scope, enabled, and match counts.
//

use common::{RuleScope, TargetDirection};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row, Table, TableState};

use crate::app::App;
use crate::ui::chrome;
use crate::ui::common::titled_panel;
use crate::ui::intercept::{hints as shared_hints, search_bar};
use crate::ui::theme::{ACCENT, BG_SELECTED, DIM, MUTED, OK, TEXT_BRIGHT};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    use ratatui::layout::Layout;
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);

    search_bar::render(f, chunks[0], app, &[]);

    let block = titled_panel(" Intercept rules ");

    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("Name"),
        Cell::from("Pattern"),
        Cell::from("Dir"),
        Cell::from("Scope"),
        Cell::from("Mtch"),
        Cell::from("Sum"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));

    let widths = [
        Constraint::Length(2),
        Constraint::Length(18),
        Constraint::Min(16),
        Constraint::Length(5),
        Constraint::Length(16),
        Constraint::Length(5),
        Constraint::Length(4),
    ];

    let filtered_ids = app.intercept.filtered_rule_ids();
    let rows: Vec<Row> = filtered_ids
        .iter()
        .filter_map(|id| app.intercept.rules.iter().find(|r| r.id == *id))
        .map(|rule| {
            let on_cell = if rule.enabled {
                chrome::dot(OK)
            } else {
                chrome::dot(DIM)
            };
            let dir = match rule.target_direction {
                TargetDirection::Send => "send",
                TargetDirection::Receive => "recv",
                TargetDirection::Both => "both",
            };
            let scope = match &rule.scope {
                RuleScope::All => "all".to_string(),
                RuleScope::Node { node_id } => {
                    format!("node:{}", &node_id[..8.min(node_id.len())])
                }
                RuleScope::Agent {
                    node_id,
                    agent_short_name,
                } => format!(
                    "agent:{}/{}",
                    &node_id[..8.min(node_id.len())],
                    agent_short_name
                ),
            };
            let match_count = app.intercept.match_count_for_rule(rule.id);
            let summ = if rule.summarization_prompt.is_some() {
                Span::styled(
                    "\u{2713}",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("\u{00b7}", Style::default().fg(DIM))
            };

            Row::new(vec![
                Cell::from(on_cell),
                Cell::from(Span::styled(
                    rule.name.clone(),
                    Style::default().fg(TEXT_BRIGHT),
                )),
                Cell::from(Span::styled(
                    rule.regex_pattern.clone(),
                    Style::default().fg(MUTED),
                )),
                Cell::from(Span::styled(dir.to_string(), Style::default().fg(MUTED))),
                Cell::from(Span::styled(scope, Style::default().fg(DIM))),
                Cell::from(Span::styled(
                    match_count.to_string(),
                    Style::default().fg(if match_count > 0 { ACCENT } else { DIM }),
                )),
                Cell::from(summ),
            ])
        })
        .collect();

    let row_count = rows.len();
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = TableState::default();
    if row_count > 0 {
        state.select(Some(
            app.intercept
                .selected_rule_filtered_index()
                .min(row_count - 1),
        ));
    }
    f.render_stateful_widget(table, chunks[1], &mut state);

    if app.intercept.rules.is_empty() {
        let empty = Span::styled(
            "  No rules yet — press ^n to create one.",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        );
        let mut empty_area = chunks[1];
        empty_area.y += 3;
        empty_area.x += 3;
        empty_area.height = 1;
        f.render_widget(
            ratatui::widgets::Paragraph::new(Line::from(empty)),
            empty_area,
        );
    }
}

pub fn hints(_app: &App) -> Line<'static> {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    shared_hints::line_with_tier(vec![
        Span::styled("^n", key),
        Span::styled(" new", label),
        Span::raw("    "),
        Span::styled("^e", key),
        Span::styled(" edit", label),
        Span::raw("    "),
        Span::styled("^u", key),
        Span::styled(" dup", label),
        Span::raw("    "),
        Span::styled("^d", key),
        Span::styled(" del", label),
        Span::raw("    "),
        Span::styled("space", key),
        Span::styled(" toggle", label),
        Span::raw("    "),
        Span::styled("\u{21b5}", key),
        Span::styled(" matches", label),
        Span::raw("    "),
        Span::styled("r", key),
        Span::styled(" refresh", label),
    ])
}