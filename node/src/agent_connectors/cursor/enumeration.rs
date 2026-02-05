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
use std::process::{Command, Stdio};
use std::sync::OnceLock;

const MAX_SCAN_DEPTH: usize = 7;

//
// Global config patterns - files in user home directories.
//

const GLOBAL_CONFIG_PATTERNS: &[GlobalConfigPattern] = &[
    GlobalConfigPattern {
        path: ".cursor/cli-config.json",
        config_type: "global_settings",
    },
];

//
// Project-level config patterns - files in project directories.
//

const PROJECT_CONFIG_PATTERNS: &[ConfigFilePattern] = &[
    ConfigFilePattern {
        filename: "cli.json",
        parent_dir: Some(".cursor"),
        config_type_prefix: "project_settings",
    },
    ConfigFilePattern {
        filename: "mcp.json",
        parent_dir: Some(".cursor"),
        config_type_prefix: "project_mcp",
    },
];

pub struct EnumerationData {
    pub config_items: Vec<ConfigItem>,
    pub sessions: Vec<SessionItem>,
    pub project_paths: Vec<String>,
}

//
// Cache for cursor-agent path lookup.
//

static CURSOR_AGENT_PATH: OnceLock<Option<String>> = OnceLock::new();

//
// Find cursor-agent executable path.
//

fn find_cursor_agent_path() -> Option<String> {
    CURSOR_AGENT_PATH
        .get_or_init(|| {
            let paths = crate::utils::find_all_executables_in_path("cursor-agent");
            if let Some(path) = paths.first() {
                return Some(path.clone());
            }

            let explicit_paths = vec![
                "/usr/bin/cursor-agent".to_string(),
                crate::agent_connectors::utils::expand_path("${HOME}/.local/bin/cursor-agent"),
            ];

            explicit_paths
                .into_iter()
                .find(|p| std::path::Path::new(p).exists())
        })
        .clone()
}

//
// Check if cursor-agent is available.
//

#[allow(dead_code)]
pub fn has_cursor_cli() -> bool {
    find_cursor_agent_path().is_some()
}

//
// Check if authentication environment variables are set.
//

pub fn has_auth_env_vars() -> bool {
    std::env::var("CURSOR_API_KEY").is_ok()
}

//
// Check if a user is logged in by running `cursor-agent status`.
// Returns true if the output contains "Logged in".
//

fn check_user_logged_in(cursor_agent_path: &str, working_dir: &Path) -> bool {
    let mut cmd = Command::new(cursor_agent_path);
    cmd.arg("status");
    cmd.current_dir(working_dir);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    //
    // Run as the user who owns the working directory.
    //

    crate::agent_connectors::utils::configure_command_for_directory(&mut cmd, working_dir);

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            common::log_debug!("Failed to run cursor-agent status: {}", e);
            return false;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    //
    // Check for "Logged in" in output (with or without checkmark).
    //

    let logged_in = stdout.contains("Logged in");
    common::log_debug!(
        "cursor-agent status for {}: logged_in={}",
        working_dir.display(),
        logged_in
    );

    logged_in
}

//
// Check if a path has valid Cursor access.
// Auth can come from:
// 1. CURSOR_API_KEY environment variable (global)
// 2. User logged in via cursor-agent (per-user)
//

pub fn path_has_valid_auth(path: &Path, user_homes: &[PathBuf]) -> bool {
    //
    // Global env vars take precedence.
    //

    if has_auth_env_vars() {
        return true;
    }

    //
    // Need cursor-agent to check login status.
    //

    let Some(cursor_agent_path) = find_cursor_agent_path() else {
        return false;
    };

    //
    // Find the user home that owns this path.
    //

    let path_str = path.to_string_lossy();
    for home in user_homes {
        let home_str = home.to_string_lossy();
        if path_str.starts_with(home_str.as_ref()) {
            return check_user_logged_in(&cursor_agent_path, home);
        }
    }

    //
    // If path is a user home itself, check directly.
    //

    if user_homes.iter().any(|h| h == path) {
        return check_user_logged_in(&cursor_agent_path, path);
    }

    //
    // Fallback: check with current directory.
    //

    check_user_logged_in(&cursor_agent_path, path)
}

