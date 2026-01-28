use crate::agent_connectors::utils::SKIP_DIRS;
use common::ConfigItem;
use std::collections::HashSet;
use std::fs;
use walkdir::WalkDir;

/// Result of enumeration containing configs and project paths.
pub struct EnumerationData {
    pub config_items: Vec<ConfigItem>,
    pub project_paths: Vec<String>,
}

/// Enumerate Gemini configurations.
pub fn enumerate() -> anyhow::Result<EnumerationData> {
    common::log_info!("Enumerating Gemini configurations");

    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    let mut config_items = Vec::new();
    let mut project_paths_set = HashSet::new();

    //
    // Global configuration file (~/.gemini/settings.json).
    //

    let global_settings = home.join(".gemini").join("settings.json");
    if let Ok(contents) = fs::read_to_string(&global_settings) {
        config_items.push(ConfigItem {
            path: global_settings.to_string_lossy().to_string(),
            contents,
            config_type: "global_settings".to_string(),
        });
    }

    //
    // Use walkdir to scan from HOME for .gemini directories or gemini.json files.
    //

    common::log_info!("Scanning for Gemini project configs from home directory...");

    let walker = WalkDir::new(&home)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();

            //
            // Skip hidden directories (except .gemini which we're looking for)
            // and skip known non-project directories.
            //

            if name.starts_with('.') && name != ".gemini" {
                return false;
            }
            !SKIP_DIRS.contains(&name.as_ref())
        });

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();

        //
        // We're looking for .gemini directories.
        //

        if path.is_dir() && path.file_name().map_or(false, |n| n == ".gemini") {
            //
            // The project path is the parent of .gemini.
            //

            if let Some(project_dir) = path.parent() {
                let project_path = project_dir.to_string_lossy().to_string();

                //
                // Skip the global ~/.gemini directory.
                //

                if project_dir == home {
                    continue;
                }

                //
                // Collect project-level config.
                //

                let settings_file = path.join("settings.json");
                if let Ok(contents) = fs::read_to_string(&settings_file) {
                    config_items.push(ConfigItem {
                        path: settings_file.to_string_lossy().to_string(),
                        contents,
                        config_type: format!("project_settings:{}", project_path),
                    });
                    project_paths_set.insert(project_path);
                }
            }
        }

        //
        // Also check for standalone gemini.json without .gemini dir.
        //

        if path.is_file() {
            if let Some(file_name) = path.file_name() {
                if file_name == "gemini.json" {
                    if let Some(parent) = path.parent() {
                        //
                        // Skip if there's already a .gemini directory here.
                        //

                        if parent.join(".gemini").exists() {
                            continue;
                        }

                        let project_path = parent.to_string_lossy().to_string();

                        //
                        // Skip home directory.
                        //

                        if parent == home {
                            continue;
                        }

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
        "Gemini enumeration complete: {} configs, {} projects",
        config_items.len(),
        project_paths.len()
    );

    Ok(EnumerationData {
        config_items,
        project_paths,
    })
}
