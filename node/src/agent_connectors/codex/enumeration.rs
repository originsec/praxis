use crate::agent_connectors::utils::{
    collect_global_config_files, enumerate_user_homes, scan_directories_for_config_files_multi,
    ConfigFilePattern, GlobalConfigPattern,
};
use chrono::Utc;
use common::{ConfigItem, SessionItem};
use std::collections::HashSet;
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
    GlobalConfigPattern {
        path: ".codex/history.jsonl",
        config_type: "session_history",
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
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

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
    // Codex stores session history as per-session JSONL files in:
    // - ~/.codex/sessions/**.jsonl
    // - ~/.codex/archived_sessions/*.jsonl
    //

    let codex_dir = home.join(".codex");
    let sessions_dir = codex_dir.join("sessions");
    let archived_dir = codex_dir.join("archived_sessions");

    discover_sessions_in_dir(home, &sessions_dir, sessions)?;
    discover_sessions_in_dir(home, &archived_dir, sessions)?;

    Ok(())
}

fn discover_sessions_in_dir(
    home: &Path,
    dir: &Path,
    sessions: &mut Vec<SessionItem>,
) -> anyhow::Result<()> {
    use walkdir::WalkDir;

    if !dir.exists() {
        return Ok(());
    }

    let context_path = home.to_string_lossy().to_string();

    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        if let Some(session) = parse_session_file(&context_path, path) {
            sessions.push(session);
        }
    }

    Ok(())
}

fn parse_session_file(context_path: &str, path: &Path) -> Option<SessionItem> {
    let file = fs::File::open(path).ok()?;
    let session_file = path.to_string_lossy().to_string();

    let mut session_id: Option<String> = None;
    let mut message_count: usize = 0;
    let mut last_modified: Option<chrono::DateTime<Utc>> = None;

    for line in std::io::BufReader::new(file).lines().flatten() {
        if line.trim().is_empty() {
            continue;
        }

        let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };

        if let Some(ts) = json.get("timestamp").and_then(|v| v.as_str()) {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                let dt_utc = dt.with_timezone(&Utc);
                if last_modified.map_or(true, |cur| dt_utc > cur) {
                    last_modified = Some(dt_utc);
                }
            }
        }

        if session_id.is_none() {
            if json.get("type").and_then(|v| v.as_str()) == Some("session_meta") {
                if let Some(id) = json
                    .get("payload")
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                {
                    session_id = Some(id.to_string());
                }
            } else if let Some(id) = json.get("session_id").and_then(|v| v.as_str()) {
                session_id = Some(id.to_string());
            }
        }

        if json.get("type").and_then(|v| v.as_str()) == Some("response_item") {
            message_count += 1;
        }
    }

    let session_id = session_id.unwrap_or_else(|| "unknown".to_string());
    let last_modified = last_modified
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();

    Some(SessionItem {
        session_id,
        context_path: context_path.to_string(),
        session_file,
        last_modified,
        message_count,
        content: None,
    })
}

fn extract_project_paths_from_config(home: &Path, project_paths: &mut HashSet<String>) {
    //
    // Parse ~/.codex/config.toml and extract [projects."<path>"] sections.
    //

    let config_path = home.join(".codex").join("config.toml");
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
