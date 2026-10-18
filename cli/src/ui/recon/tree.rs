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
        ReconNodeId::ToolsSection(kind) => Some(ReconExpandId::ToolsSection(*kind)),
        ReconNodeId::McpServer(i) => Some(ReconExpandId::McpServer(*i)),
        ReconNodeId::ConfigType(t) => Some(ReconExpandId::ConfigType(t.clone())),
        ReconNodeId::CredentialsGroup => Some(ReconExpandId::Credentials),
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
    e.insert(ReconExpandId::ToolsSection(ToolsSectionKind::Mcp));
    for (kind, n) in [
        (ToolsSectionKind::Skills, result.tools.skills.len()),
        (ToolsSectionKind::Hooks, result.tools.hooks.len()),
        (ToolsSectionKind::Plugins, result.tools.plugins.len()),
        (
            ToolsSectionKind::Marketplaces,
            result.tools.marketplaces.len(),
        ),
    ] {
        if n <= 8 {
            e.insert(ReconExpandId::ToolsSection(kind));
        }
    }
    if result.config.credentials.len() <= 12 {
        e.insert(ReconExpandId::Credentials);
    }
    // Internal often long / semantic-only — start collapsed.

    //
    // Expand small config groups by default; large ones (e.g. dozens of
    // project_instructions paths) stay collapsed so the tree stays
    // scannable. Group key normalises `type: /path` → `type`.
    //
    let mut type_counts: BTreeMap<String, usize> = BTreeMap::new();
    for item in &result.config.items {
        *type_counts
            .entry(config_group_key(&item.config_type))
            .or_default() += 1;
    }
    for (t, n) in type_counts {
        if n <= 12 {
            e.insert(ReconExpandId::ConfigType(t));
        }
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
                    || s.tools
                        .iter()
                        .any(|t| matches_filter(filter, &[&t.name, &t.description]))
            })
            .collect();

        if !filtering || !servers.is_empty() || matches_filter(filter, &["mcp", "servers"]) {
            let sec_id = ReconNodeId::ToolsSection(ToolsSectionKind::Mcp);
            let expanded = is_expanded(
                overlay,
                &ReconExpandId::ToolsSection(ToolsSectionKind::Mcp),
                filtering,
            );
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
                    let expanded_s =
                        is_expanded(overlay, &eid, filtering && !tool_matches.is_empty());
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

    let tools = &result.tools;
    let skills = tools
        .skills
        .iter()
        .map(|t| leaf(&t.name, String::new(), &[&t.name, &t.description]))
        .collect();
    push_flat_section(&mut rows, overlay, filter, ToolsSectionKind::Skills, skills);

    let hooks = tools
        .hooks
        .iter()
        .map(|h| {
            let label = match h.matcher.as_deref() {
                Some(m) => format!("{} [{}]", h.event, m),
                None => h.event.clone(),
            };
            leaf(
                &label,
                h.scope.clone(),
                &[&h.event, &h.command, &h.scope, &h.source_path],
            )
        })
        .collect();
    push_flat_section(&mut rows, overlay, filter, ToolsSectionKind::Hooks, hooks);

    let plugins = tools
        .plugins
        .iter()
        .map(|p| {
            let trailing = if p.enabled {
                p.scope.clone()
            } else {
                "disabled".to_string()
            };
            leaf(
                &p.name,
                trailing,
                &[&p.name, p.source.as_deref().unwrap_or(""), &p.path],
            )
        })
        .collect();
    push_flat_section(
        &mut rows,
        overlay,
        filter,
        ToolsSectionKind::Plugins,
        plugins,
    );

    let marketplaces = tools
        .marketplaces
        .iter()
        .map(|m| leaf(&m.name, truncate_path(&m.source, 32), &[&m.name, &m.source]))
        .collect();
    push_flat_section(
        &mut rows,
        overlay,
        filter,
        ToolsSectionKind::Marketplaces,
        marketplaces,
    );

    let internal = tools
        .internal_tools
        .iter()
        .map(|t| leaf(&t.name, String::new(), &[&t.name, &t.description]))
        .collect();
    push_flat_section(
        &mut rows,
        overlay,
        filter,
        ToolsSectionKind::Internal,
        internal,
    );

    rows
}

//
// One leaf row in a flat Tools section: label, trailing hint and the
// strings the filter matches against.
//

