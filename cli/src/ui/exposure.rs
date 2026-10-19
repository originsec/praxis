use crate::app::{App, ExposureOverlay, ExposureTab};
use crate::ui::chrome;
use crate::ui::common::{focused_titled_panel, short_id};
use crate::ui::hint_row::{self, HintItem};
use crate::ui::hits::{ExposureHintAction, MouseAction};
use crate::ui::theme::{
    ACCENT, BG_SELECTED, BORDER_SUBTLE, DIM, ERROR, MUTED, SECONDARY, STATUS_RUNNING, TEXT,
    TEXT_BRIGHT, WARN,
};
use common::exposure::{
    AgentExposure, CapabilityClass, ExposureReport, RiskLevel, SharedEntity, TrifectaPath,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

//
// Exposure overlay renderer: header with summary counts, tab bar
// (Agents / Paths / Shared), list + detail panes, hint row.
//

pub fn render(f: &mut Frame, area: Rect, app: &App, overlay: &ExposureOverlay) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // divider
        Constraint::Length(1), // tabs
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);

    render_header(f, chunks[0], overlay);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(chunks[1].width as usize),
            Style::default().fg(BORDER_SUBTLE),
        ))),
        chunks[1],
    );
    render_tabs(f, chunks[2], app, overlay);
    render_content(f, chunks[3], app, overlay);
    render_hints(f, chunks[4], app);
}

pub fn risk_color(risk: RiskLevel) -> Color {
    match risk {
        RiskLevel::Critical => ERROR,
        RiskLevel::High => SECONDARY,
        RiskLevel::Medium => WARN,
        RiskLevel::Low => MUTED,
    }
}

fn render_header(f: &mut Frame, area: Rect, overlay: &ExposureOverlay) {
    let mut spans = vec![
        chrome::diamond(ACCENT),
        Span::raw(" "),
        Span::styled(
            "Exposure",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        chrome::mid_dot(),
        Span::styled("lethal trifecta", Style::default().fg(DIM)),
    ];
    if let Some(report) = &overlay.report {
        let s = report.summary();
        spans.push(chrome::mid_dot());
        spans.push(Span::styled(
            format!(
                "{} agents / {} nodes  trifecta {}  critical {}",
                s.agents, s.nodes, s.trifecta_agents, s.critical
            ),
            Style::default().fg(TEXT),
        ));
        if s.unclassified > 0 {
            spans.push(Span::styled(
                format!("  unclassified {}", s.unclassified),
                Style::default().fg(DIM),
            ));
        }
    }
    if overlay.is_loading {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            overlay.loading_label,
            Style::default()
                .fg(STATUS_RUNNING)
                .add_modifier(Modifier::ITALIC),
        ));
    } else if let Some(error) = &overlay.error {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("error: {}", error),
            Style::default().fg(ERROR).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some((msg, at)) = &overlay.status
        && at.elapsed() < std::time::Duration::from_secs(5)
    {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            msg.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App, overlay: &ExposureOverlay) {
    let mut spans = Vec::new();
    let mut x = 0u16;
    for (i, tab) in ExposureTab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(chrome::tab_sep());
            x = x.saturating_add(chrome::tab_sep_width());
        }
        let count = overlay.item_count(*tab);
        spans.extend(chrome::tab(tab.label(), Some(count), overlay.tab == *tab));
        let w = chrome::tab_width(tab.label(), Some(count));
        app.hits_register(
            Rect::new(area.x.saturating_add(x), area.y, w, 1),
            MouseAction::ExposureTab(*tab),
        );
        x = x.saturating_add(w);
    }
    spans.push(Span::raw("      "));
    spans.push(Span::styled("tab", Style::default().fg(TEXT_BRIGHT)));
    spans.push(Span::styled(" switch", Style::default().fg(MUTED)));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_hints(f: &mut Frame, area: Rect, app: &App) {
    let items = [
        HintItem::with_action(
            "r",
            "refresh",
            MouseAction::ExposureHint(ExposureHintAction::Refresh),
        ),
        HintItem::with_action(
            "p",
            "probe configs",
            MouseAction::ExposureHint(ExposureHintAction::Probe),
        ),
        HintItem::with_action(
            "l",
            "llm classify",
            MouseAction::ExposureHint(ExposureHintAction::Llm),
        ),
        HintItem::with_action(
            "e",
            "edit rules",
            MouseAction::ExposureHint(ExposureHintAction::EditRules),
        ),
        HintItem::new("^d", "reset rules"),
        HintItem::with_action(
            "x",
            "export",
            MouseAction::ExposureHint(ExposureHintAction::Export),
        ),
        HintItem::with_action(
            "esc",
            "close",
            MouseAction::ExposureHint(ExposureHintAction::Close),
        ),
    ];
    hint_row::render(f, area, &items, Some(app));
}

