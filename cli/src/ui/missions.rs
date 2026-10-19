use crate::app::App;
use crate::app::missions::{
    MissionField, MissionFieldKind, MissionFocus, MissionForm, MissionFormKind, MissionTab,
    MissionsState, status_label,
};
use crate::ui::hits::MouseAction;
use crate::ui::theme::{
    ACCENT, BG, BG_ELEMENT, BG_PANEL, BG_SELECTED, BORDER_SUBTLE, DIM, ERROR, MUTED, SECONDARY,
    STATUS_DONE, STATUS_QUEUED, STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};
use crate::ui::{chrome, filter_bar, hint_row};
use common::missions::{Mission, MissionStatus};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, Paragraph, Wrap};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    render_state(f, area, &app.missions, Some(app));
}

fn render_state(f: &mut Frame, area: Rect, state: &MissionsState, app: Option<&App>) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    f.render_widget(Block::default().style(Style::default().bg(BG)), area);
    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    let active = state
        .missions
        .iter()
        .filter(|mission| mission.status == MissionStatus::Running)
        .count();
    let review = state
        .missions
        .iter()
        .filter(|mission| {
            matches!(
                mission.status,
                MissionStatus::NeedsReview
                    | MissionStatus::Interrupted
                    | MissionStatus::BudgetExceeded
            )
        })
        .count();
    let mut title = vec![Span::styled(
        " Missions ",
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )];
    title.push(chrome::mid_dot());
    title.push(Span::styled(
        format!("{active} running"),
        Style::default().fg(STATUS_RUNNING),
    ));
    if area.width >= 50 {
        title.push(chrome::mid_dot());
        title.push(Span::styled(
            format!("{review} to review"),
            Style::default().fg(SECONDARY),
        ));
    }
    if state.loading() {
        title.push(Span::styled("   Updating…", Style::default().fg(MUTED)));
    }
    f.render_widget(Paragraph::new(Line::from(title)), chunks[0]);
    let background_app = if state.form.is_none() { app } else { None };
    filter_bar::render(
        f,
        chunks[1],
        &filter_bar::FilterBarModel {
            focused: state.focus == MissionFocus::Filter,
            query: &state.filter,
            placeholder: "filter missions by name, status or objective",
            extra_pills: vec![],
            meta: (area.width >= 90).then(|| format!("{} missions", state.visible_indices().len())),
        },
    );
    if state.focus == MissionFocus::Filter {
        let field = MissionField {
            label: "Filter",
            value: state.filter.clone(),
            cursor: state.filter_cursor,
            kind: MissionFieldKind::Text,
            hint: "",
        };
        if let Some(mut line) = field_lines(&field, chunks[1].width.saturating_sub(2).max(1), true)
            .into_iter()
            .next()
        {
            line.spans[0] = Span::styled("/ ", Style::default().fg(TEXT_BRIGHT));
            f.render_widget(Paragraph::new(line), row(chunks[1], 0));
        }
    }
    register(
        background_app,
        row(chunks[1], 0),
        MouseAction::MissionFocus(MissionFocus::Filter),
    );

    if chunks[2].width >= 96 {
        let panes = Layout::horizontal([
            Constraint::Length((chunks[2].width / 3).clamp(30, 42)),
            Constraint::Min(1),
        ])
        .split(chunks[2]);
        render_list(f, panes[0], state, background_app);
        render_detail(f, panes[1], state, background_app);
    } else if state.focus == MissionFocus::Detail {
        render_detail(f, chunks[2], state, background_app);
    } else {
        render_list(f, chunks[2], state, background_app);
    }
    let message = state
        .error
        .as_ref()
        .map(|text| (text.as_str(), ERROR))
        .or_else(|| state.status.as_ref().map(|text| (text.as_str(), MUTED)));
    if let Some((message, color)) = message {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                message,
                Style::default().fg(color),
            ))),
            chunks[3],
        );
    } else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                if state.focus == MissionFocus::Detail {
                    " ↑↓ scroll   ←→ tabs   / filter   F5 refresh"
                } else {
                    " ↑↓ select   enter inspect   / filter   F5 refresh"
                },
                Style::default().fg(DIM),
            ))),
            chunks[3],
        );
    }
    let mut hints = vec![hint_row::HintItem::with_action(
        "Ctrl+N",
        "new",
        MouseAction::MissionKey(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)),
    )];
    if let Some(mission) = state.selected_mission() {
        match mission.status {
            MissionStatus::Running => hints.push(hint("p", "pause", KeyCode::Char('p'))),
            MissionStatus::Completed | MissionStatus::Cancelled => {}
            _ => hints.push(hint("r", "resume", KeyCode::Char('r'))),
        }
        if !matches!(
            mission.status,
            MissionStatus::Completed | MissionStatus::Cancelled
        ) {
            if area.width >= 68 {
                hints.push(hint("s", "steer", KeyCode::Char('s')));
            }
            hints.push(hint("x", "cancel", KeyCode::Char('x')));
        }
        hints.push(hint(
            "Tab",
            if state.focus == MissionFocus::Detail {
                "list"
            } else {
                "detail"
            },
            KeyCode::Tab,
        ));
    }
    if area.width >= 110 || state.selected_mission().is_none() {
        hints.extend([
            hint("/", "filter", KeyCode::Char('/')),
            hint("F5", "refresh", KeyCode::F(5)),
        ]);
    }
    hint_row::render(f, chunks[4], &hints, background_app);
    if let Some(form) = &state.form {
        render_form(f, area, form, state.selected_mission(), app);
    }
}