pub fn enumerate() -> anyhow::Result<EnumerationData> {
    common::log_debug!("Enumerating Cursor configurations across all users");

    let mut config_items = Vec::new();
    let mut sessions = Vec::new();
    let mut project_paths_set = HashSet::new();

    let user_homes = enumerate_user_homes();
    let user_homes_set: HashSet<&Path> = user_homes.iter().map(|p| p.as_path()).collect();

    //
    // Collect global config files from all user homes.
    //

    config_items.extend(collect_global_config_files(&user_homes, GLOBAL_CONFIG_PATTERNS));

    //
    // Discover project paths from ~/.cursor/projects/ workspace-trusted files.
    //

    for home in &user_homes {
        discover_trusted_workspaces(home, &mut project_paths_set);
    }

    //
    // Discover sessions from ~/.config/cursor/chats/.
    //

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
        "Cursor enumeration complete: {} configs, {} sessions, {} projects",
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

//
// Discover trusted workspaces from ~/.cursor/projects/<hash>/.workspace-trusted files.
// These files contain the actual workspace path.
//

fn discover_trusted_workspaces(home: &Path, project_paths: &mut HashSet<String>) {
    let projects_dir = home.join(".cursor/projects");
    if !projects_dir.exists() {
        return;
    }

    let Ok(entries) = fs::read_dir(&projects_dir) else {
        return;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let project_hash_dir = entry.path();
        if !project_hash_dir.is_dir() {
            continue;
        }

        let trusted_file = project_hash_dir.join(".workspace-trusted");
        if !trusted_file.exists() {
            continue;
        }

        let Ok(contents) = fs::read_to_string(&trusted_file) else {
            continue;
        };

        let Ok(json) = serde_json::from_str::<Value>(&contents) else {
            continue;
        };

        //
        // Extract workspacePath from .workspace-trusted JSON.
        //

        if let Some(workspace_path) = json.get("workspacePath").and_then(|v| v.as_str()) {
            if Path::new(workspace_path).exists() {
                project_paths.insert(workspace_path.to_string());
            }
        }
    }
}

//
// Discover sessions from ~/.config/cursor/chats/ directories.
// Structure: ~/.config/cursor/chats/<project_hash>/<chat_id>/
//

fn discover_sessions(home: &Path, sessions: &mut Vec<SessionItem>) -> anyhow::Result<()> {
    let chats_dir = home.join(".config/cursor/chats");
    if !chats_dir.exists() {
        return Ok(());
    }

    let context_path = home.to_string_lossy().to_string();

    let Ok(project_entries) = fs::read_dir(&chats_dir) else {
        return Ok(());
    };

    for project_entry in project_entries.filter_map(|e| e.ok()) {
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }

        let project_hash = project_entry.file_name().to_string_lossy().to_string();

        let Ok(chat_entries) = fs::read_dir(&project_path) else {
            continue;
        };

        for chat_entry in chat_entries.filter_map(|e| e.ok()) {
            let chat_path = chat_entry.path();
            if !chat_path.is_dir() {
                continue;
            }

            let chat_id = chat_entry.file_name().to_string_lossy().to_string();

            if let Some(session) =
                parse_chat_session(&chat_path, &chat_id, &project_hash, &context_path)
            {
                sessions.push(session);
            }
        }
    }

    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    Ok(())
}

//
// Parse a chat session directory to extract session information.
//

fn parse_chat_session(
    chat_path: &Path,
    chat_id: &str,
    project_hash: &str,
    context_path: &str,
) -> Option<SessionItem> {
    //
    // Get directory modification time as last_modified.
    //

    let metadata = fs::metadata(chat_path).ok()?;
    let last_modified = metadata.modified().ok()?;
    let last_modified_dt: DateTime<Utc> = last_modified.into();

    //
    // Count files in the chat directory as a proxy for message count.
    //

    let message_count = fs::read_dir(chat_path)
        .ok()
        .map(|entries| entries.filter_map(|e| e.ok()).count())
        .unwrap_or(0);

    Some(SessionItem {
        session_id: chat_id.to_string(),
        context_path: format!("{}:{}", context_path, project_hash),
        session_file: chat_path.to_string_lossy().to_string(),
        last_modified: last_modified_dt.to_rfc3339(),
        message_count,
        content: None,
    })
}
