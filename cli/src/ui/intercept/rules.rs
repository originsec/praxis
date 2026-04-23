//
// Rules tab: list of intercept rules with name, pattern, direction,
// scope, enabled. Form opens via the `n`/`e` keybindings (handled
// elsewhere; rendered by ui/intercept/form.rs).
//

use common::{RuleScope, TargetDirection};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};

use crate::app::App;
use crate::ui::theme::{
    ACCENT, DIM, INPUT_BORDER, MUTED, PANEL_HIGHLIGHT_BG, STATUS_DONE, STATUS_FAIL, TEXT,
};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    use ratatui::layout::Layout;
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);

    //
    // Filter bar.
    //
    let filter_span = if app.intercept.rule_filter_focused {
        let v = if app.intercept.rule_filter.is_empty() {
            "_".to_string()
        } else {
            format!("{}_", app.intercept.rule_filter)
        };
        Span::styled(v, Style::default().fg(ACCENT))
    } else if app.intercept.rule_filter.is_empty() {
        Span::styled("(/ to filter)", Style::default().fg(DIM))
    } else {
        Span::styled(app.intercept.rule_filter.clone(), Style::default().fg(ACCENT))
    };
    let filter_line = Line::from(vec![
        Span::styled(" /", Style::default().fg(DIM)),
        Span::styled(" filter: ", Style::default().fg(MUTED)),
        filter_span,
    ]);
    f.render_widget(ratatui::widgets::Paragraph::new(filter_line), chunks[0]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(INPUT_BORDER))
        .title(Span::styled(" Intercept rules ", Style::default().fg(MUTED)));

    let header = Row::new(vec![
        Cell::from(Span::styled("On", Style::default().fg(ACCENT))),
        Cell::from(Span::styled("Name", Style::default().fg(ACCENT))),
        Cell::from(Span::styled("Pattern", Style::default().fg(ACCENT))),
        Cell::from(Span::styled("Dir", Style::default().fg(ACCENT))),
        Cell::from(Span::styled("Scope", Style::default().fg(ACCENT))),
        Cell::from(Span::styled("Summ", Style::default().fg(ACCENT))),
    ]);

    let widths = [
        Constraint::Length(3),
        Constraint::Length(20),
        Constraint::Min(20),
        Constraint::Length(5),
        Constraint::Length(18),
        Constraint::Length(5),
    ];

    let filter = app.intercept.rule_filter.to_lowercase();
    let rows: Vec<Row> = app
        .intercept
        .rules
        .iter()
        .filter(|rule| {
            filter.is_empty()
                || rule.name.to_lowercase().contains(&filter)
                || rule.regex_pattern.to_lowercase().contains(&filter)
        })
        .map(|rule| {
            let on_cell = if rule.enabled {
                Span::styled("\u{25cf}", Style::default().fg(STATUS_DONE))
            } else {
                Span::styled("\u{25cb}", Style::default().fg(DIM))
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
            let summ = if rule.summarization_prompt.is_some() {
                Span::styled(
                    "\u{2713}",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("·", Style::default().fg(DIM))
            };

            Row::new(vec![
                Cell::from(on_cell),
                Cell::from(Span::styled(rule.name.clone(), Style::default().fg(TEXT))),
                Cell::from(Span::styled(
                    rule.regex_pattern.clone(),
                    Style::default().fg(MUTED),
                )),
                Cell::from(Span::styled(dir.to_string(), Style::default().fg(MUTED))),
                Cell::from(Span::styled(scope, Style::default().fg(MUTED))),
                Cell::from(summ),
            ])
        })
        .collect();

    let row_count = rows.len();
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(Style::default().bg(PANEL_HIGHLIGHT_BG));

    let mut state = TableState::default();
    if row_count > 0 {
        state.select(Some(app.intercept.rule_selected.min(row_count - 1)));
    }
    f.render_stateful_widget(table, chunks[1], &mut state);

    if app.intercept.rules.is_empty() {
        let empty = Span::styled(
            "No rules yet — press ^n to create one.",
            Style::default().fg(MUTED),
        );
        let mut empty_area = chunks[1];
        empty_area.y += 2;
        empty_area.x += 3;
        empty_area.height = 1;
        f.render_widget(
            ratatui::widgets::Paragraph::new(Line::from(empty)),
            empty_area,
        );
    }

    let _ = STATUS_FAIL;
}

pub fn hints(app: &App) -> Line<'static> {
    let mut spans = vec![
        Span::raw(" "),
        Span::styled("^n", Style::default().fg(ACCENT)),
        Span::styled(" new  ", Style::default().fg(MUTED)),
        Span::styled("^e", Style::default().fg(ACCENT)),
        Span::styled(" edit  ", Style::default().fg(MUTED)),
        Span::styled("^d", Style::default().fg(ACCENT)),
        Span::styled(" delete  ", Style::default().fg(MUTED)),
        Span::styled("space", Style::default().fg(ACCENT)),
        Span::styled(" toggle  ", Style::default().fg(MUTED)),
        Span::styled("enter", Style::default().fg(ACCENT)),
        Span::styled(" matches  ", Style::default().fg(MUTED)),
    ];
    if !app.intercept.rule_filter.is_empty() {
        spans.push(Span::styled("filter: ", Style::default().fg(DIM)));
        spans.push(Span::styled(
            app.intercept.rule_filter.clone(),
            Style::default().fg(ACCENT),
        ));
        spans.push(Span::styled("  esc clear", Style::default().fg(DIM)));
    } else {
        spans.push(Span::styled("/ to filter", Style::default().fg(DIM)));
    }
    Line::from(spans)
}
