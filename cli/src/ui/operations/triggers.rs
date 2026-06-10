use crate::app::OperationsState;
use crate::ui::chrome;
use crate::ui::common::focused_panel;
use crate::ui::theme::{ACCENT, BG_SELECTED, DIM, MUTED, OK, STATUS_FAIL, TEXT_BRIGHT};
use common::{ChainTriggerInfo, ScheduleSpec, TriggerConfig};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

pub(super) fn render_triggers(f: &mut Frame, area: Rect, state: &OperationsState) {
    let pct = state.split_percent.clamp(20, 80);
    let chunks = Layout::horizontal([
        Constraint::Percentage(pct),
        Constraint::Percentage(100 - pct),
    ])
    .split(area);
    render_triggers_list(f, chunks[0], state);
    render_trigger_detail(f, chunks[1], state);
}

fn render_triggers_list(f: &mut Frame, area: Rect, state: &OperationsState) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("CHAIN"),
        Cell::from("TYPE"),
        Cell::from("SUMMARY"),
        Cell::from("NEXT"),
        Cell::from("ON"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let mut rows: Vec<Row> = Vec::new();
    for t in &state.triggers {
        let chain_name = state
            .chain_definitions
            .iter()
            .find(|c| c.id == t.chain_id)
            .map(|c| c.name.as_str())
            .unwrap_or(t.chain_id.as_str());
        let (type_label, summary) = describe_trigger(t, &state.intercept_rules);
        let next = t
            .next_fire_at
            .map(|t| t.format("%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".to_string());

        let on_cell = if t.enabled {
            chrome::dot(OK)
        } else {
            chrome::dot(DIM)
        };

        rows.push(Row::new(vec![
            Cell::from(Span::styled(
                " T ",
                Style::default()
                    .fg(crate::ui::theme::BG)
                    .bg(super::CHAIN_COLOR)
                    .add_modifier(Modifier::BOLD),
            )),
            Cell::from(chain_name.to_string()).style(Style::default().fg(TEXT_BRIGHT)),
            Cell::from(type_label).style(Style::default().fg(MUTED)),
            Cell::from(summary).style(Style::default().fg(DIM)),
            Cell::from(next).style(Style::default().fg(MUTED)),
            Cell::from(on_cell),
        ]));
    }

    let widths = [
        Constraint::Length(3),
        Constraint::Min(12),
        Constraint::Length(10),
        Constraint::Min(14),
        Constraint::Length(12),
        Constraint::Length(3),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(focused_panel(false))
        .row_highlight_style(
            Style::default()
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        );

    let mut table_state = TableState::default();
    table_state.select(Some(state.trigger_selected));

    f.render_stateful_widget(table, area, &mut table_state);
}

fn render_trigger_detail(f: &mut Frame, area: Rect, state: &OperationsState) {
    let block = focused_panel(false);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(trigger) = state.triggers.get(state.trigger_selected) else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No trigger selected",
                Style::default().fg(DIM),
            ))),
            inner,
        );
        return;
    };

    let chain_name = state
        .chain_definitions
        .iter()
        .find(|c| c.id == trigger.chain_id)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| trigger.chain_id.clone());

    let (type_label, summary) = describe_trigger(trigger, &state.intercept_rules);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        chrome::pill("CHAIN", super::CHAIN_COLOR),
        Span::raw(" "),
        Span::styled(
            chain_name,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(chrome::kv("type", &type_label));
    lines.push(chrome::kv("config", &summary));
    lines.push(Line::from(vec![
        Span::styled("enabled: ", Style::default().fg(MUTED)),
        if trigger.enabled {
            chrome::pill("ON", OK)
        } else {
            Span::styled(
                " OFF ",
                Style::default()
                    .fg(STATUS_FAIL)
                    .bg(crate::ui::theme::BG_ELEMENT),
            )
        },
    ]));

    if let Some(last) = trigger.last_fired_at {
        lines.push(chrome::kv(
            "last fired",
            &last.format("%Y-%m-%d %H:%M:%S").to_string(),
        ));
    }
    if let Some(next) = trigger.next_fire_at {
        lines.push(chrome::kv(
            "next fire",
            &next.format("%Y-%m-%d %H:%M:%S").to_string(),
        ));
    }

    lines.push(Line::from(""));
    lines.push(chrome::section_title("Target", true));

    let spec = &trigger.target_spec;
    let nodes_txt = if spec.node_ids.is_empty() {
        "(all nodes)".to_string()
    } else {
        spec.node_ids
            .iter()
            .map(|id| id.chars().take(8).collect::<String>())
            .collect::<Vec<_>>()
            .join(", ")
    };
    lines.push(chrome::kv("nodes", &nodes_txt));
    if let Some(ref os) = spec.os_filter
        && !os.is_empty()
    {
        lines.push(chrome::kv("os filter", os));
    }
    let agents_txt = if spec.agent_short_names.is_empty() {
        "(all agents)".to_string()
    } else {
        spec.agent_short_names.join(", ")
    };
    lines.push(chrome::kv("agents", &agents_txt));
    if spec.include_triggering_node {
        lines.push(chrome::kv("include triggering", "yes"));
    }
    let _ = ACCENT;

    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

//
// Human-readable type label and config summary for a trigger. Returned as
// owned strings to keep call sites simple.
//
pub fn describe_trigger(t: &ChainTriggerInfo, rules: &[common::InterceptRule]) -> (String, String) {
    match &t.trigger_config {
        TriggerConfig::Scheduled {
            schedule,
            recurring,
        } => {
            let sched_text = match schedule {
                ScheduleSpec::DailyAt { hour, minute } => {
                    format!("daily @ {:02}:{:02}", hour, minute)
                }
                ScheduleSpec::Interval { minutes } => {
                    format!("every {}m", minutes)
                }
            };
            let suffix = if *recurring { "" } else { " (once)" };
            ("Scheduled".to_string(), format!("{}{}", sched_text, suffix))
        }
        TriggerConfig::InterceptMatch { rule_id } => {
            let name = rules
                .iter()
                .find(|r| r.id == *rule_id)
                .map(|r| r.name.clone())
                .unwrap_or_else(|| format!("rule #{}", rule_id));
            ("Intercept".to_string(), format!("match: {}", name))
        }
        TriggerConfig::NewNode => ("NewNode".to_string(), "new node connected".to_string()),
    }
}
