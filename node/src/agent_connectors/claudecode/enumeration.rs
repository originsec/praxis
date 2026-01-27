use common::{AgentSessionInfo, ConfigItem};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// Directories to skip during recursive scanning
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "__pycache__",
    "target",
    "build",
    "dist",
    ".venv",
    "venv",
    ".cache",
    ".local",
    ".config",
    ".npm",
    ".cargo",
    ".rustup",
    "Library",
    "Applications",
    ".Trash",
    "Pictures",
    "Music",
    "Movies",
    "Downloads",
];

/// Result of enumeration containing configs, sessions, and project paths.
pub struct EnumerationData {
    pub config_items: Vec<ConfigItem>,
    pub sessions: Vec<AgentSessionInfo>,
    pub project_paths: Vec<String>,
}

/// Enumerate Claude Code configurations and sessions.
pub fn enumerate() -> anyhow::Result<EnumerationData> {
    common::log_info!("Enumerating Claude Code configurations and sessions");

    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    let mut config_items = Vec::new();
    let mut sessions = Vec::new();
    let mut project_paths_set = HashSet::new();

    //
    // Global configuration files.
    //
    collect_config_file(&home.join(".claude/settings.json"), "global_settings", &mut config_items);
    collect_config_file(&home.join(".claude.json"), "preferences", &mut config_items);
    collect_config_file(&home.join(".claude/CLAUDE.md"), "global_instructions", &mut config_items);

    //
    // Discover sessions.
    //
    discover_sessions(&home, &mut sessions)?;

    //
    // Use walkdir to scan from HOME for .claude directories.
    //
    common::log_info!("Scanning for project configs from home directory...");

    let walker = WalkDir::new(&home)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            //
            // Skip hidden directories (except .claude which we're looking for)
            // and skip known non-project directories.
            //
            if name.starts_with('.') && name != ".claude" {
                return false;
            }
            !SKIP_DIRS.contains(&name.as_ref())
        });

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();

        //
        // We're looking for .claude directories.
        //
        if path.is_dir() && path.file_name().map_or(false, |n| n == ".claude") {
            //
            // The project path is the parent of .claude.
            //
            if let Some(project_dir) = path.parent() {
                let project_path = project_dir.to_string_lossy().to_string();

                //
                // Skip the global ~/.claude directory.
                //
                if project_dir == home {
                    continue;
                }

                //
                // Collect project-level configs.
                //
                collect_project_configs(project_dir, path, &project_path, &mut config_items);
                project_paths_set.insert(project_path);
            }
        }

        //
        // Also check for standalone CLAUDE.md or .mcp.json without .claude dir.
        //
        if path.is_file() {
            let file_name = path.file_name().map(|n| n.to_string_lossy().to_string());
            if let (Some(name), Some(parent)) = (file_name, path.parent()) {
                if (name == "CLAUDE.md" || name == ".mcp.json") && !parent.join(".claude").exists() {
                    let project_path = parent.to_string_lossy().to_string();

                    //
                    // Skip home directory.
                    //
                    if parent == home {
                        continue;
                    }

                    if name == "CLAUDE.md" {
                        if let Ok(contents) = fs::read_to_string(path) {
                            config_items.push(ConfigItem {
                                path: path.to_string_lossy().to_string(),
                                contents,
                                config_type: format!("project_instructions:{}", project_path),
                            });
                            project_paths_set.insert(project_path);
                        }
                    } else if name == ".mcp.json" {
                        if let Ok(contents) = fs::read_to_string(path) {
                            config_items.push(ConfigItem {
                                path: path.to_string_lossy().to_string(),
                                contents,
                                config_type: format!("project_mcp:{}", project_path),
                            });
                            project_paths_set.insert(project_path);
                        }
                    }
                }
            }
        }
    }

    //
    // Convert project_paths to sorted vec.
    //
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

fn collect_project_configs(
    project_dir: &Path,
    claude_dir: &Path,
    project_path: &str,
    config_items: &mut Vec<ConfigItem>,
) {
    //
    // .claude/settings.json.
    //
    if let Ok(contents) = fs::read_to_string(claude_dir.join("settings.json")) {
        config_items.push(ConfigItem {
            path: claude_dir.join("settings.json").to_string_lossy().to_string(),
            contents,
            config_type: format!("project_settings:{}", project_path),
        });
    }

    //
    // .claude/settings.local.json.
    //
    if let Ok(contents) = fs::read_to_string(claude_dir.join("settings.local.json")) {
        config_items.push(ConfigItem {
            path: claude_dir.join("settings.local.json").to_string_lossy().to_string(),
            contents,
            config_type: format!("project_settings_local:{}", project_path),
        });
    }

    //
    // CLAUDE.md (in project root).
    //
    if let Ok(contents) = fs::read_to_string(project_dir.join("CLAUDE.md")) {
        config_items.push(ConfigItem {
            path: project_dir.join("CLAUDE.md").to_string_lossy().to_string(),
            contents,
            config_type: format!("project_instructions:{}", project_path),
        });
    }

    //
    // .claude/CLAUDE.md.
    //
    if let Ok(contents) = fs::read_to_string(claude_dir.join("CLAUDE.md")) {
        config_items.push(ConfigItem {
            path: claude_dir.join("CLAUDE.md").to_string_lossy().to_string(),
            contents,
            config_type: format!("project_instructions:{}", project_path),
        });
    }

    //
    // .mcp.json (in project root).
    //
    if let Ok(contents) = fs::read_to_string(project_dir.join(".mcp.json")) {
        config_items.push(ConfigItem {
            path: project_dir.join(".mcp.json").to_string_lossy().to_string(),
            contents,
            config_type: format!("project_mcp:{}", project_path),
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
