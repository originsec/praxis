//
// Hierarchical recon browser: flatten expand/filter state into visible
// rows for the left pane, shared across Config / Tools / Sessions.
//

use crate::app::{ReconExpandId, ReconNodeId, ReconOverlay, ReconTab, ToolsSectionKind};
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, STATUS_DONE, STATUS_FAIL, STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug)]
pub struct VisibleRow {
    pub id: ReconNodeId,
    pub depth: u8,
    pub expandable: bool,
    pub expanded: bool,
    pub label: String,
    pub meta: String,
    pub badge: Option<(String, ratatui::style::Color)>,
    pub trailing: String,
}

pub fn selected_config_idx(selected: &Option<ReconNodeId>) -> Option<usize> {
    match selected {
        Some(ReconNodeId::ConfigItem(i)) => Some(*i),
        _ => None,
    }
}

pub fn selected_session_idx(selected: &Option<ReconNodeId>) -> Option<usize> {
    match selected {
        Some(ReconNodeId::SessionItem(i)) => Some(*i),
        _ => None,
    }
}

pub fn expand_id_for(id: &ReconNodeId) -> Option<ReconExpandId> {
    match id {
        ReconNodeId::ToolsSection(ToolsSectionKind::Mcp) => Some(ReconExpandId::ToolsMcp),
        ReconNodeId::ToolsSection(ToolsSectionKind::Skills) => Some(ReconExpandId::ToolsSkills),
        ReconNodeId::ToolsSection(ToolsSectionKind::Internal) => Some(ReconExpandId::ToolsInternal),
        ReconNodeId::McpServer(i) => Some(ReconExpandId::McpServer(*i)),
        ReconNodeId::ConfigType(t) => Some(ReconExpandId::ConfigType(t.clone())),
        ReconNodeId::SessionProject(p) => Some(ReconExpandId::SessionProject(p.clone())),
        _ => None,
    }
}

pub fn is_expandable(id: &ReconNodeId) -> bool {
    expand_id_for(id).is_some()
}

pub fn toggle_expand(overlay: &mut ReconOverlay, id: &ReconNodeId) {
    if let Some(eid) = expand_id_for(id) {
        if !overlay.expanded.remove(&eid) {
            overlay.expanded.insert(eid);
        }
    }
}

pub fn default_expanded(result: &common::ReconResult) -> HashSet<ReconExpandId> {
    let mut e = HashSet::new();
    e.insert(ReconExpandId::ToolsMcp);
    if result.tools.skills.len() <= 8 {
        e.insert(ReconExpandId::ToolsSkills);
    }
    // Internal often long / semantic-only — start collapsed.

    let mut types: HashSet<String> = HashSet::new();
    for item in &result.config.items {
        types.insert(item.config_type.clone());
    }
    for t in types {
        e.insert(ReconExpandId::ConfigType(t));
    }

    let mut projects: BTreeMap<String, usize> = BTreeMap::new();
    for s in &result.sessions.items {
        let key = if s.context_path.is_empty() {
            "(no project)".to_string()
        } else {
            s.context_path.clone()
        };
        *projects.entry(key).or_default() += 1;
    }
    if result.sessions.items.len() <= 20 {
        for p in projects.keys() {
            e.insert(ReconExpandId::SessionProject(p.clone()));
        }
    } else if let Some(first) = projects.keys().next() {
        e.insert(ReconExpandId::SessionProject(first.clone()));
    }
    e
}

fn filter_active(filter: &str) -> bool {
    !filter.trim().is_empty()
}

fn matches_filter(filter: &str, haystacks: &[&str]) -> bool {
    let f = filter.trim().to_lowercase();
    if f.is_empty() {
        return true;
    }
    haystacks.iter().any(|h| h.to_lowercase().contains(&f))
}

fn is_expanded(overlay: &ReconOverlay, eid: &ReconExpandId, force: bool) -> bool {
    force || overlay.expanded.contains(eid)
}

pub fn build_visible_rows(overlay: &ReconOverlay) -> Vec<VisibleRow> {
    let Some(result) = overlay.recon_result.as_ref() else {
        return Vec::new();
    };
    let filter = overlay.filter.as_str();
    match overlay.active_tab {
        ReconTab::Tools => build_tools_rows(overlay, result, filter),
        ReconTab::Config => build_config_rows(overlay, result, filter),
        ReconTab::Sessions => build_sessions_rows(overlay, result, filter),
    }
}