fn render_content(f: &mut Frame, area: Rect, app: &App, overlay: &ExposureOverlay) {
    let (left, right) = crate::ui::list_detail::two_pane(area, 40);
    app.text_selection_region_register(left);
    app.text_selection_region_register(right);
    app.hits_register(right, MouseAction::ExposureDetail);

    let list_block = focused_titled_panel(overlay.tab.label(), !overlay.detail_focused);
    let list_inner = list_block.inner(left);
    f.render_widget(list_block, left);

    let detail_block = focused_titled_panel("Detail", overlay.detail_focused);
    let detail_inner = detail_block.inner(right);
    f.render_widget(detail_block, right);

    let Some(report) = &overlay.report else {
        let (msg, color) = if overlay.is_loading {
            ("Computing exposure…", STATUS_RUNNING)
        } else {
            ("No report", ERROR)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(color)))),
            list_inner,
        );
        return;
    };

    let rows = list_rows(report, overlay.tab);
    if rows.is_empty() {
        let msg = match overlay.tab {
            ExposureTab::Agents => "No agents with stored recon. Run recon (r) on an agent first.",
            ExposureTab::Paths => "No agent has all three trifecta classes.",
            ExposureTab::Shared => "No MCP server or host is shared by two agents.",
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(DIM))))
                .wrap(Wrap { trim: true }),
            list_inner,
        );
    } else {
        let h = list_inner.height as usize;
        let scroll = if overlay.selected >= h {
            overlay.selected + 1 - h
        } else {
            0
        };
        let mut lines = Vec::new();
        for (i, row) in rows.into_iter().enumerate().skip(scroll).take(h) {
            let mut line = row;
            if i == overlay.selected {
                line = line.style(Style::default().bg(BG_SELECTED));
            }
            lines.push(line);
            app.hits_register(
                Rect::new(
                    list_inner.x,
                    list_inner.y.saturating_add((i - scroll) as u16),
                    list_inner.width,
                    1,
                ),
                MouseAction::ExposureRow(i),
            );
        }
        f.render_widget(Paragraph::new(lines), list_inner);
    }

    let detail = detail_lines(report, overlay);
    let paragraph = Paragraph::new(detail).wrap(Wrap { trim: false });
    let total = paragraph.line_count(detail_inner.width) as u16;
    overlay
        .detail_max_scroll
        .set(total.saturating_sub(detail_inner.height));
    f.render_widget(paragraph.scroll((overlay.detail_scroll, 0)), detail_inner);
}

fn badge_spans(classes: &[CapabilityClass]) -> Vec<Span<'static>> {
    CapabilityClass::ALL
        .iter()
        .map(|c| {
            if classes.contains(c) {
                Span::styled(
                    format!("{} ", c.badge()),
                    Style::default()
                        .fg(class_color(*c))
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("- ", Style::default().fg(DIM))
            }
        })
        .collect()
}

fn class_color(class: CapabilityClass) -> Color {
    match class {
        CapabilityClass::PrivateData => crate::ui::theme::TERTIARY,
        CapabilityClass::UntrustedContent => WARN,
        CapabilityClass::Exfiltration => ERROR,
    }
}

fn risk_span(risk: RiskLevel) -> Span<'static> {
    Span::styled(
        format!("{:<9}", risk.as_str()),
        Style::default()
            .fg(risk_color(risk))
            .add_modifier(Modifier::BOLD),
    )
}

fn node_label(node_id: &str, machine: &str) -> String {
    if machine.is_empty() {
        short_id(node_id).to_string()
    } else {
        machine.to_string()
    }
}

