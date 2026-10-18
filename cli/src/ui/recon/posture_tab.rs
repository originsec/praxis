use crate::app::{App, PostureSectionKind, ReconExpandId, ReconNodeId, ReconOverlay};
use crate::ui::common::focused_titled_panel;
use crate::ui::recon::tree::VisibleRow;
use crate::ui::theme::{
    ACCENT, DIM, MUTED, STATUS_DONE, STATUS_FAIL, STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};
use common::posture::{AgentPosture, PostureCategory, PostureResult, PostureSetting};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

//
// Posture tab: managed policy, telemetry and sandbox settings for the
// overlay's agent, plus host security products for its node. Rows reuse the
// recon tree browser; data comes from overlay.posture rather than the recon
// result.
//

pub fn posture_of(overlay: &ReconOverlay) -> Option<&PostureResult> {
    overlay.posture.as_ref().and_then(|p| p.posture.as_ref())
}

fn agent_of<'a>(overlay: &ReconOverlay, posture: &'a PostureResult) -> Option<&'a AgentPosture> {
    posture
        .agents
        .iter()
        .find(|a| a.agent_short_name == overlay.agent_short_name)
}

fn matches(filter: &str, haystacks: &[&str]) -> bool {
    let f = filter.trim().to_lowercase();
    f.is_empty() || haystacks.iter().any(|h| h.to_lowercase().contains(&f))
}

fn section_row(
    overlay: &ReconOverlay,
    kind: PostureSectionKind,
    label: &str,
    meta: String,
    force: bool,
) -> (VisibleRow, bool) {
    let expanded = force
        || overlay
            .expanded
            .contains(&ReconExpandId::PostureSection(kind));
    (
        VisibleRow {
            id: ReconNodeId::PostureSection(kind),
            depth: 0,
            expandable: true,
            expanded,
            label: label.to_string(),
            meta,
            badge: None,
            trailing: String::new(),
        },
        expanded,
    )
}

fn setting_rows(agent: &AgentPosture, category: PostureCategory, filter: &str) -> Vec<VisibleRow> {
    agent
        .settings
        .iter()
        .enumerate()
        .filter(|(_, s)| s.category == category)
        .filter(|(_, s)| matches(filter, &[&s.key, &s.value, &s.source]))
        .map(|(i, s)| VisibleRow {
            id: ReconNodeId::PostureSetting(i),
            depth: 1,
            expandable: false,
            expanded: false,
            label: s.key.clone(),
            meta: s.value.clone(),
            badge: Some(if s.effective {
                ("effective".to_string(), STATUS_DONE)
            } else {
                ("overridden".to_string(), DIM)
            }),
            trailing: s.scope.as_str().to_string(),
        })
        .collect()
}