fn build_tools_rows(
    overlay: &ReconOverlay,
    result: &common::ReconResult,
    filter: &str,
) -> Vec<VisibleRow> {
    let mut rows = Vec::new();
    let filtering = filter_active(filter);

    // MCP Servers
    {
        let servers: Vec<(usize, &common::McpServer)> = result
            .tools
            .mcp_servers
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                if !filtering {
                    return true;
                }
                matches_filter(filter, &[&s.name, &s.transport.to_string()])
                    || s.tools.iter().any(|t| {
                        matches_filter(filter, &[&t.name, &t.description])
                    })
            })
            .collect();

        if !filtering || !servers.is_empty() || matches_filter(filter, &["mcp", "servers"]) {
            let sec_id = ReconNodeId::ToolsSection(ToolsSectionKind::Mcp);
            let expanded = is_expanded(overlay, &ReconExpandId::ToolsMcp, filtering);
            rows.push(VisibleRow {
                id: sec_id,
                depth: 0,
                expandable: true,
                expanded,
                label: "MCP Servers".to_string(),
                meta: format!("({})", result.tools.mcp_servers.len()),
                badge: None,
                trailing: String::new(),
            });
            if expanded {
                for (si, server) in &servers {
                    let tool_matches: Vec<(usize, &common::AgentTool)> = server
                        .tools
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| {
                            !filtering
                                || matches_filter(filter, &[&t.name, &t.description, &server.name])
                        })
                        .collect();
                    let server_match = !filtering
                        || matches_filter(filter, &[&server.name, &server.transport.to_string()])
                        || !tool_matches.is_empty();
                    if !server_match {
                        continue;
                    }

                    let eid = ReconExpandId::McpServer(*si);
                    let expanded_s = is_expanded(overlay, &eid, filtering && !tool_matches.is_empty());
                    let (badge_label, badge_color) = if server.tools.is_empty() {
                        ("empty".to_string(), STATUS_RUNNING)
                    } else {
                        ("ok".to_string(), STATUS_DONE)
                    };
                    let trailing = server
                        .context_path
                        .as_ref()
                        .map(|p| truncate_path(p, 24))
                        .unwrap_or_else(|| {
                            if server.command.is_some() {
                                "(local)".to_string()
                            } else {
                                "(remote)".to_string()
                            }
                        });
                    rows.push(VisibleRow {
                        id: ReconNodeId::McpServer(*si),
                        depth: 1,
                        expandable: true,
                        expanded: expanded_s,
                        label: server.name.clone(),
                        meta: format!("{} tools", server.tools.len()),
                        badge: Some((badge_label, badge_color)),
                        trailing,
                    });
                    if expanded_s {
                        for (ti, tool) in &tool_matches {
                            rows.push(VisibleRow {
                                id: ReconNodeId::McpTool {
                                    server: *si,
                                    tool: *ti,
                                },
                                depth: 2,
                                expandable: false,
                                expanded: false,
                                label: tool.name.clone(),
                                meta: String::new(),
                                badge: None,
                                trailing: String::new(),
                            });
                        }
                    }
                }
            }
        }
    }

    // Skills
    {
        let skills: Vec<(usize, &common::AgentTool)> = result
            .tools
            .skills
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                !filtering || matches_filter(filter, &[&s.name, &s.description])
            })
            .collect();
        if !filtering || !skills.is_empty() || matches_filter(filter, &["skills"]) {
            let expanded = is_expanded(overlay, &ReconExpandId::ToolsSkills, filtering);
            rows.push(VisibleRow {
                id: ReconNodeId::ToolsSection(ToolsSectionKind::Skills),
                depth: 0,
                expandable: true,
                expanded,
                label: "Skills".to_string(),
                meta: format!("({})", result.tools.skills.len()),
                badge: None,
                trailing: String::new(),
            });
            if expanded {
                for (i, skill) in skills {
                    rows.push(VisibleRow {
                        id: ReconNodeId::Skill(i),
                        depth: 1,
                        expandable: false,
                        expanded: false,
                        label: skill.name.clone(),
                        meta: String::new(),
                        badge: None,
                        trailing: String::new(),
                    });
                }
            }
        }
    }

    // Internal
    {
        let tools: Vec<(usize, &common::AgentTool)> = result
            .tools
            .internal_tools
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                !filtering || matches_filter(filter, &[&t.name, &t.description])
            })
            .collect();
        if !filtering || !tools.is_empty() || matches_filter(filter, &["internal"]) {
            let expanded = is_expanded(overlay, &ReconExpandId::ToolsInternal, filtering);
            rows.push(VisibleRow {
                id: ReconNodeId::ToolsSection(ToolsSectionKind::Internal),
                depth: 0,
                expandable: true,
                expanded,
                label: "Internal".to_string(),
                meta: format!("({})", result.tools.internal_tools.len()),
                badge: None,
                trailing: String::new(),
            });
            if expanded {
                for (i, tool) in tools {
                    rows.push(VisibleRow {
                        id: ReconNodeId::Internal(i),
                        depth: 1,
                        expandable: false,
                        expanded: false,
                        label: tool.name.clone(),
                        meta: String::new(),
                        badge: None,
                        trailing: String::new(),
                    });
                }
            }
        }
    }

    rows
}