struct Leaf {
    label: String,
    trailing: String,
    haystack: Vec<String>,
}

fn leaf(label: &str, trailing: String, haystack: &[&str]) -> Leaf {
    Leaf {
        label: label.to_string(),
        trailing,
        haystack: haystack.iter().map(|h| h.to_string()).collect(),
    }
}

fn section_label(kind: ToolsSectionKind) -> &'static str {
    match kind {
        ToolsSectionKind::Mcp => "MCP Servers",
        ToolsSectionKind::Skills => "Skills",
        ToolsSectionKind::Hooks => "Hooks",
        ToolsSectionKind::Plugins => "Plugins",
        ToolsSectionKind::Marketplaces => "Marketplaces",
        ToolsSectionKind::Internal => "Internal",
    }
}

fn leaf_id(kind: ToolsSectionKind, i: usize) -> ReconNodeId {
    match kind {
        ToolsSectionKind::Skills => ReconNodeId::Skill(i),
        ToolsSectionKind::Hooks => ReconNodeId::Hook(i),
        ToolsSectionKind::Plugins => ReconNodeId::Plugin(i),
        ToolsSectionKind::Marketplaces => ReconNodeId::Marketplace(i),
        ToolsSectionKind::Mcp => ReconNodeId::McpServer(i),
        ToolsSectionKind::Internal => ReconNodeId::Internal(i),
    }
}

fn push_flat_section(
    rows: &mut Vec<VisibleRow>,
    overlay: &ReconOverlay,
    filter: &str,
    kind: ToolsSectionKind,
    leaves: Vec<Leaf>,
) {
    let filtering = filter_active(filter);
    let total = leaves.len();
    let label = section_label(kind);
    let matching: Vec<(usize, Leaf)> = leaves
        .into_iter()
        .enumerate()
        .filter(|(_, l)| {
            let hay: Vec<&str> = l.haystack.iter().map(String::as_str).collect();
            !filtering || matches_filter(filter, &hay)
        })
        .collect();
    if filtering && matching.is_empty() && !matches_filter(filter, &[label]) {
        return;
    }
    let expanded = is_expanded(overlay, &ReconExpandId::ToolsSection(kind), filtering);
    rows.push(VisibleRow {
        id: ReconNodeId::ToolsSection(kind),
        depth: 0,
        expandable: true,
        expanded,
        label: label.to_string(),
        meta: format!("({})", total),
        badge: None,
        trailing: String::new(),
    });
    if expanded {
        for (i, l) in matching {
            rows.push(VisibleRow {
                id: leaf_id(kind, i),
                depth: 1,
                expandable: false,
                expanded: false,
                label: l.label,
                meta: String::new(),
                badge: None,
                trailing: l.trailing,
            });
        }
    }
}

//
// Connectors sometimes encode the project path into config_type
// (`project_instructions: /home/...`). Collapse those under a single
// group keyed by the type prefix so the tree is scannable.
//
fn config_group_key(config_type: &str) -> String {
    common::config_type_group(config_type).to_string()
}

fn config_item_in_group(item: &common::ConfigItem, group: &str) -> bool {
    config_group_key(&item.config_type) == group
}

/// Prefer a path-like suffix from config_type when the file path alone
/// is less informative (rare); otherwise show the file path.
fn config_item_label(item: &common::ConfigItem) -> String {
    if let Some((_, rest)) = item.config_type.split_once(':') {
        let rest = rest.trim();
        if (rest.starts_with('/') || rest.starts_with('~') || rest.starts_with('.'))
            && !item.path.is_empty()
            && item.path != rest
        {
            //
            // Type carries a project root and path is the file — show
            // the file path (truncated). Project root is implied by
            // belonging to this group.
            //
            return truncate_path(&item.path, 48);
        }
        if item.path.is_empty() && !rest.is_empty() {
            return truncate_path(rest, 48);
        }
    }
    if item.path.is_empty() {
        item.config_type.clone()
    } else {
        truncate_path(&item.path, 48)
    }
}