pub fn build_rows(overlay: &ReconOverlay) -> Vec<VisibleRow> {
    let Some(posture) = posture_of(overlay) else {
        return Vec::new();
    };
    let filter = overlay.filter.as_str();
    let filtering = !filter.trim().is_empty();
    let mut rows = Vec::new();

    if let Some(agent) = agent_of(overlay, posture) {
        let present: Vec<_> = agent
            .policy_sources
            .iter()
            .enumerate()
            .filter(|(_, p)| p.present)
            .filter(|(_, p)| {
                matches(filter, &[&p.location])
                    || p.keys
                        .iter()
                        .any(|k| matches(filter, &[&k.key, &k.summary]))
            })
            .collect();
        let meta = if agent.managed_policy {
            "(present)".to_string()
        } else {
            "(none)".to_string()
        };
        let (row, expanded) = section_row(
            overlay,
            PostureSectionKind::Policy,
            "Managed Policy",
            meta,
            filtering,
        );
        rows.push(row);
        if expanded {
            for (i, p) in present {
                rows.push(VisibleRow {
                    id: ReconNodeId::PosturePolicy(i),
                    depth: 1,
                    expandable: false,
                    expanded: false,
                    label: p.location.clone(),
                    meta: format!("{} key(s)", p.keys.len()),
                    badge: Some(match (p.scope.map(|s| s.as_str()), &p.error) {
                        (_, Some(_)) => ("error".to_string(), STATUS_FAIL),
                        (Some(scope), None) => (scope.to_string(), STATUS_RUNNING),
                        (None, None) => ("policy".to_string(), STATUS_RUNNING),
                    }),
                    trailing: String::new(),
                });
            }
        }

        let telemetry_meta = match agent.telemetry.enabled {
            Some(true) if !agent.telemetry.endpoints.is_empty() => {
                format!("(enabled -> {})", agent.telemetry.endpoints.join(", "))
            }
            Some(true) => "(enabled)".to_string(),
            Some(false) => "(disabled)".to_string(),
            None => "(not configured)".to_string(),
        };
        let (row, expanded) = section_row(
            overlay,
            PostureSectionKind::Telemetry,
            "Telemetry",
            telemetry_meta,
            filtering,
        );
        rows.push(row);
        if expanded {
            rows.extend(setting_rows(agent, PostureCategory::Telemetry, filter));
        }

        let sandbox_count = agent
            .settings
            .iter()
            .filter(|s| s.category == PostureCategory::Sandbox)
            .count();
        let (row, expanded) = section_row(
            overlay,
            PostureSectionKind::Sandbox,
            "Sandbox & Permissions",
            format!("({})", sandbox_count),
            filtering,
        );
        rows.push(row);
        if expanded {
            rows.extend(setting_rows(agent, PostureCategory::Sandbox, filter));
        }
    }

    let products = &posture.host.security_products;
    let (row, expanded) = section_row(
        overlay,
        PostureSectionKind::Host,
        "Host Security",
        format!("({})", products.len()),
        filtering,
    );
    rows.push(row);
    if expanded {
        for (i, p) in products.iter().enumerate() {
            if !matches(filter, &[&p.name, &p.category]) {
                continue;
            }
            rows.push(VisibleRow {
                id: ReconNodeId::PostureProduct(i),
                depth: 1,
                expandable: false,
                expanded: false,
                label: p.name.clone(),
                meta: p.category.clone(),
                badge: Some(if p.running {
                    ("running".to_string(), STATUS_RUNNING)
                } else {
                    ("installed".to_string(), MUTED)
                }),
                trailing: String::new(),
            });
        }
    }
    rows
}

pub fn render(f: &mut Frame, area: Rect, app: &App, overlay: &ReconOverlay) {
    let lookup = overlay.posture.as_ref();
    if posture_of(overlay).is_none() {
        let (msg, style) = if overlay.posture_loading {
            (
                " Scanning posture...".to_string(),
                Style::default().fg(STATUS_RUNNING),
            )
        } else if let Some(e) = lookup.and_then(|l| l.error.as_ref()) {
            (format!(" Error: {}", e), Style::default().fg(STATUS_FAIL))
        } else {
            (
                " No posture data (press r to scan)".to_string(),
                Style::default().fg(DIM),
            )
        };
        f.render_widget(Paragraph::new(Line::from(Span::styled(msg, style))), area);
        return;
    }

    let (left, right) = super::common_two_pane_layout(area, overlay.recon_split_percent);
    super::render_tree_left(f, left, app, overlay, " Posture ");

    let block = focused_titled_panel(&detail_title(overlay), overlay.right_pane_focused);
    let inner = block.inner(right);
    f.render_widget(block, right);
    let paragraph = Paragraph::new(detail_lines(overlay)).wrap(Wrap { trim: false });
    let total = paragraph.line_count(inner.width) as u16;
    let max_scroll = total.saturating_sub(inner.height);
    overlay.right_pane_max_scroll.set(max_scroll);
    let effective = overlay.selected_right_scroll.min(max_scroll);
    f.render_widget(paragraph.scroll((effective, 0)), inner);
}