fn build_config_rows(
    overlay: &ReconOverlay,
    result: &common::ReconResult,
    filter: &str,
) -> Vec<VisibleRow> {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, item) in result.config.items.iter().enumerate() {
        if filter_active(filter)
            && !matches_filter(filter, &[&item.path, &item.config_type])
        {
            continue;
        }
        groups
            .entry(item.config_type.clone())
            .or_default()
            .push(i);
    }

    let mut rows = Vec::new();
    let filtering = filter_active(filter);
    for (ctype, indices) in groups {
        let eid = ReconExpandId::ConfigType(ctype.clone());
        let expanded = is_expanded(overlay, &eid, filtering);
        rows.push(VisibleRow {
            id: ReconNodeId::ConfigType(ctype.clone()),
            depth: 0,
            expandable: true,
            expanded,
            label: ctype,
            meta: format!("({})", indices.len()),
            badge: None,
            trailing: String::new(),
        });
        if expanded {
            for i in indices {
                let item = &result.config.items[i];
                rows.push(VisibleRow {
                    id: ReconNodeId::ConfigItem(i),
                    depth: 1,
                    expandable: false,
                    expanded: false,
                    label: truncate_path(&item.path, 48),
                    meta: String::new(),
                    badge: None,
                    trailing: String::new(),
                });
            }
        }
    }
    rows
}

fn build_sessions_rows(
    overlay: &ReconOverlay,
    result: &common::ReconResult,
    filter: &str,
) -> Vec<VisibleRow> {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, s) in result.sessions.items.iter().enumerate() {
        if filter_active(filter)
            && !matches_filter(
                filter,
                &[&s.session_id, &s.context_path, &s.session_file],
            )
        {
            continue;
        }
        let key = if s.context_path.is_empty() {
            "(no project)".to_string()
        } else {
            s.context_path.clone()
        };
        groups.entry(key).or_default().push(i);
    }

    // Sort sessions within each project by last_modified descending (string ISO works).
    for indices in groups.values_mut() {
        indices.sort_by(|a, b| {
            result.sessions.items[*b]
                .last_modified
                .cmp(&result.sessions.items[*a].last_modified)
        });
    }

    let mut rows = Vec::new();
    let filtering = filter_active(filter);
    for (project, indices) in groups {
        let eid = ReconExpandId::SessionProject(project.clone());
        let expanded = is_expanded(overlay, &eid, filtering);
        rows.push(VisibleRow {
            id: ReconNodeId::SessionProject(project.clone()),
            depth: 0,
            expandable: true,
            expanded,
            label: truncate_path(&project, 40),
            meta: format!("({})", indices.len()),
            badge: None,
            trailing: String::new(),
        });
        if expanded {
            for i in indices {
                let s = &result.sessions.items[i];
                let short = short_session_id(&s.session_id);
                let when = format_relative_ts(&s.last_modified);
                rows.push(VisibleRow {
                    id: ReconNodeId::SessionItem(i),
                    depth: 1,
                    expandable: false,
                    expanded: false,
                    label: short,
                    meta: format!("{} msgs", s.message_count),
                    badge: None,
                    trailing: when,
                });
            }
        }
    }
    rows
}

fn short_session_id(id: &str) -> String {
    let total = id.chars().count();
    if total > 12 {
        let prefix: String = id.chars().take(12).collect();
        format!("{}…", prefix)
    } else {
        id.to_string()
    }
}

fn truncate_path(path: &str, max: usize) -> String {
    let total = path.chars().count();
    if total <= max {
        path.to_string()
    } else {
        let skip = total - max.saturating_sub(1);
        let suffix: String = path.chars().skip(skip).collect();
        format!("…{}", suffix)
    }
}