fn list_rows(report: &ExposureReport, tab: ExposureTab) -> Vec<Line<'static>> {
    match tab {
        ExposureTab::Agents => ExposureOverlay::sorted_agents(report)
            .into_iter()
            .map(|a| {
                let mut spans = vec![risk_span(a.risk)];
                spans.extend(badge_spans(&a.classes_present()));
                spans.push(Span::styled(
                    format!(" {:<14}", a.agent_short_name),
                    Style::default().fg(TEXT_BRIGHT),
                ));
                spans.push(Span::styled(
                    node_label(&a.node_id, &a.machine_name),
                    Style::default().fg(MUTED),
                ));
                Line::from(spans)
            })
            .collect(),
        ExposureTab::Paths => report
            .paths
            .iter()
            .map(|p| {
                Line::from(vec![
                    risk_span(p.risk),
                    Span::styled(
                        format!("{:<14}", p.agent_short_name),
                        Style::default().fg(TEXT_BRIGHT),
                    ),
                    Span::styled(
                        format!(
                            "{}  U{} P{} X{}",
                            node_label(&p.node_id, &p.machine_name),
                            p.sources.len(),
                            p.private_data.len(),
                            p.exfiltration.len()
                        ),
                        Style::default().fg(MUTED),
                    ),
                ])
            })
            .collect(),
        ExposureTab::Shared => report
            .shared
            .iter()
            .map(|s| {
                let mut spans = badge_spans(&s.classes);
                spans.push(Span::styled(
                    format!("{:<28}", s.label),
                    Style::default().fg(TEXT_BRIGHT),
                ));
                spans.push(Span::styled(
                    format!(" {} agents", s.agents.len()),
                    Style::default().fg(MUTED),
                ));
                if s.cross_node {
                    spans.push(Span::styled(" cross-node", Style::default().fg(SECONDARY)));
                }
                Line::from(spans)
            })
            .collect(),
    }
}

fn heading(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        text.into(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ))
}

fn kv(key: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<14}", key), Style::default().fg(MUTED)),
        Span::styled(value.into(), Style::default().fg(TEXT)),
    ])
}

fn item(text: impl Into<String>, note: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {}", text.into()), Style::default().fg(TEXT)),
        Span::styled(format!("  {}", note), Style::default().fg(DIM)),
    ])
}

fn detail_lines(report: &ExposureReport, overlay: &ExposureOverlay) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    match overlay.tab {
        ExposureTab::Agents => {
            if let Some(a) = ExposureOverlay::sorted_agents(report).get(overlay.selected) {
                agent_detail(report, a, &mut lines);
            }
        }
        ExposureTab::Paths => {
            if let Some(p) = report.paths.get(overlay.selected) {
                path_detail(p, &mut lines);
            }
        }
        ExposureTab::Shared => {
            if let Some(s) = report.shared.get(overlay.selected) {
                shared_detail(report, s, &mut lines);
            }
        }
    }
    if !report.warnings.is_empty() {
        lines.push(Line::raw(""));
        lines.push(heading("Warnings"));
        for w in &report.warnings {
            lines.push(Line::from(Span::styled(
                format!("  ! {}", w),
                Style::default().fg(WARN),
            )));
        }
    }
    lines
}

