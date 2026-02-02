use crate::agent_connectors::utils::{
    collect_global_config_files, enumerate_user_homes, scan_directories_for_config_files_multi,
    ConfigFilePattern, GlobalConfigPattern,
};
use chrono::{TimeZone, Utc};
use common::{ConfigItem, SessionItem};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::BufRead;
use std::path::Path;

const MAX_SCAN_DEPTH: usize = 7;

//
// Global config patterns - files in user home directories.
//

const GLOBAL_CONFIG_PATTERNS: &[GlobalConfigPattern] = &[
    GlobalConfigPattern {
        path: ".codex/config.toml",
        config_type: "global_settings",
    },
    GlobalConfigPattern {
        path: ".codex/auth.json",
        config_type: "credentials",
    },
];

//
// Project-level config patterns - files in project directories.
//

const PROJECT_CONFIG_PATTERNS: &[ConfigFilePattern] = &[
    ConfigFilePattern {
        filename: "config.toml",
        parent_dir: Some(".codex"),
        config_type_prefix: "project_settings",
    },
];

pub struct EnumerationData {
    pub config_items: Vec<ConfigItem>,
    pub sessions: Vec<SessionItem>,
    pub project_paths: Vec<String>,
}

pub fn enumerate() -> anyhow::Result<EnumerationData> {
    common::log_debug!("Enumerating Codex configurations across all users");

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
    // Extract project paths from global config [projects."<path>"] sections.
    //

    for home in &user_homes {
        extract_project_paths_from_config(home, &mut project_paths_set);
    }

    //
    // Discover sessions from history.jsonl files.
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
        "Codex enumeration complete: {} configs, {} sessions, {} projects",
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
    //
    // Codex stores session history in ~/.codex/history.jsonl.
    // Parse and group entries by session_id.
    //

    let history_file = home.join(".codex/history.jsonl");
    if !history_file.exists() {
        return Ok(());
    }

    let parsed_sessions = parse_history_file(&history_file);
    sessions.extend(parsed_sessions);

    Ok(())
}

fn parse_history_file(path: &Path) -> Vec<SessionItem> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let context_path = path
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let session_file = path.to_string_lossy().to_string();

    //
    // Group entries by session_id, tracking count and max timestamp.
    //

    let mut session_data: HashMap<String, (usize, i64)> = HashMap::new();

    for line in std::io::BufReader::new(file).lines().flatten() {
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            let session_id = json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let ts = json.get("ts").and_then(|v| v.as_i64()).unwrap_or(0);

            let entry = session_data.entry(session_id).or_insert((0, 0));
            entry.0 += 1;  // message count
            if ts > entry.1 {
                entry.1 = ts;  // max timestamp
            }
        }
    }

    //
    // Convert to SessionItem list, sorted by timestamp descending.
    //

    let mut sessions: Vec<SessionItem> = session_data
        .into_iter()
        .map(|(session_id, (message_count, max_ts))| {
            let last_modified = if max_ts > 0 {
                Utc.timestamp_opt(max_ts, 0)
                    .single()
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default()
            } else {
                String::new()
            };

            SessionItem {
                session_id,
                context_path: context_path.clone(),
                session_file: session_file.clone(),
                last_modified,
                message_count,
                content: None,
            }
        })
        .collect();

    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    sessions
}

fn extract_project_paths_from_config(home: &Path, project_paths: &mut HashSet<String>) {
    //
    // Parse ~/.codex/config.toml and extract [projects."<path>"] sections.
    //

    let config_path = home.join(".codex/config.toml");
    if !config_path.exists() {
        return;
    }

    let contents = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let toml_value: toml::Value = match toml::from_str(&contents) {
        Ok(v) => v,
        Err(_) => return,
    };

    //
    // Look for [projects] table with path keys like [projects."/home/user/code"].
    //

    if let Some(projects) = toml_value.get("projects").and_then(|v| v.as_table()) {
        for path in projects.keys() {
            if std::path::Path::new(path).exists() {
                project_paths.insert(path.clone());
            }
        }
    }
}