fn format_relative_ts(iso: &str) -> String {
    if iso.is_empty() {
        return String::new();
    }
    let parsed = chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .or_else(|| chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%d %H:%M:%S").ok())
                .map(|ndt| ndt.and_utc())
        });
    let Some(dt) = parsed else {
        return iso.chars().take(16).collect();
    };
    let now = chrono::Utc::now();
    let secs = (now - dt).num_seconds().max(0) as u64;
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

//
// Clamp selection to a visible row and keep it in the scroll window.
//

pub fn sync_selection(overlay: &mut ReconOverlay, rows: &[VisibleRow], viewport_h: u16) {
    if rows.is_empty() {
        overlay.selected = None;
        overlay.tree_scroll = 0;
        return;
    }
    let idx = overlay
        .selected
        .as_ref()
        .and_then(|sel| rows.iter().position(|r| &r.id == sel))
        .unwrap_or(0);
    overlay.selected = Some(rows[idx].id.clone());

    let h = viewport_h as usize;
    if h == 0 {
        return;
    }
    if idx < overlay.tree_scroll as usize {
        overlay.tree_scroll = idx as u16;
    } else if idx >= overlay.tree_scroll as usize + h {
        overlay.tree_scroll = (idx + 1).saturating_sub(h) as u16;
    }
}

pub fn selected_visible_index(overlay: &ReconOverlay, rows: &[VisibleRow]) -> Option<usize> {
    overlay
        .selected
        .as_ref()
        .and_then(|sel| rows.iter().position(|r| &r.id == sel))
}

pub fn row_line(
    row: &VisibleRow,
    is_selected: bool,
    is_hovered: bool,
    width: u16,
) -> Line<'static> {
    let bg = if is_selected {
        Some(BG_SELECTED)
    } else if is_hovered {
        Some(ratatui::style::Color::Rgb(32, 36, 40))
    } else {
        None
    };

    let apply_bg = |s: Style| match bg {
        Some(c) => s.bg(c),
        None => s,
    };

    let mut prefix_style = apply_bg(Style::default().fg(if is_selected { ACCENT } else { MUTED }));
    let mut label_style = apply_bg(Style::default().fg(TEXT_BRIGHT));
    if is_selected {
        label_style = label_style.add_modifier(Modifier::BOLD);
    }
    let mut meta_style = apply_bg(Style::default().fg(DIM));
    let mut trail_style = apply_bg(Style::default().fg(MUTED));

    let cursor = if is_selected { "\u{276f} " } else { "  " };
    let indent = "  ".repeat(row.depth as usize);
    let chevron = if row.expandable {
        if row.expanded {
            "\u{25be} "
        } else {
            "\u{25b8} "
        }
    } else if row.depth > 0 {
        "  "
    } else {
        "  "
    };

    let mut spans = vec![
        Span::styled(cursor.to_string(), prefix_style),
        Span::styled(indent, apply_bg(Style::default())),
        Span::styled(chevron.to_string(), prefix_style),
        Span::styled(row.label.clone(), label_style),
    ];

    if let Some((ref badge, color)) = row.badge {
        spans.push(Span::styled(" ", apply_bg(Style::default())));
        spans.push(Span::styled(
            format!("[{}]", badge),
            apply_bg(Style::default().fg(color)),
        ));
    }

    if !row.meta.is_empty() {
        spans.push(Span::styled(
            format!("  {}", row.meta),
            meta_style,
        ));
    }

    //
    // Right-align trailing meta when width allows — approximate with
    // padding between content and trailing text.
    //

    if !row.trailing.is_empty() && width > 20 {
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let trail_len = row.trailing.chars().count();
        let pad = (width as usize).saturating_sub(used + trail_len + 1);
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), apply_bg(Style::default())));
            spans.push(Span::styled(row.trailing.clone(), trail_style));
        } else {
            spans.push(Span::styled(
                format!("  {}", row.trailing),
                trail_style,
            ));
        }
    }

    // Silence unused mut warnings when bg is None path
    let _ = (&mut prefix_style, &mut meta_style, &mut trail_style);

    Line::from(spans)
}

