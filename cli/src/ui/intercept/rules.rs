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

    let rows: Vec<Row> = app
        .intercept
        .rules
        .iter()
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

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(Style::default().bg(PANEL_HIGHLIGHT_BG));

    let mut state = TableState::default();
    if !app.intercept.rules.is_empty() {
        state.select(Some(
            app.intercept
                .rule_selected
                .min(app.intercept.rules.len() - 1),
        ));
    }
    f.render_stateful_widget(table, area, &mut state);

    if app.intercept.rules.is_empty() {
        let empty = Span::styled(
            "No rules yet — press N to create one.",
            Style::default().fg(MUTED),
        );
        let mut empty_area = area;
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

pub fn hints(_app: &App) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled("n", Style::default().fg(ACCENT)),
        Span::styled(" new  ", Style::default().fg(MUTED)),
        Span::styled("e", Style::default().fg(ACCENT)),
        Span::styled(" edit  ", Style::default().fg(MUTED)),
        Span::styled("d", Style::default().fg(ACCENT)),
        Span::styled(" delete  ", Style::default().fg(MUTED)),
        Span::styled("space", Style::default().fg(ACCENT)),
        Span::styled(" toggle  ", Style::default().fg(MUTED)),
        Span::styled("enter", Style::default().fg(ACCENT)),
        Span::styled(" matches  ", Style::default().fg(MUTED)),
        Span::styled("r", Style::default().fg(ACCENT)),
        Span::styled(" refresh", Style::default().fg(MUTED)),
    ])
}