fn agent_detail(report: &ExposureReport, a: &AgentExposure, lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(vec![
        Span::styled(
            a.agent_short_name.clone(),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" @ {}", node_label(&a.node_id, &a.machine_name)),
            Style::default().fg(MUTED),
        ),
        Span::raw("  "),
        chrome::pill(&a.risk.as_str().to_uppercase(), risk_color(a.risk)),
    ]));
    lines.push(kv("node", a.node_id.clone()));
    lines.push(kv(
        "trifecta",
        if a.trifecta {
            "yes — all three classes present"
        } else {
            "no"
        },
    ));
    lines.push(kv(
        "recon",
        format!(
            "{} ({})",
            a.performed_at,
            if a.is_semantic { "semantic" } else { "static" }
        ),
    ));

    for class in CapabilityClass::ALL {
        let contributors = a.contributors(class);
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", class.badge()),
                Style::default()
                    .fg(class_color(class))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} ({})", class.label(), contributors.len()),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]));
        for cap in contributors {
            let reason = cap
                .classes
                .iter()
                .find(|c| c.class == class)
                .map(|c| c.reason.as_str())
                .unwrap_or("");
            lines.push(item(cap.label(), reason));
        }
    }

    lines.push(Line::raw(""));
    lines.push(heading(match &a.modifiers_probed_at {
        Some(at) => format!("Modifiers (probed {})", at),
        None => "Modifiers (not probed — press p)".to_string(),
    }));
    for m in &a.modifiers {
        lines.push(item(
            format!("{} {}", m.kind.as_str(), m.detail),
            &format!("{} · {}", m.reason, m.source),
        ));
    }

    let unclassified: Vec<String> = a
        .capabilities
        .iter()
        .filter(|c| c.is_unclassified())
        .map(|c| c.label())
        .collect();
    if !unclassified.is_empty() {
        lines.push(Line::raw(""));
        lines.push(heading(format!(
            "Unclassified ({}) — press l to ask the LLM",
            unclassified.len()
        )));
        for label in unclassified {
            lines.push(item(label, ""));
        }
    }

    if !a.observed_hosts.is_empty() {
        lines.push(Line::raw(""));
        lines.push(heading("Observed hosts"));
        for host in &a.observed_hosts {
            lines.push(item(host.clone(), ""));
        }
    }

    for path in report.paths_for_agent(&a.id) {
        lines.push(Line::raw(""));
        path_detail(path, lines);
    }
    let shared = report.shared_for_agent(&a.id);
    if !shared.is_empty() {
        lines.push(Line::raw(""));
        lines.push(heading("Shared with other agents"));
        for s in shared {
            lines.push(item(
                s.label.clone(),
                &format!("{} · {} agents", s.kind.as_str(), s.agents.len()),
            ));
        }
    }
}

fn path_detail(p: &TrifectaPath, lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(vec![
        Span::styled(
            "Trifecta path ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        chrome::pill(&p.risk.as_str().to_uppercase(), risk_color(p.risk)),
    ]));
    let sections: [(&str, &[String]); 3] = [
        ("untrusted in", &p.sources),
        ("private data", &p.private_data),
        ("exfil out", &p.exfiltration),
    ];
    for (i, (label, items)) in sections.iter().enumerate() {
        if i == 1 {
            lines.push(Line::from(Span::styled(
                format!(
                    "  ↓ {} @ {}",
                    p.agent_short_name,
                    node_label(&p.node_id, &p.machine_name)
                ),
                Style::default().fg(TEXT_BRIGHT),
            )));
        }
        lines.push(Line::from(Span::styled(
            format!("  {} ({})", label, items.len()),
            Style::default().fg(MUTED),
        )));
        for entry in items.iter() {
            lines.push(item(format!("  {}", entry), ""));
        }
    }
    if !p.unattended.is_empty() {
        lines.push(Line::from(Span::styled(
            "  unattended (no human in the loop)",
            Style::default().fg(ERROR),
        )));
        for entry in &p.unattended {
            lines.push(item(format!("  {}", entry), ""));
        }
    }
}

fn shared_detail(report: &ExposureReport, s: &SharedEntity, lines: &mut Vec<Line<'static>>) {
    let mut title = vec![Span::styled(
        s.label.clone(),
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )];
    title.push(Span::raw("  "));
    title.extend(badge_spans(&s.classes));
    lines.push(Line::from(title));
    lines.push(kv("kind", s.kind.as_str()));
    lines.push(kv("entity", s.entity_id.clone()));
    if let Some(entity) = report.graph.entity(&s.entity_id) {
        for (k, v) in &entity.attrs {
            lines.push(kv(k, v.clone()));
        }
    }
    lines.push(kv(
        "reach",
        format!(
            "{} agents on {} node(s){}{}",
            s.agents.len(),
            s.nodes.len(),
            if s.cross_node { " · cross-node" } else { "" },
            if s.reaches_trifecta {
                " · reaches a trifecta agent"
            } else {
                ""
            }
        ),
    ));
    lines.push(Line::raw(""));
    lines.push(heading("Agents"));
    for id in &s.agents {
        match report.agents.iter().find(|a| &a.id == id) {
            Some(a) => lines.push(Line::from(vec![
                Span::raw("  "),
                risk_span(a.risk),
                Span::styled(
                    format!(
                        "{} @ {}",
                        a.agent_short_name,
                        node_label(&a.node_id, &a.machine_name)
                    ),
                    Style::default().fg(TEXT),
                ),
            ])),
            None => lines.push(item(id.clone(), "")),
        }
    }
}