pub fn detail_title(overlay: &ReconOverlay) -> String {
    let posture = posture_of(overlay);
    let agent = posture.and_then(|p| agent_of(overlay, p));
    match &overlay.selected {
        Some(ReconNodeId::PostureSection(kind)) => match kind {
            PostureSectionKind::Policy => " Managed Policy ".to_string(),
            PostureSectionKind::Telemetry => " Telemetry ".to_string(),
            PostureSectionKind::Sandbox => " Sandbox & Permissions ".to_string(),
            PostureSectionKind::Host => " Host Security ".to_string(),
        },
        Some(ReconNodeId::PosturePolicy(i)) => agent
            .and_then(|a| a.policy_sources.get(*i))
            .map(|p| format!(" {} ", p.location))
            .unwrap_or_else(|| " Policy ".to_string()),
        Some(ReconNodeId::PostureSetting(i)) => agent
            .and_then(|a| a.settings.get(*i))
            .map(|s| format!(" {} ", s.key))
            .unwrap_or_else(|| " Setting ".to_string()),
        Some(ReconNodeId::PostureProduct(i)) => posture
            .and_then(|p| p.host.security_products.get(*i))
            .map(|p| format!(" {} ", p.name))
            .unwrap_or_else(|| " Product ".to_string()),
        _ => " Detail ".to_string(),
    }
}

fn kv(key: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {:<12}", key), Style::default().fg(MUTED)),
        Span::styled(value.into(), Style::default().fg(TEXT)),
    ])
}

fn heading(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {}", text),
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    ))
}

fn dim(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {}", text.into()),
        Style::default().fg(DIM),
    ))
}

fn setting_detail(agent: &AgentPosture, s: &PostureSetting) -> Vec<Line<'static>> {
    let mut lines = vec![
        kv("Key", s.key.clone()),
        kv("Value", s.value.clone()),
        kv("Scope", s.scope.as_str()),
        kv("Source", s.source.clone()),
        Line::from(vec![
            Span::styled(format!(" {:<12}", "Status"), Style::default().fg(MUTED)),
            if s.effective {
                Span::styled("effective", Style::default().fg(STATUS_DONE))
            } else {
                Span::styled(
                    "overridden by a higher-precedence source",
                    Style::default().fg(STATUS_RUNNING),
                )
            },
        ]),
    ];
    let others: Vec<_> = agent
        .settings
        .iter()
        .filter(|o| o.key == s.key && o.source != s.source)
        .collect();
    if !others.is_empty() {
        lines.push(Line::from(""));
        lines.push(heading("Also set in"));
        for o in others {
            lines.push(dim(format!(
                "{} [{}{}] = {}",
                o.source,
                o.scope.as_str(),
                if o.effective { ", effective" } else { "" },
                o.value
            )));
        }
    }
    lines
}