fn build_config_rows(
    overlay: &ReconOverlay,
    result: &common::ReconResult,
    filter: &str,
) -> Vec<VisibleRow> {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, item) in result.config.items.iter().enumerate() {
        if filter_active(filter) && !matches_filter(filter, &[&item.path, &item.config_type]) {
            continue;
        }
        groups
            .entry(config_group_key(&item.config_type))
            .or_default()
            .push(i);
    }

    //
    // Stable order within a group: path, then original config_type so
    // same-path variants stay deterministic.
    //
    for indices in groups.values_mut() {
        indices.sort_by(|&a, &b| {
            let ia = &result.config.items[a];
            let ib = &result.config.items[b];
            ia.path
                .cmp(&ib.path)
                .then_with(|| ia.config_type.cmp(&ib.config_type))
        });
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
                    label: config_item_label(item),
                    meta: String::new(),
                    badge: None,
                    trailing: String::new(),
                });
            }
        }
    }

    //
    // Credential stores are metadata-only, so they live in their own
    // group with no file content behind the rows.
    //
    let creds: Vec<(usize, &common::CredentialItem)> = result
        .config
        .credentials
        .iter()
        .enumerate()
        .filter(|(_, c)| !filtering || matches_filter(filter, &[&c.path, &c.kind, "credentials"]))
        .collect();
    if !creds.is_empty() {
        let expanded = is_expanded(overlay, &ReconExpandId::Credentials, filtering);
        rows.push(VisibleRow {
            id: ReconNodeId::CredentialsGroup,
            depth: 0,
            expandable: true,
            expanded,
            label: "credentials".to_string(),
            meta: format!("({})", result.config.credentials.len()),
            badge: None,
            trailing: "metadata only".to_string(),
        });
        if expanded {
            for (i, c) in creds {
                let (badge, color) = match c.exists {
                    Some(true) => ("present", STATUS_DONE),
                    Some(false) => ("absent", MUTED),
                    None => ("keychain", STATUS_RUNNING),
                };
                rows.push(VisibleRow {
                    id: ReconNodeId::Credential(i),
                    depth: 1,
                    expandable: false,
                    expanded: false,
                    label: truncate_path(&c.path, 48),
                    meta: String::new(),
                    badge: Some((badge.to_string(), color)),
                    trailing: c.kind.clone(),
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
            && !matches_filter(filter, &[&s.session_id, &s.context_path, &s.session_file])
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
        spans.push(Span::styled(format!("  {}", row.meta), meta_style));
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
            spans.push(Span::styled(format!("  {}", row.trailing), trail_style));
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
        ReconNodeId::ToolsSection(ToolsSectionKind::Internal) => " Internal Tools ".to_string(),
        ReconNodeId::ToolsSection(kind) => format!(" {} ", section_label(*kind)),
        ReconNodeId::Hook(i) => result
            .tools
            .hooks
            .get(*i)
            .map(|h| format!(" {} ", h.event))
            .unwrap_or_else(|| " Hook ".to_string()),
        ReconNodeId::Plugin(i) => result
            .tools
            .plugins
            .get(*i)
            .map(|p| format!(" {} ", p.name))
            .unwrap_or_else(|| " Plugin ".to_string()),
        ReconNodeId::Marketplace(i) => result
            .tools
            .marketplaces
            .get(*i)
            .map(|m| format!(" {} ", m.name))
            .unwrap_or_else(|| " Marketplace ".to_string()),
        ReconNodeId::CredentialsGroup => " Credentials ".to_string(),
        ReconNodeId::Credential(i) => result
            .config
            .credentials
            .get(*i)
            .map(|c| format!(" {} ", c.path))
            .unwrap_or_else(|| " Credential ".to_string()),
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
                    format!(
                        " {} tool{} discovered across servers",
                        tools,
                        if tools == 1 { "" } else { "s" }
                    ),
                    Style::default().fg(MUTED),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " Expand a server to browse its tools.",
                    Style::default().fg(DIM),
                )),
            ]
        }
        ReconNodeId::ToolsSection(ToolsSectionKind::Skills) => summary_list(
            "skill",
            result.tools.skills.len(),
            "Slash commands and SKILL.md skills (agent-specific and shared .agents/skills dirs).",
        ),
        ReconNodeId::ToolsSection(ToolsSectionKind::Hooks) => summary_list(
            "hook",
            result.tools.hooks.len(),
            "Lifecycle hooks from user, project, managed and plugin scopes.",
        ),
        ReconNodeId::ToolsSection(ToolsSectionKind::Plugins) => summary_list(
            "plugin",
            result.tools.plugins.len(),
            "Installed plugins / extensions.",
        ),
        ReconNodeId::ToolsSection(ToolsSectionKind::Marketplaces) => summary_list(
            "marketplace",
            result.tools.marketplaces.len(),
            "Configured plugin marketplace sources.",
        ),
        ReconNodeId::Hook(i) => {
            let Some(h) = result.tools.hooks.get(*i) else {
                return empty_detail();
            };
            field_lines(&[
                ("Event", Some(h.event.as_str())),
                ("Matcher", h.matcher.as_deref()),
                ("Type", h.hook_type.as_deref()),
                ("Command", Some(h.command.as_str())),
                ("Scope", Some(h.scope.as_str())),
                ("Source", Some(h.source_path.as_str())),
                ("Context", h.context_path.as_deref()),
            ])
        }
        ReconNodeId::Plugin(i) => {
            let Some(p) = result.tools.plugins.get(*i) else {
                return empty_detail();
            };
            field_lines(&[
                ("Name", Some(p.name.as_str())),
                ("Version", p.version.as_deref()),
                ("Source", p.source.as_deref()),
                ("Enabled", Some(if p.enabled { "yes" } else { "no" })),
                ("Scope", Some(p.scope.as_str())),
                ("Path", Some(p.path.as_str())),
                ("Context", p.context_path.as_deref()),
            ])
        }
        ReconNodeId::Marketplace(i) => {
            let Some(m) = result.tools.marketplaces.get(*i) else {
                return empty_detail();
            };
            field_lines(&[
                ("Name", Some(m.name.as_str())),
                ("Source", Some(m.source.as_str())),
                ("Scope", Some(m.scope.as_str())),
                ("Declared in", Some(m.path.as_str())),
                ("Context", m.context_path.as_deref()),
            ])
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
                    Span::styled(
                        server.name.clone(),
                        Style::default()
                            .fg(TEXT_BRIGHT)
                            .add_modifier(Modifier::BOLD),
                    ),
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
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
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
            tool_detail_lines(
                &t.name,
                &t.description,
                Some(server_name),
                t.context_path.as_deref(),
            )
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

//
// Label/value detail block; empty or missing values are skipped.
//

fn field_lines(fields: &[(&str, Option<&str>)]) -> Vec<Line<'static>> {
    fields
        .iter()
        .filter_map(|(label, value)| {
            let value = value.filter(|v| !v.is_empty())?;
            Some(Line::from(vec![
                Span::styled(format!(" {}  ", label), Style::default().fg(MUTED)),
                Span::styled(value.to_string(), Style::default().fg(TEXT)),
            ]))
        })
        .collect()
}

pub fn credential_detail_lines(overlay: &ReconOverlay) -> Vec<Line<'static>> {
    let Some(result) = overlay.recon_result.as_ref() else {
        return empty_detail();
    };
    match overlay.selected {
        Some(ReconNodeId::Credential(i)) => {
            let Some(c) = result.config.credentials.get(i) else {
                return empty_detail();
            };
            let exists = match c.exists {
                Some(true) => "yes",
                Some(false) => "no",
                None => "unknown (not inspected)",
            };
            let mut lines = field_lines(&[
                ("Path", Some(c.path.as_str())),
                ("Kind", Some(c.kind.as_str())),
                ("Exists", Some(exists)),
                ("Modified", c.last_modified.as_deref()),
            ]);
            if !c.metadata.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " Metadata",
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                )));
                let pairs: Vec<(&str, Option<&str>)> = c
                    .metadata
                    .iter()
                    .map(|(k, v)| (k.as_str(), Some(v.as_str())))
                    .collect();
                lines.extend(field_lines(&pairs));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Secret values are never collected.",
                Style::default().fg(DIM),
            )));
            lines
        }
        _ => summary_list(
            "credential store",
            result.config.credentials.len(),
            "Where the agent keeps credentials (metadata only, secrets redacted on the node).",
        ),
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
            format!(" {} {}{}", n, singular, if n == 1 { "" } else { "s" }),
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
        .filter(|i| config_item_in_group(i, t))
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
            format!(
                " {} session{} under",
                count,
                if count == 1 { "" } else { "s" }
            ),
            Style::default().fg(TEXT_BRIGHT),
        )),
        Line::from(Span::styled(format!(" {}", p), Style::default().fg(MUTED))),
        Line::from(""),
        Line::from(Span::styled(
            " Expand and select a session to view the transcript.",
            Style::default().fg(DIM),
        )),
    ]
}