fn register(app: Option<&App>, area: Rect, action: MouseAction) {
    if area.width > 0 && area.height > 0 {
        if let Some(app) = app {
            app.hits_register(area, action);
        }
    }
}

fn hint(key: &str, label: &str, code: KeyCode) -> hint_row::HintItem {
    hint_row::HintItem::with_action(
        key,
        label,
        MouseAction::MissionKey(KeyEvent::new(code, KeyModifiers::NONE)),
    )
}

fn row(area: Rect, offset: u16) -> Rect {
    Rect::new(
        area.x,
        area.y.saturating_add(offset),
        area.width,
        u16::from(offset < area.height),
    )
}

fn status(status: MissionStatus) -> (&'static str, Color) {
    let color = match status {
        MissionStatus::Running => STATUS_RUNNING,
        MissionStatus::Paused => STATUS_QUEUED,
        MissionStatus::Interrupted | MissionStatus::NeedsReview | MissionStatus::BudgetExceeded => {
            SECONDARY
        }
        MissionStatus::Completed => STATUS_DONE,
        MissionStatus::Cancelled => MUTED,
        MissionStatus::Failed => ERROR,
    };
    (status_label(status), color)
}

fn compact_number(number: u64) -> String {
    if number >= 1_000_000 {
        format!("{:.1}m", number as f64 / 1_000_000.0)
    } else if number >= 10_000 {
        format!("{:.0}k", number as f64 / 1_000.0)
    } else {
        number.to_string()
    }
}

fn duration(seconds: u64) -> String {
    if seconds >= 3600 {
        format!("{}h{}m", seconds / 3600, seconds % 3600 / 60)
    } else if seconds >= 60 {
        format!("{}m{}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn render_list(f: &mut Frame, area: Rect, state: &MissionsState, app: Option<&App>) {
    let focused = state.focus == MissionFocus::List;
    f.render_widget(Block::default().style(Style::default().bg(BG_PANEL)), area);
    register(app, area, MouseAction::MissionFocus(MissionFocus::List));
    let inner = area.inner(ratatui::layout::Margin::new(1, 0));
    f.render_widget(
        Paragraph::new(chrome::section_title("MISSIONS", focused)),
        row(inner, 0),
    );
    let body = Rect::new(
        inner.x,
        inner.y.saturating_add(2),
        inner.width,
        inner.height.saturating_sub(2),
    );
    let visible = state.visible_indices();
    if visible.is_empty() {
        let (title, detail) = if !state.loaded && state.loading() {
            ("Loading missions…", "Fetching saved work from the service.")
        } else if !state.filter.is_empty() {
            (
                "No matching missions",
                "Change the filter to see other work.",
            )
        } else {
            (
                "Give an objective a home",
                "Press Ctrl+N to create a mission.\nSet its scope and budget, then follow its progress here.",
            )
        };
        empty(f, body, title, detail);
        return;
    }
    let height = if body.height >= 6 { 3 } else { 1 };
    let capacity = (body.height / height).max(1) as usize;
    let selected = state.selected.min(visible.len() - 1);
    let start = selected.saturating_sub(capacity.saturating_sub(1));
    for (offset, index) in visible.iter().skip(start).take(capacity).enumerate() {
        let mission = &state.missions[*index];
        let selected_row = start + offset == selected;
        let card = Rect::new(
            body.x,
            body.y + offset as u16 * height,
            body.width,
            height.min(body.height.saturating_sub(offset as u16 * height)),
        );
        let background = if selected_row { BG_SELECTED } else { BG_PANEL };
        f.render_widget(
            Block::default().style(Style::default().bg(background)),
            card,
        );
        let (label, color) = status(mission.status);
        let marker = if selected_row { "▸ " } else { "  " };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    marker,
                    Style::default().fg(if focused { ACCENT } else { MUTED }),
                ),
                Span::styled(
                    mission.spec.name.clone(),
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(if selected_row {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ]))
            .style(Style::default().bg(background)),
            row(card, 0),
        );
        if height > 1 {
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw("  "),
                    chrome::dot(color),
                    Span::styled(format!(" {label}"), Style::default().fg(color)),
                    chrome::mid_dot(),
                    Span::styled(
                        common::truncate_str(&mission.id, 8).to_string(),
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(
                        format!("   {} steps", mission.iterations),
                        Style::default().fg(MUTED),
                    ),
                ]))
                .style(Style::default().bg(background)),
                row(card, 1),
            );
        }
        register(app, card, MouseAction::MissionRow(mission.id.clone()));
    }
}