pub fn detail_title(overlay: &ReconOverlay) -> String {
    let Some(ref id) = overlay.selected else {
        return " Detail ".to_string();
    };
    let Some(result) = overlay.recon_result.as_ref() else {
        return " Detail ".to_string();
    };
    match id {
        ReconNodeId::ToolsSection(ToolsSectionKind::Mcp) => " MCP Servers ".to_string(),
        ReconNodeId::ToolsSection(ToolsSectionKind::Skills) => " Skills ".to_string(),
        ReconNodeId::ToolsSection(ToolsSectionKind::Internal) => " Internal Tools ".to_string(),
        ReconNodeId::McpServer(i) => result
            .tools
            .mcp_servers
            .get(*i)
            .map(|s| format!(" {} ", s.name))
            .unwrap_or_else(|| " MCP Server ".to_string()),
        ReconNodeId::McpTool { server, tool } => result
            .tools
            .mcp_servers
            .get(*server)
            .and_then(|s| s.tools.get(*tool))
            .map(|t| format!(" {} ", t.name))
            .unwrap_or_else(|| " Tool ".to_string()),
        ReconNodeId::Skill(i) => result
            .tools
            .skills
            .get(*i)
            .map(|s| format!(" {} ", s.name))
            .unwrap_or_else(|| " Skill ".to_string()),
        ReconNodeId::Internal(i) => result
            .tools
            .internal_tools
            .get(*i)
            .map(|t| format!(" {} ", t.name))
            .unwrap_or_else(|| " Tool ".to_string()),
        ReconNodeId::ConfigType(t) => format!(" {} ", t),
        ReconNodeId::ConfigItem(i) => result
            .config
            .items
            .get(*i)
            .map(|c| format!(" {} ", c.path))
            .unwrap_or_else(|| " Config ".to_string()),
        ReconNodeId::SessionProject(p) => format!(" {} ", p),
        ReconNodeId::SessionItem(i) => result
            .sessions
            .items
            .get(*i)
            .map(|s| format!(" {} ", s.session_id))
            .unwrap_or_else(|| " Session ".to_string()),
    }
}

pub fn tools_detail_lines(overlay: &ReconOverlay) -> Vec<Line<'static>> {
    let Some(ref id) = overlay.selected else {
        return vec![Line::from(Span::styled(
            " Select an item",
            Style::default().fg(DIM),
        ))];
    };
    let Some(result) = overlay.recon_result.as_ref() else {
        return vec![];
    };

    match id {
        ReconNodeId::ToolsSection(ToolsSectionKind::Mcp) => {
            let n = result.tools.mcp_servers.len();
            let tools = result.tools.mcp_tool_count();
            vec![
                Line::from(Span::styled(
                    format!(" {} MCP server{}", n, if n == 1 { "" } else { "s" }),
                    Style::default().fg(TEXT_BRIGHT),
                )),
                Line::from(Span::styled(
                    format!(" {} tool{} discovered across servers", tools, if tools == 1 { "" } else { "s" }),
                    Style::default().fg(MUTED),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " Expand a server to browse its tools.",
                    Style::default().fg(DIM),
                )),
            ]
        }
        ReconNodeId::ToolsSection(ToolsSectionKind::Skills) => {
            summary_list(
                "skill",
                result.tools.skills.len(),
                "Slash-command style skills available to the agent.",
            )
        }
        ReconNodeId::ToolsSection(ToolsSectionKind::Internal) => {
            let n = result.tools.internal_tools.len();
            let mut lines = summary_list(
                "internal tool",
                n,
                "Built-in agent tools (usually filled by semantic recon).",
            );
            if n == 0 {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " Run Discover (^d) for semantic recon.",
                    Style::default().fg(STATUS_RUNNING),
                )));
            }
            lines
        }
        ReconNodeId::McpServer(i) => {
            let Some(server) = result.tools.mcp_servers.get(*i) else {
                return empty_detail();
            };
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(" Name  ", Style::default().fg(MUTED)),
                    Span::styled(server.name.clone(), Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled(" Transport  ", Style::default().fg(MUTED)),
                    Span::styled(server.transport.to_string(), Style::default().fg(TEXT)),
                ]),
            ];
            if let Some(ref cmd) = server.command {
                lines.push(Line::from(vec![
                    Span::styled(" Command  ", Style::default().fg(MUTED)),
                    Span::styled(cmd.clone(), Style::default().fg(DIM)),
                ]));
            }
            if let Some(ref addr) = server.address {
                lines.push(Line::from(vec![
                    Span::styled(" Address  ", Style::default().fg(MUTED)),
                    Span::styled(addr.clone(), Style::default().fg(DIM)),
                ]));
            }
            if let Some(ref ctx) = server.context_path {
                lines.push(Line::from(vec![
                    Span::styled(" Context  ", Style::default().fg(MUTED)),
                    Span::styled(ctx.clone(), Style::default().fg(DIM)),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!(" Tools ({})", server.tools.len()),
                Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD),
            )));
            if server.tools.is_empty() {
                lines.push(Line::from(Span::styled(
                    " No tools discovered for this server.",
                    Style::default().fg(STATUS_FAIL),
                )));
            } else {
                for t in &server.tools {
                    lines.push(Line::from(vec![
                        Span::styled("  \u{2022} ", Style::default().fg(ACCENT)),
                        Span::styled(t.name.clone(), Style::default().fg(TEXT_BRIGHT)),
                    ]));
                }
            }
            lines
        }
        ReconNodeId::McpTool { server, tool } => {
            let Some(t) = result
                .tools
                .mcp_servers
                .get(*server)
                .and_then(|s| s.tools.get(*tool))
            else {
                return empty_detail();
            };
            let server_name = result
                .tools
                .mcp_servers
                .get(*server)
                .map(|s| s.name.as_str())
                .unwrap_or("?");
            tool_detail_lines(&t.name, &t.description, Some(server_name), t.context_path.as_deref())
        }
        ReconNodeId::Skill(i) => {
            let Some(t) = result.tools.skills.get(*i) else {
                return empty_detail();
            };
            tool_detail_lines(&t.name, &t.description, None, t.context_path.as_deref())
        }
        ReconNodeId::Internal(i) => {
            let Some(t) = result.tools.internal_tools.get(*i) else {
                return empty_detail();
            };
            tool_detail_lines(&t.name, &t.description, None, t.context_path.as_deref())
        }
        _ => empty_detail(),
    }
}