pub fn detail_lines(overlay: &ReconOverlay) -> Vec<Line<'static>> {
    let Some(posture) = posture_of(overlay) else {
        return Vec::new();
    };
    let agent = agent_of(overlay, posture);
    let mut lines = Vec::new();
    if let Some(at) = overlay
        .posture
        .as_ref()
        .and_then(|l| l.performed_at.as_ref())
    {
        lines.push(dim(format!("Scanned {}", at)));
    }
    if let Some(err) = overlay.posture.as_ref().and_then(|l| l.error.as_ref()) {
        lines.push(Line::from(Span::styled(
            format!(" Last refresh failed: {}", err),
            Style::default().fg(STATUS_FAIL),
        )));
    }
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }

    match &overlay.selected {
        Some(ReconNodeId::PostureSection(PostureSectionKind::Policy)) => {
            let Some(agent) = agent else {
                return lines;
            };
            if agent.managed_policy {
                lines.push(Line::from(Span::styled(
                    " Managed policy present: user-level edits to enforced keys will not take effect.",
                    Style::default().fg(STATUS_RUNNING),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    " No managed policy found: user-level config is authoritative.",
                    Style::default().fg(ACCENT),
                )));
            }
            lines.push(Line::from(""));
            lines.push(heading("Locations checked"));
            for p in &agent.policy_sources {
                lines.push(dim(format!(
                    "{} {}",
                    if p.present { "[present]" } else { "[absent] " },
                    p.location
                )));
            }
            for note in &agent.notes {
                lines.push(Line::from(""));
                lines.push(dim(note.clone()));
            }
        }
        Some(ReconNodeId::PostureSection(
            kind @ (PostureSectionKind::Telemetry | PostureSectionKind::Sandbox),
        )) => {
            let Some(agent) = agent else {
                return lines;
            };
            if *kind == PostureSectionKind::Telemetry {
                let state = match agent.telemetry.enabled {
                    Some(true) => "enabled",
                    Some(false) => "disabled",
                    None => "not configured",
                };
                lines.push(kv("Export", state));
                if !agent.telemetry.endpoints.is_empty() {
                    lines.push(kv("Endpoints", agent.telemetry.endpoints.join(", ")));
                }
                lines.push(Line::from(""));
            }
            lines.push(dim(
                "Values are shown at every source; [effective] marks the one that wins at precedence.",
            ));
        }
        Some(ReconNodeId::PostureSection(PostureSectionKind::Host)) => {
            lines.push(kv("OS", posture.host.os.clone()));
            lines.push(kv(
                "Signatures",
                posture.host.signatures_evaluated.to_string(),
            ));
            lines.push(kv(
                "Detected",
                posture.host.security_products.len().to_string(),
            ));
            lines.push(Line::from(""));
            lines.push(dim(
                "Signatures are edited in Settings > Posture (service-side TOML).",
            ));
        }
        Some(ReconNodeId::PosturePolicy(i)) => {
            let Some(p) = agent.and_then(|a| a.policy_sources.get(*i)) else {
                return lines;
            };
            lines.push(kv("Location", p.location.clone()));
            if let Some(scope) = p.scope {
                lines.push(kv("Scope", scope.as_str()));
            }
            if let Some(err) = &p.error {
                lines.push(Line::from(Span::styled(
                    format!(" Error: {}", err),
                    Style::default().fg(STATUS_FAIL),
                )));
            }
            lines.push(Line::from(""));
            lines.push(heading("Enforced keys"));
            for k in &p.keys {
                lines.push(Line::from(vec![
                    Span::styled(format!(" [{}] ", k.category), Style::default().fg(MUTED)),
                    Span::styled(k.key.clone(), Style::default().fg(TEXT_BRIGHT)),
                    Span::styled(format!(" = {}", k.summary), Style::default().fg(TEXT)),
                ]));
            }
            if let Some(raw) = &p.raw {
                lines.push(Line::from(""));
                lines.push(heading("Raw"));
                for l in raw.lines() {
                    lines.push(Line::from(Span::styled(
                        format!(" {}", l),
                        Style::default().fg(TEXT),
                    )));
                }
            }
        }
        Some(ReconNodeId::PostureSetting(i)) => {
            if let Some(agent) = agent
                && let Some(s) = agent.settings.get(*i)
            {
                lines.extend(setting_detail(agent, s));
            }
        }
        Some(ReconNodeId::PostureProduct(i)) => {
            let Some(p) = posture.host.security_products.get(*i) else {
                return lines;
            };
            lines.push(kv("Product", p.name.clone()));
            lines.push(kv("Category", p.category.clone()));
            lines.push(kv("Running", if p.running { "yes" } else { "no" }));
            lines.push(Line::from(""));
            lines.push(heading("Evidence"));
            for e in &p.evidence {
                lines.push(dim(e.clone()));
            }
        }
        _ => lines.push(dim("Select an item")),
    }
    lines
}
