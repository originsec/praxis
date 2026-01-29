use crate::agent_connectors::utils::{
    enumerate_user_homes, scan_directories_for_config_files_multi, ConfigFilePattern,
};
use common::{AgentSessionInfo, ConfigItem};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const MAX_SCAN_DEPTH: usize = 7;

const PROJECT_CONFIG_PATTERNS: &[ConfigFilePattern] = &[
    ConfigFilePattern { filename: "settings.json", parent_dir: Some(".claude"), config_type_prefix: "project_settings" },
    ConfigFilePattern { filename: "settings.local.json", parent_dir: Some(".claude"), config_type_prefix: "project_settings_local" },
    ConfigFilePattern { filename: "CLAUDE.md", parent_dir: None, config_type_prefix: "project_instructions" },
    ConfigFilePattern { filename: ".mcp.json", parent_dir: None, config_type_prefix: "project_mcp" },
];

pub struct EnumerationData {
    pub config_items: Vec<ConfigItem>,
    pub sessions: Vec<AgentSessionInfo>,
    pub project_paths: Vec<String>,
}

pub fn enumerate() -> anyhow::Result<EnumerationData> {
    common::log_debug!("Enumerating Claude Code configurations across all users");

    let mut config_items = Vec::new();
    let mut sessions = Vec::new();
    let mut project_paths_set = HashSet::new();

    //
    // Collect from all user homes.
    //

    let user_homes = enumerate_user_homes();
    let user_homes_set: HashSet<&Path> = user_homes.iter().map(|p| p.as_path()).collect();

    for home in &user_homes {
        collect_config_file(&home.join(".claude/settings.json"), "global_settings", &mut config_items);
        collect_config_file(&home.join(".claude.json"), "preferences", &mut config_items);
        collect_config_file(&home.join(".claude/CLAUDE.md"), "global_instructions", &mut config_items);

        discover_sessions(home, &mut sessions)?;
    }

    //
    // Scan for project-level config files.
    //

    let project_configs = scan_directories_for_config_files_multi(
        &user_homes,
        PROJECT_CONFIG_PATTERNS,
        &user_homes_set,
        &mut project_paths_set,
        MAX_SCAN_DEPTH,
    );
    config_items.extend(project_configs);

    let mut project_paths: Vec<String> = project_paths_set.into_iter().collect();
    project_paths.sort();

    common::log_info!(
        "Claude Code enumeration complete: {} configs, {} sessions, {} projects",
        config_items.len(),
        sessions.len(),
        project_paths.len()
    );

    Ok(EnumerationData {
        config_items,
        sessions,
        project_paths,
    })
}

fn collect_config_file(path: &Path, config_type: &str, config_items: &mut Vec<ConfigItem>) {
    if let Ok(contents) = fs::read_to_string(path) {
        config_items.push(ConfigItem {
            path: path.to_string_lossy().to_string(),
            contents,
            config_type: config_type.to_string(),
        });
    }
}

fn discover_sessions(home: &Path, sessions: &mut Vec<AgentSessionInfo>) -> anyhow::Result<()> {
    let projects_dir = home.join(".claude/projects");
    if !projects_dir.exists() {
        return Ok(());
    }

    //
    // Iterate over project directories (hashed names).
    //
    for entry in fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let project_path = entry.path();
        if !project_path.is_dir() {
            continue;
        }

        let sessions_dir = project_path.join("sessions");
        if !sessions_dir.exists() {
            continue;
        }

        //
        // Extract project hash from directory name.
        //
        let project_hash = entry.file_name().to_string_lossy().to_string();

        //
        // Look for session files.
        //
        for session_entry in fs::read_dir(&sessions_dir)? {
            let session_entry = session_entry?;
            let session_path = session_entry.path();

            if session_path.extension().map_or(false, |e| e == "jsonl") {
                if let Some(session_info) = parse_session_file(&session_path, &project_hash) {
                    sessions.push(session_info);
                }
            }
        }
    }

    //
    // Sort by last modified (most recent first).
    //
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    Ok(())
}

fn parse_session_file(path: &Path, project_hash: &str) -> Option<AgentSessionInfo> {
    let file_name = path.file_stem()?.to_string_lossy().to_string();

    //
    // Get file metadata for last modified.
    //
    let metadata = fs::metadata(path).ok()?;
    let last_modified = metadata.modified().ok()?;
    let last_modified_dt: DateTime<Utc> = last_modified.into();

    //
    // Count lines (messages) in the JSONL file.
    //
    let content = fs::read_to_string(path).ok()?;
    let message_count = content.lines().filter(|l| !l.trim().is_empty()).count();

    Some(AgentSessionInfo {
        session_id: file_name,
        context_path: project_hash.to_string(),
        session_file: path.to_string_lossy().to_string(),
        last_modified: last_modified_dt.to_rfc3339(),
        message_count,
    })
}