fn tool_detail_lines(
    name: &str,
    description: &str,
    server: Option<&str>,
    context: Option<&str>,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!(" {}", name),
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    ))];
    if let Some(s) = server {
        lines.push(Line::from(vec![
            Span::styled(" Server  ", Style::default().fg(MUTED)),
            Span::styled(s.to_string(), Style::default().fg(TEXT)),
        ]));
    }
    if let Some(c) = context {
        lines.push(Line::from(vec![
            Span::styled(" Context  ", Style::default().fg(MUTED)),
            Span::styled(c.to_string(), Style::default().fg(DIM)),
        ]));
    }
    lines.push(Line::from(""));
    if description.is_empty() {
        lines.push(Line::from(Span::styled(
            " No description.",
            Style::default().fg(DIM),
        )));
    } else {
        for line in description.lines() {
            lines.push(Line::from(Span::styled(
                format!(" {}", line),
                Style::default().fg(TEXT),
            )));
        }
    }
    lines
}

fn summary_list(singular: &str, n: usize, blurb: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            format!(
                " {} {}{}",
                n,
                singular,
                if n == 1 { "" } else { "s" }
            ),
            Style::default().fg(TEXT_BRIGHT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(" {}", blurb),
            Style::default().fg(MUTED),
        )),
    ]
}

fn empty_detail() -> Vec<Line<'static>> {
    vec![Line::from(Span::styled(
        " Select an item",
        Style::default().fg(DIM),
    ))]
}

pub fn config_type_detail_lines(overlay: &ReconOverlay) -> Vec<Line<'static>> {
    let Some(ReconNodeId::ConfigType(ref t)) = overlay.selected else {
        return empty_detail();
    };
    let Some(result) = overlay.recon_result.as_ref() else {
        return empty_detail();
    };
    let count = result
        .config
        .items
        .iter()
        .filter(|i| &i.config_type == t)
        .count();
    vec![
        Line::from(Span::styled(
            format!(" {} config files of type \"{}\"", count, t),
            Style::default().fg(TEXT_BRIGHT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Expand and select a file to view its contents.",
            Style::default().fg(DIM),
        )),
    ]
}

pub fn session_project_detail_lines(overlay: &ReconOverlay) -> Vec<Line<'static>> {
    let Some(ReconNodeId::SessionProject(ref p)) = overlay.selected else {
        return empty_detail();
    };
    let Some(result) = overlay.recon_result.as_ref() else {
        return empty_detail();
    };
    let count = result
        .sessions
        .items
        .iter()
        .filter(|s| {
            let key = if s.context_path.is_empty() {
                "(no project)"
            } else {
                s.context_path.as_str()
            };
            key == p
        })
        .count();
    vec![
        Line::from(Span::styled(
            format!(" {} session{} under", count, if count == 1 { "" } else { "s" }),
            Style::default().fg(TEXT_BRIGHT),
        )),
        Line::from(Span::styled(
            format!(" {}", p),
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Expand and select a session to view the transcript.",
            Style::default().fg(DIM),
        )),
    ]
}
