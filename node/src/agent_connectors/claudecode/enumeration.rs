use crate::agent_connectors::utils::{
    collect_global_config_files, enumerate_user_homes, scan_directories_for_config_files_multi,
    ConfigFilePattern, GlobalConfigPattern,
};
use common::{ConfigItem, SessionItem};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_SCAN_DEPTH: usize = 7;

const GLOBAL_CONFIG_PATTERNS: &[GlobalConfigPattern] = &[
    GlobalConfigPattern { path: ".claude/settings.json", config_type: "global_settings" },
    GlobalConfigPattern { path: ".claude.json", config_type: "preferences" },
    GlobalConfigPattern { path: ".claude/CLAUDE.md", config_type: "global_instructions" },
];

const PROJECT_CONFIG_PATTERNS: &[ConfigFilePattern] = &[
    ConfigFilePattern { filename: "settings.json", parent_dir: Some(".claude"), config_type_prefix: "project_settings" },
    ConfigFilePattern { filename: "settings.local.json", parent_dir: Some(".claude"), config_type_prefix: "project_settings_local" },
    ConfigFilePattern { filename: "CLAUDE.md", parent_dir: None, config_type_prefix: "project_instructions" },
    ConfigFilePattern { filename: ".mcp.json", parent_dir: None, config_type_prefix: "project_mcp" },
];

pub struct EnumerationData {
    pub config_items: Vec<ConfigItem>,
    pub sessions: Vec<SessionItem>,
    pub project_paths: Vec<String>,
}

//
// Check if authentication environment variables are set.
//

pub fn has_auth_env_vars() -> bool {
    std::env::var("ANTHROPIC_API_KEY").is_ok()
        || std::env::var("ANTHROPIC_AUTH_TOKEN").is_ok()
        || std::env::var("ANTHROPIC_FOUNDRY_API_KEY").is_ok()
        || std::env::var("AWS_BEARER_TOKEN_BEDROCK").is_ok()
}

//
// Check if a .claude.json file has authentication configured.
// Looks for oauthAccount, primaryApiKey, or apiKeyHelper fields.
//

fn has_auth_in_claude_json(claude_json_path: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(claude_json_path) else {
        return false;
    };

    let Ok(json) = serde_json::from_str::<Value>(&contents) else {
        return false;
    };

    //
    // Check for auth-related fields in .claude.json.
    //

    json.get("oauthAccount").is_some()
        || json.get("primaryApiKey").is_some()
        || json.get("apiKeyHelper").is_some()
}

//
// Check if a path (user home or project) has valid Claude Code authentication.
// Auth can come from:
// 1. Environment variables (global)
// 2. Auth in the path's own .claude.json (for user homes)
// 3. Auth in the owning user's home .claude.json (for project paths)
//

pub fn path_has_valid_auth(path: &Path, user_homes: &[PathBuf]) -> bool {
    //
    // Global env vars take precedence.
    //

    if has_auth_env_vars() {
        return true;
    }

    //
    // Check path's own .claude.json (for user homes).
    //

    let path_claude_json = path.join(".claude.json");
    if has_auth_in_claude_json(&path_claude_json) {
        return true;
    }

    //
    // For project paths, check if the owning user's home has auth configured.
    // Find which user home is a parent of this path.
    //

    let path_str = path.to_string_lossy();
    for home in user_homes {
        let home_str = home.to_string_lossy();
        if path_str.starts_with(home_str.as_ref()) {
            let home_claude_json = home.join(".claude.json");
            if has_auth_in_claude_json(&home_claude_json) {
                return true;
            }
        }
    }

    false
}

pub fn enumerate() -> anyhow::Result<EnumerationData> {
    common::log_debug!("Enumerating Claude Code configurations across all users");

    let mut config_items = Vec::new();
    let mut sessions = Vec::new();
    let mut project_paths_set = HashSet::new();

    let user_homes = enumerate_user_homes();
    let user_homes_set: HashSet<&Path> = user_homes.iter().map(|p| p.as_path()).collect();

    //
    // Collect global config files from all user homes.
    //

    config_items.extend(collect_global_config_files(&user_homes, GLOBAL_CONFIG_PATTERNS));

    for home in &user_homes {
        discover_sessions(home, &mut sessions)?;
    }

    //
    // Collect project-level config files.
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

fn discover_sessions(home: &Path, sessions: &mut Vec<SessionItem>) -> anyhow::Result<()> {
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

        //
        // Extract project hash from directory name.
        //

        let project_hash = entry.file_name().to_string_lossy().to_string();

        //
        // Look for session files directly in the project directory.
        // Claude Code stores sessions as *.jsonl files in the project dir.
        //

        for session_entry in fs::read_dir(&project_path)? {
            let session_entry = session_entry?;
            let session_path = session_entry.path();

            if session_path.extension().map_or(false, |e| e == "jsonl") {
                if let Some(session_info) = parse_session_file(&session_path, &project_hash) {
                    sessions.push(session_info);
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    Ok(())
}

fn parse_session_file(path: &Path, project_hash: &str) -> Option<SessionItem> {
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

    //
    // Count lines without loading entire file into memory. Session files can be
    // very large and including content would exceed RabbitMQ message limits.
    //

    let message_count = if let Ok(file) = std::fs::File::open(path) {
        std::io::BufRead::lines(std::io::BufReader::new(file))
            .filter_map(|l| l.ok())
            .filter(|l| !l.trim().is_empty())
            .count()
    } else {
        0
    };

    Some(SessionItem {
        session_id: file_name,
        context_path: project_hash.to_string(),
        session_file: path.to_string_lossy().to_string(),
        last_modified: last_modified_dt.to_rfc3339(),
        message_count,
        content: None,
    })
}