fn render_detail(f: &mut Frame, area: Rect, state: &MissionsState, app: Option<&App>) {
    register(app, area, MouseAction::MissionFocus(MissionFocus::Detail));
    let area = area.inner(ratatui::layout::Margin::new(2, 0));
    let Some(mission) = state.selected_mission() else {
        empty(
            f,
            area,
            "A clear view of ongoing work",
            "Select a mission to inspect its objective, budgets, checkpoints and evidence.",
        );
        return;
    };
    if let Some(app) = app {
        app.text_selection_region_register(area);
    }
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .split(area);
    let (label, color) = status(mission.status);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            chrome::pill(label, color),
            Span::styled(
                format!("  {}", mission.spec.name),
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(mission.id.clone(), Style::default().fg(MUTED)),
            chrome::mid_dot(),
            Span::styled(
                mission
                    .spec
                    .model
                    .clone()
                    .unwrap_or_else(|| "Default model".into()),
                Style::default().fg(MUTED),
            ),
        ])),
        chunks[1],
    );
    let tabs = [
        (MissionTab::Overview, "1 Overview"),
        (MissionTab::Evidence, "2 Evidence"),
        (MissionTab::Events, "3 Events"),
    ];
    let mut spans = Vec::new();
    let mut x = chunks[2].x;
    for (tab, label) in tabs {
        let width = chrome::tab_width(label, None).min(chunks[2].right().saturating_sub(x));
        register(
            app,
            Rect::new(x, chunks[2].y, width, chunks[2].height),
            MouseAction::MissionTab(tab),
        );
        spans.extend(chrome::tab(label, None, state.tab == tab));
        spans.push(Span::raw(" "));
        x = x.saturating_add(width + 1);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), chunks[2]);
    f.render_widget(
        Paragraph::new("─".repeat(chunks[3].width as usize))
            .style(Style::default().fg(BORDER_SUBTLE)),
        chunks[3],
    );
    render_budgets(f, chunks[4], mission);
    let lines = match state.tab {
        MissionTab::Overview => overview_lines(mission),
        MissionTab::Evidence => evidence_lines(state),
        MissionTab::Events => event_lines(state),
    };
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(TEXT))
        .wrap(Wrap { trim: false });
    let max_scroll = paragraph
        .line_count(chunks[5].width)
        .saturating_sub(chunks[5].height as usize);
    f.render_widget(
        paragraph.scroll((
            state
                .detail_scroll
                .min(max_scroll.min(u16::MAX as usize) as u16),
            0,
        )),
        chunks[5],
    );
}

