use crate::app::OperationsState;
use crate::ui::common::titled_panel;
use crate::ui::theme::{ACCENT, DIM, MUTED, PANEL_HIGHLIGHT_BG, STATUS_DONE, STATUS_FAIL, TEXT};
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
        Cell::from("Chain"),
        Cell::from("Type"),
        Cell::from("Summary"),
        Cell::from("Next"),
        Cell::from("Enabled"),
    ])
    .style(Style::default().fg(ACCENT));

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
        let (enabled_str, enabled_color) = if t.enabled {
            ("ON", STATUS_DONE)
        } else {
            ("OFF", STATUS_FAIL)
        };

        rows.push(Row::new(vec![
            Cell::from("T").style(Style::default().fg(super::CHAIN_COLOR)),
            Cell::from(chain_name.to_string()).style(Style::default().fg(TEXT)),
            Cell::from(type_label).style(Style::default().fg(MUTED)),
            Cell::from(summary).style(Style::default().fg(DIM)),
            Cell::from(next).style(Style::default().fg(DIM)),
            Cell::from(enabled_str).style(Style::default().fg(enabled_color)),
        ]));
    }

    let widths = [
        Constraint::Length(1),
        Constraint::Min(12),
        Constraint::Length(10),
        Constraint::Min(14),
        Constraint::Length(12),
        Constraint::Length(7),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(titled_panel(" Triggers "))
        .row_highlight_style(Style::default().bg(PANEL_HIGHLIGHT_BG));

    let mut table_state = TableState::default();
    table_state.select(Some(state.trigger_selected));

    f.render_stateful_widget(table, area, &mut table_state);
}

fn render_trigger_detail(f: &mut Frame, area: Rect, state: &OperationsState) {
    let block = titled_panel(" Detail ");
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
    lines.push(Line::from(Span::styled(
        format!(" {}", chain_name),
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(kv(" Type", type_label));
    lines.push(kv(" Config", summary));
    lines.push(kv(
        " Enabled",
        if trigger.enabled { "yes" } else { "no" }.to_string(),
    ));

    if let Some(last) = trigger.last_fired_at {
        lines.push(kv(
            " Last fired",
            last.format("%Y-%m-%d %H:%M:%S").to_string(),
        ));
    }
    if let Some(next) = trigger.next_fire_at {
        lines.push(kv(
            " Next fire",
            next.format("%Y-%m-%d %H:%M:%S").to_string(),
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Target",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )));

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
    lines.push(kv(" Nodes", nodes_txt));
    if let Some(ref os) = spec.os_filter
        && !os.is_empty()
    {
        lines.push(kv(" OS filter", os.clone()));
    }
    let agents_txt = if spec.agent_short_names.is_empty() {
        "(all agents)".to_string()
    } else {
        spec.agent_short_names.join(", ")
    };
    lines.push(kv(" Agents", agents_txt));
    if spec.include_triggering_node {
        lines.push(kv(" Include triggering", "yes".to_string()));
    }

    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn kv(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{}: ", label), Style::default().fg(DIM)),
        Span::styled(value, Style::default().fg(TEXT)),
    ])
}

//
// Human-readable type label and config summary for a trigger. Returned as
// owned strings to keep call sites simple.
//
pub fn describe_trigger(
    t: &ChainTriggerInfo,
    rules: &[common::InterceptRule],
) -> (String, String) {
    match &t.trigger_config {
        TriggerConfig::Scheduled { schedule, recurring } => {
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
