//
// Rules tab: list of intercept rules on the left, selected-rule detail
// on the right (same chrome as Traffic / Matches).
//

use chrono::Local;
use common::{InterceptRule, RuleScope, TargetDirection};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

use crate::app::App;
use crate::ui::chrome;
use crate::ui::common::focused_titled_panel;
use crate::ui::intercept::search_bar;
use crate::ui::theme::{ACCENT, BG_SELECTED, DIM, MUTED, OK, STATUS_FAIL, TEXT, TEXT_BRIGHT};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);

    search_bar::render(f, chunks[0], app, &[]);

    let pct = app.intercept.rule_split_percent.clamp(20, 80);
    let split = Layout::horizontal([
        Constraint::Percentage(pct),
        Constraint::Percentage(100 - pct),
    ])
    .split(chunks[1]);
    render_list(f, split[0], app);
    render_detail(f, split[1], app);
}

fn render_list(f: &mut Frame, area: Rect, app: &App) {
    let block = focused_titled_panel(" Intercept rules ", !app.intercept.rule_detail_focus);

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
            let dir = direction_label(&rule.target_direction);
            let scope = scope_short(&rule.scope);
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
    f.render_stateful_widget(table, area, &mut state);

    if app.intercept.rules.is_empty() {
        let empty = Span::styled(
            "  No rules yet — press ^n to create one.",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        );
        let mut empty_area = area;
        empty_area.y += 3;
        empty_area.x += 3;
        empty_area.height = 1;
        f.render_widget(
            Paragraph::new(Line::from(empty)),
            empty_area,
        );
    }
}

fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let title = match app.intercept.selected_rule() {
        Some(rule) => format!(" Rule: {} ", rule.name),
        None => " Rule detail ".to_string(),
    };
    let block = focused_titled_panel(&title, app.intercept.rule_detail_focus);

    let Some(rule) = app.intercept.selected_rule() else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No rule selected.",
                Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
            )))
            .block(block),
            area,
        );
        return;
    };

    let lines = detail_lines(app, rule);
    let inner_h = block.inner(area).height as usize;
    let max_scroll = lines.len().saturating_sub(inner_h) as u16;
    app.intercept.rule_detail_max_scroll.set(max_scroll);
    let effective = app.intercept.rule_detail_scroll.min(max_scroll);
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((effective, 0))
        .block(block);
    f.render_widget(para, area);
}

fn detail_lines(app: &App, rule: &InterceptRule) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    let (enabled_label, enabled_color) = if rule.enabled {
        ("enabled", OK)
    } else {
        ("disabled", STATUS_FAIL)
    };
    out.push(Line::from(vec![
        Span::styled(
            rule.name.clone(),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            enabled_label,
            Style::default()
                .fg(enabled_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    out.push(Line::raw(""));

    out.push(kv_line("id", &rule.id.to_string()));
    out.push(kv_line("pattern", &rule.regex_pattern));
    out.push(kv_line("direction", direction_label(&rule.target_direction)));
    out.push(kv_line("scope", &scope_full(&rule.scope)));

    let match_count = app.intercept.match_count_for_rule(rule.id);
    out.push(Line::from(vec![
        Span::styled("matches: ", Style::default().fg(MUTED)),
        Span::styled(
            match_count.to_string(),
            Style::default().fg(if match_count > 0 { ACCENT } else { DIM }),
        ),
        Span::styled(
            "    (enter) open in Matches",
            Style::default().fg(DIM),
        ),
    ]));
    out.push(Line::raw(""));

    out.push(section_heading("LLM SUMMARY"));
    match rule.summarization_prompt.as_deref() {
        Some(prompt) if !prompt.is_empty() => {
            for line in prompt.lines() {
                out.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(TEXT),
                )));
            }
        }
        _ => {
            out.push(Line::from(Span::styled(
                "(none — edit rule to add a summarisation prompt)",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )));
        }
    }
    out.push(Line::raw(""));

    out.push(section_heading("TIMESTAMPS"));
    out.push(kv_line(
        "created",
        &rule
            .created_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    ));
    out.push(kv_line(
        "updated",
        &rule
            .updated_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    ));

    out
}

fn direction_label(dir: &TargetDirection) -> &'static str {
    match dir {
        TargetDirection::Send => "send",
        TargetDirection::Receive => "recv",
        TargetDirection::Both => "both",
    }
}

fn scope_short(scope: &RuleScope) -> String {
    match scope {
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
    }
}

fn scope_full(scope: &RuleScope) -> String {
    match scope {
        RuleScope::All => "all nodes".to_string(),
        RuleScope::Node { node_id } => format!("node {node_id}"),
        RuleScope::Agent {
            node_id,
            agent_short_name,
        } => format!("agent {agent_short_name} on {node_id}"),
    }
}

fn kv_line(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key}: "), Style::default().fg(MUTED)),
        Span::styled(value.to_string(), Style::default().fg(TEXT_BRIGHT)),
    ])
}

fn section_heading(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    ))
}