fn render_budgets(f: &mut Frame, area: Rect, mission: &Mission) {
    let columns = Layout::horizontal([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .split(area);
    let values = [
        (
            "Steps",
            mission.iterations as u64,
            mission.spec.budget.max_iterations as u64,
            format!(
                "{} / {}",
                mission.iterations, mission.spec.budget.max_iterations
            ),
        ),
        (
            "Tokens",
            mission.tokens_used,
            mission.spec.budget.max_tokens,
            format!(
                "{} / {}",
                compact_number(mission.tokens_used),
                compact_number(mission.spec.budget.max_tokens)
            ),
        ),
        (
            "Time",
            mission.active_seconds,
            mission.spec.budget.max_active_seconds,
            format!(
                "{} / {}",
                duration(mission.active_seconds),
                duration(mission.spec.budget.max_active_seconds)
            ),
        ),
    ];
    for (column, (label, used, limit, value)) in columns.iter().zip(values) {
        let cell = Rect::new(
            column.x,
            column.y,
            column.width.saturating_sub(2),
            column.height,
        );
        f.render_widget(
            Paragraph::new(label).style(Style::default().fg(MUTED)),
            row(cell, 0),
        );
        let value = if cell.width < 18 {
            value.replace(" / ", "/")
        } else {
            value
        };
        f.render_widget(
            Paragraph::new(value).style(Style::default().fg(TEXT_BRIGHT)),
            row(cell, 1),
        );
        let ratio = if limit == 0 {
            1.0
        } else {
            (used as f64 / limit as f64).clamp(0.0, 1.0)
        };
        f.render_widget(
            Gauge::default().ratio(ratio).label("").gauge_style(
                Style::default()
                    .fg(if ratio >= 0.9 { SECONDARY } else { ACCENT })
                    .bg(BG_ELEMENT),
            ),
            row(cell, 2),
        );
    }
}

fn section(lines: &mut Vec<Line<'static>>, title: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    lines.push(chrome::section_title(title, false));
    for line in common::truncate_str(value, 32_000).lines() {
        lines.push(Line::from(line.to_string()));
    }
    if value.len() > 32_000 {
        lines.push(Line::styled("[Excerpt shown]", Style::default().fg(MUTED)));
    }
    lines.push(Line::raw(""));
}

fn overview_lines(mission: &Mission) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if !mission.pending_tools.is_empty() {
        lines.push(Line::styled(
            format!(
                "{} actions need outcome review before resuming",
                mission.pending_tools.len()
            ),
            Style::default().fg(SECONDARY).add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::from(mission.pending_tools.join(", ")));
        lines.push(Line::raw(""));
    }
    section(&mut lines, "OBJECTIVE", &mission.spec.objective);
    section(
        &mut lines,
        "SCOPE",
        if mission.spec.scope.is_empty() {
            "No additional scope specified"
        } else {
            &mission.spec.scope
        },
    );
    section(
        &mut lines,
        "CHECKPOINT",
        if mission.checkpoint.summary.is_empty() {
            "No checkpoint yet. Findings appear as the mission progresses."
        } else {
            &mission.checkpoint.summary
        },
    );
    if !mission.checkpoint.findings.is_empty() {
        section(
            &mut lines,
            "FINDINGS",
            &mission
                .checkpoint
                .findings
                .iter()
                .map(|line| format!("• {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !mission.checkpoint.outstanding.is_empty() {
        section(
            &mut lines,
            "OUTSTANDING",
            &mission
                .checkpoint
                .outstanding
                .iter()
                .map(|line| format!("□ {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if let Some(result) = &mission.result {
        section(&mut lines, "REPORT", result);
    }
    if let Some(verification) = &mission.verification {
        section(&mut lines, "VERIFICATION", &pretty(verification));
    }
    if let Some(error) = &mission.error {
        section(&mut lines, "LAST STATUS DETAIL", error);
    }
    section(
        &mut lines,
        "RECORD",
        &format!(
            "Created  {}\nUpdated  {}\nVerification  {}",
            mission.created_at,
            mission.updated_at,
            if mission.spec.verify {
                "Required"
            } else {
                "Not requested"
            }
        ),
    );
    lines
}

fn pretty(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| text.to_string())
}

fn evidence_lines(state: &MissionsState) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    cached_events_notice(state, &mut lines);
    let mission = state.selected_mission();
    for event in state
        .events
        .iter()
        .filter(|event| event.kind == "tool_result")
    {
        let id = event.data["tool_id"].as_str().unwrap_or("unknown");
        let name = event.data["name"].as_str().unwrap_or("tool");
        let error = event.data["is_error"] == true;
        let cited = mission.is_some_and(|mission| {
            mission
                .checkpoint
                .evidence
                .iter()
                .any(|evidence| evidence == id)
        });
        lines.push(Line::from(vec![
            chrome::dot(if error { ERROR } else { STATUS_DONE }),
            Span::styled(
                format!(" {name}"),
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if cited {
                    "   CHECKPOINT EVIDENCE"
                } else if error {
                    "   ERROR"
                } else {
                    "   TOOL RESULT"
                },
                Style::default().fg(if cited { ACCENT } else { MUTED }),
            ),
        ]));
        lines.push(Line::styled(
            format!("{id}  ·  {}", event.created_at),
            Style::default().fg(MUTED),
        ));
        lines.push(Line::from(format!(
            "  {}",
            preview(event.data["result"].as_str().unwrap_or(""), 320)
        )));
        lines.push(Line::raw(""));
    }
    if lines.is_empty() {
        lines.push(chrome::section_title("No tool evidence yet", false));
        lines.push(Line::from(
            "Completed tool calls appear here with their original result IDs.",
        ));
    }
    lines
}

fn cached_events_notice(state: &MissionsState, lines: &mut Vec<Line<'static>>) {
    if state.events.len() >= 2000 {
        lines.push(Line::styled("Showing the latest 2,000 events. Earlier evidence remains in the full mission journal (CLI).", Style::default().fg(SECONDARY)));
        lines.push(Line::raw(""));
    }
}

fn event_lines(state: &MissionsState) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    cached_events_notice(state, &mut lines);
    for event in &state.events {
        lines.push(Line::from(vec![
            Span::styled(format!("#{} ", event.sequence), Style::default().fg(DIM)),
            Span::styled(
                event.kind.replace('_', " "),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", event.created_at),
                Style::default().fg(MUTED),
            ),
        ]));
        lines.push(Line::styled(
            format!("  {}", event_preview(event)),
            Style::default().fg(TEXT),
        ));
        lines.push(Line::raw(""));
    }
    if lines.is_empty() {
        lines.push(Line::styled(
            "No recorded events yet.",
            Style::default().fg(MUTED),
        ));
    }
    lines
}

fn preview(value: &str, limit: usize) -> String {
    let mut text = String::new();
    let mut whitespace = false;
    let mut count = 0;
    for character in value.chars() {
        if count == limit {
            text.push('…');
            break;
        }
        if character.is_whitespace() {
            whitespace = true;
            continue;
        }
        if whitespace && !text.is_empty() {
            text.push(' ');
            count += 1;
        }
        whitespace = false;
        text.push(character);
        count += 1;
    }
    text
}

fn event_preview(event: &common::missions::MissionEvent) -> String {
    let data = &event.data;
    let detail = match event.kind.as_str() {
        "tool_dispatch" | "tool_result" => {
            let name = data["name"].as_str().unwrap_or("tool");
            let id = data["tool_id"].as_str().unwrap_or("unknown");
            let outcome = if data["is_error"] == true {
                " · error"
            } else {
                ""
            };
            let result = data["result"]
                .as_str()
                .or_else(|| data["arguments"].as_str())
                .unwrap_or("");
            format!("{name} · {id}{outcome} · {}", preview(result, 220))
        }
        "assistant" => preview(data["text"].as_str().unwrap_or(""), 220),
        "usage" => format!(
            "{} tokens this turn · {} total",
            data["total_tokens"], data["tokens_used"]
        ),
        "status" => format!(
            "{} · {}",
            data["status"].as_str().unwrap_or(""),
            data["error"].as_str().unwrap_or("")
        ),
        "created" => preview(data["name"].as_str().unwrap_or(""), 220),
        _ => preview(&data.to_string(), 220),
    };
    detail
}

fn empty(f: &mut Frame, area: Rect, title: &str, detail: &str) {
    let mut lines = vec![
        Line::raw(""),
        chrome::section_title(title, false),
        Line::raw(""),
    ];
    lines.extend(
        detail
            .lines()
            .map(|line| Line::styled(line.to_string(), Style::default().fg(MUTED))),
    );
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn render_form(
    f: &mut Frame,
    area: Rect,
    form: &MissionForm,
    mission: Option<&Mission>,
    app: Option<&App>,
) {
    let width = area.width.saturating_sub(4).min(88).max(1).min(area.width);
    let desired_height = if form.fields.is_empty() {
        13
    } else {
        form.fields
            .iter()
            .map(|field| {
                if field.kind == MissionFieldKind::Multiline {
                    7
                } else {
                    4
                }
            })
            .sum::<u16>()
            + 9
    };
    let height = area
        .height
        .saturating_sub(2)
        .min(desired_height)
        .min(38)
        .max(1)
        .min(area.height);
    let popup = crate::ui::common::centered_rect_fixed(width, height, area);
    let body = chrome::modal_panel(f, popup, form.title(), "esc close");
    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .split(body);
    let mut lines = Vec::new();
    match &form.kind {
        MissionFormKind::Create => {
            lines.push(Line::styled(
                "Define the outcome, its scope, and how much work to allow.",
                Style::default().fg(MUTED),
            ));
            lines.push(Line::raw(""));
        }
        MissionFormKind::Resume {
            requires_reconciliation: true,
            ..
        } => {
            lines.push(Line::styled(
                "Review uncertain actions and record their outcomes before continuing.",
                Style::default().fg(SECONDARY),
            ));
            lines.push(Line::raw(""));
        }
        MissionFormKind::Resume { .. } => {
            lines.push(Line::styled(
                "Budgets are totals for the mission, including work already completed.",
                Style::default().fg(MUTED),
            ));
            lines.push(Line::raw(""));
        }
        MissionFormKind::Steer { .. } => {
            lines.push(Line::styled(
                "Add guidance for the mission's next step.",
                Style::default().fg(MUTED),
            ));
            lines.push(Line::raw(""));
        }
        MissionFormKind::Cancel { name, .. } => {
            lines.push(Line::styled(
                format!("Cancel {name}?"),
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::raw(""));
            lines.push(Line::from(
                "The mission will stop. Its history and evidence remain available.",
            ));
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "Press Enter to confirm, or Esc to keep it running.",
                Style::default().fg(SECONDARY),
            ));
        }
    }
    if matches!(form.kind, MissionFormKind::Resume { .. }) {
        if let Some(mission) = mission {
            lines.push(Line::styled(
                format!(
                    "Used: {} steps · {} tokens · {} active",
                    mission.iterations,
                    compact_number(mission.tokens_used),
                    duration(mission.active_seconds)
                ),
                Style::default().fg(TEXT_BRIGHT),
            ));
            lines.push(Line::raw(""));
        }
    }
    let mut fields = Vec::new();
    let mut focused_end = 0;
    for (index, field) in form.fields.iter().enumerate() {
        let start = lines.len();
        let focused = index == form.focused;
        lines.push(Line::from(vec![
            Span::styled(
                if focused { "› " } else { "  " },
                Style::default().fg(ACCENT),
            ),
            Span::styled(
                field.label,
                Style::default()
                    .fg(if focused { ACCENT } else { MUTED })
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        if field.kind == MissionFieldKind::Toggle {
            let enabled = matches!(field.value.as_str(), "true" | "yes" | "on");
            lines.push(Line::from(vec![
                Span::raw("  "),
                chrome::pill(
                    if enabled { "Enabled" } else { "Disabled" },
                    if enabled { ACCENT } else { MUTED },
                ),
                Span::styled("  space to change", Style::default().fg(DIM)),
            ]));
        } else {
            lines.extend(field_lines(
                field,
                chunks[0].width.saturating_sub(2).max(1),
                focused,
            ));
        }
        if !field.hint.is_empty() {
            lines.push(Line::styled(
                format!("  {}", field.hint),
                Style::default().fg(DIM),
            ));
        }
        fields.push((index, start, lines.len()));
        if focused {
            focused_end = lines.len();
        }
        lines.push(Line::raw(""));
    }
    let offset = focused_end.saturating_sub(chunks[0].height as usize);
    for (index, start, end) in fields {
        let start = start.max(offset);
        let end = end.min(offset + chunks[0].height as usize);
        if end > start {
            register(
                app,
                Rect::new(
                    chunks[0].x,
                    chunks[0].y + (start - offset) as u16,
                    chunks[0].width,
                    (end - start) as u16,
                ),
                MouseAction::MissionField(index),
            );
        }
    }
    let has_below = lines
        .iter()
        .skip(offset + chunks[0].height as usize)
        .any(|line| line.width() > 0);
    f.render_widget(
        Paragraph::new(lines.into_iter().skip(offset).collect::<Vec<_>>())
            .style(Style::default().fg(TEXT).bg(BG_ELEMENT)),
        chunks[0],
    );
    if let Some(error) = &form.error {
        f.render_widget(
            Paragraph::new(error.as_str()).style(Style::default().fg(ERROR)),
            chunks[1],
        );
    } else if has_below {
        f.render_widget(
            Paragraph::new("More fields below · Tab to continue")
                .style(Style::default().fg(SECONDARY)),
            chunks[1],
        );
    } else if offset > 0 {
        f.render_widget(
            Paragraph::new("More fields above · Shift+Tab to return")
                .style(Style::default().fg(MUTED)),
            chunks[1],
        );
    }
    let label = if form.submitting {
        "Submitting…"
    } else {
        form.submit_label()
    };
    let footer_app = if form.submitting { None } else { app };
    hint_row::render(
        f,
        row(chunks[2], 0),
        &[
            hint_row::HintItem::with_action(
                "Ctrl+S",
                label,
                MouseAction::MissionKey(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            ),
            hint("Esc", "close", KeyCode::Esc),
        ],
        footer_app,
    );
    f.render_widget(
        Paragraph::new(if form.fields.is_empty() {
            "Enter confirms · Esc closes".to_string()
        } else {
            format!(
                "Field {}/{}   Tab next   Shift+Enter newline",
                form.focused + 1,
                form.fields.len()
            )
        })
        .style(Style::default().fg(DIM)),
        row(chunks[2], 1),
    );
}

fn field_lines(field: &MissionField, width: u16, focused: bool) -> Vec<Line<'static>> {
    let mut cursor = field.cursor.min(field.value.len());
    while !field.value.is_char_boundary(cursor) {
        cursor = cursor.saturating_sub(1);
    }
    let mut lines = Vec::new();
    let mut spans = Vec::new();
    let mut columns = 0;
    let mut cursor_line = 0;
    for (byte, character) in field
        .value
        .char_indices()
        .chain(std::iter::once((field.value.len(), ' ')))
    {
        let is_cursor = focused && byte == cursor;
        if character == '\n' {
            if is_cursor {
                spans.push(Span::styled(" ", Style::default().bg(ACCENT)));
                cursor_line = lines.len();
            }
            lines.push(Line::from(std::mem::take(&mut spans)));
            columns = 0;
            continue;
        }
        if byte == field.value.len() && !is_cursor {
            break;
        }
        let text = character.to_string();
        let cell_width = Span::raw(text.clone()).width() as u16;
        if columns + cell_width > width && !spans.is_empty() {
            lines.push(Line::from(std::mem::take(&mut spans)));
            columns = 0;
        }
        if is_cursor {
            cursor_line = lines.len();
        }
        spans.push(Span::styled(
            text,
            if is_cursor {
                Style::default().fg(BG).bg(ACCENT)
            } else {
                Style::default().fg(TEXT_BRIGHT)
            },
        ));
        columns += cell_width;
    }
    if !spans.is_empty() || lines.is_empty() {
        lines.push(Line::from(spans));
    }
    let capacity = if field.kind == MissionFieldKind::Multiline {
        4
    } else {
        1
    };
    let offset = if focused {
        cursor_line.saturating_sub(capacity - 1)
    } else {
        0
    };
    lines
        .into_iter()
        .skip(offset)
        .take(capacity)
        .map(|mut line| {
            line.spans.insert(0, Span::raw("  "));
            line
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::missions::{MissionBudget, MissionCheckpoint, MissionEvent, MissionSpec};
    use ratatui::{Terminal, backend::TestBackend};

    fn mission() -> Mission {
        Mission {
            id: "12345678-1234-1234-1234-123456789012".into(),
            spec: MissionSpec {
                name: "Investigate the fixture".into(),
                objective: "Find the observed behavior".into(),
                scope: "Fixtures only".into(),
                model: Some("fixture-model".into()),
                budget: MissionBudget::default(),
                verify: true,
            },
            status: MissionStatus::Running,
            checkpoint: MissionCheckpoint::default(),
            iterations: 3,
            tokens_used: 1200,
            active_seconds: 14,
            pending_tools: vec![],
            result: None,
            verification: None,
            error: None,
            created_at: "2026-10-18T10:00:00Z".into(),
            updated_at: "2026-10-18T10:01:00Z".into(),
        }
    }

    fn populated() -> MissionsState {
        let mut state = MissionsState::default();
        state.missions.push(mission());
        state.loaded = true;
        state
    }

    fn snapshot(state: &MissionsState, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render_state(frame, frame.area(), state, None))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn large_workspace_shows_missions_objective_and_budget() {
        let text = snapshot(&populated(), 140, 40);
        for expected in [
            "MISSIONS",
            "Investigate the fixture",
            "OBJECTIVE",
            "Find the observed behavior",
            "Steps",
            "Tokens",
            "Time",
            "14s",
            "Ctrl+N",
            "Tab",
        ] {
            assert!(text.contains(expected), "Missing {expected}:\n{text}");
        }
    }

    #[test]
    fn narrow_workspace_switches_between_list_and_detail_without_losing_navigation() {
        let mut state = populated();
        let list = snapshot(&state, 80, 24);
        assert!(list.contains("Investigate the fixture"));
        assert!(!list.contains("OBJECTIVE"));
        assert!(list.contains("Tab detail"));
        assert!(list.contains("x cancel"));
        state.focus = MissionFocus::Detail;
        let detail = snapshot(&state, 80, 24);
        assert!(detail.contains("OBJECTIVE"));
        assert!(detail.contains("Tab list"));
        assert!(detail.contains("14s / 1h0m"));
    }

    #[test]
    fn evidence_keeps_tool_ids_and_original_results_visible() {
        let mut state = populated();
        state.focus = MissionFocus::Detail;
        state.tab = MissionTab::Evidence;
        state.events.push(MissionEvent {
            mission_id: state.missions[0].id.clone(), sequence: 1, kind: "tool_result".into(),
            created_at: "2026-10-18T10:01:00Z".into(),
            data: serde_json::json!({"tool_id":"observed-call","name":"read_fixture","is_error":false,"result":"The file exists"}),
        });
        let text = snapshot(&state, 140, 40);
        assert!(text.contains("observed-call"));
        assert!(text.contains("read_fixture"));
        assert!(text.contains("The file exists"));
    }

    #[test]
    fn long_mission_events_render_bounded_previews() {
        let mut state = populated();
        state.focus = MissionFocus::Detail;
        state.tab = MissionTab::Events;
        state.events.push(MissionEvent {
            mission_id: state.missions[0].id.clone(), sequence: 42, kind: "tool_result".into(),
            created_at: "2026-10-18T10:01:00Z".into(),
            data: serde_json::json!({"tool_id":"observed-call","name":"read_fixture","is_error":false,"result":format!("visible {} hidden-tail", "x".repeat(20_000))}),
        });
        let lines = event_lines(&state);
        assert_eq!(lines.len(), 3);
        let text = snapshot(&state, 140, 40);
        assert!(text.contains("observed-call"));
        assert!(text.contains("visible"));
        assert!(!text.contains("hidden-tail"));
    }

    #[test]
    fn forms_scroll_to_focused_fields_and_explain_remaining_fields() {
        let mut state = populated();
        state.open_create("");
        let first = snapshot(&state, 80, 24);
        assert!(first.contains("Create mission"));
        assert!(first.contains("More fields below"));
        assert!(first.contains("Ctrl+S"));
        let form = state.form.as_mut().unwrap();
        form.focused = form.fields.len() - 1;
        let label = form.fields[form.focused].label;
        let last = snapshot(&state, 80, 24);
        assert!(last.contains(label));
        assert!(last.contains("More fields above"));
        assert!(last.contains("Field 8/8"));
    }

    #[test]
    fn layouts_and_forms_fit_even_very_small_terminals() {
        let mut state = populated();
        for (width, height) in [(1, 1), (12, 6), (40, 12), (80, 24), (140, 40)] {
            state.form = None;
            for focus in [MissionFocus::List, MissionFocus::Detail] {
                state.focus = focus;
                snapshot(&state, width, height);
            }
            state.open_create("");
            snapshot(&state, width, height);
        }
    }

    #[test]
    fn unicode_field_cursor_stays_visible_after_wrapping() {
        let field = MissionField {
            label: "Objective",
            value: "λ".repeat(100),
            cursor: 200,
            kind: MissionFieldKind::Multiline,
            hint: "",
        };
        let lines = field_lines(&field, 10, true);
        assert!(lines.len() <= 4);
        assert!(
            lines
                .last()
                .unwrap()
                .spans
                .iter()
                .any(|span| span.style.bg == Some(ACCENT))
        );
        assert!(lines.iter().all(|line| line.width() <= 12));
    }
}
