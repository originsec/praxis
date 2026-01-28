use crate::agent_connectors::utils::{enumerate_user_homes, scan_directories_for_config_files};
use common::ConfigItem;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

const MAX_SCAN_DEPTH: usize = 7;    // Maximum directory depth to scan for config files

pub struct EnumerationData {
    pub config_items: Vec<ConfigItem>,
    pub project_paths: Vec<String>,
}

//
// Get the system defaults file path.
// Can be overridden with GEMINI_CLI_SYSTEM_DEFAULTS_PATH environment variable.
//

fn get_system_defaults_path() -> Option<PathBuf> {
    //
    // Check environment variable first.
    //

    if let Ok(env_path) = std::env::var("GEMINI_CLI_SYSTEM_DEFAULTS_PATH") {
        return Some(PathBuf::from(env_path));
    }

    //
    // Platform-specific defaults.
    //

    #[cfg(target_os = "linux")]
    {
        Some(PathBuf::from("/etc/gemini-cli/system-defaults.json"))
    }

    #[cfg(target_os = "windows")]
    {
        Some(PathBuf::from(
            "C:\\ProgramData\\gemini-cli\\system-defaults.json",
        ))
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

//
// Get the system settings file path (overrides).
// Can be overridden with GEMINI_CLI_SYSTEM_SETTINGS_PATH environment variable.
//

fn get_system_settings_path() -> Option<PathBuf> {
    //
    // Check environment variable first.
    //

    if let Ok(env_path) = std::env::var("GEMINI_CLI_SYSTEM_SETTINGS_PATH") {
        return Some(PathBuf::from(env_path));
    }

    //
    // Platform-specific defaults.
    //

    #[cfg(target_os = "linux")]
    {
        Some(PathBuf::from("/etc/gemini-cli/settings.json"))
    }

    #[cfg(target_os = "windows")]
    {
        Some(PathBuf::from("C:\\ProgramData\\gemini-cli\\settings.json"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

//
// Extract context file names from a settings JSON file.
// Looks for context.fileName which can be a string or array of strings.
//

fn extract_context_filenames(json_str: &str) -> Vec<String> {
    let mut filenames = Vec::new();

    if let Ok(json) = serde_json::from_str::<Value>(json_str) {
        if let Some(context) = json.get("context") {
            if let Some(file_name) = context.get("fileName") {
                match file_name {
                    //
                    // Single string.
                    //

                    Value::String(s) => {
                        filenames.push(s.clone());
                    }

                    //
                    // Array of strings.
                    //

                    Value::Array(arr) => {
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                filenames.push(s.to_string());
                            }
                        }
                    }

                    _ => {}
                }
            }
        }
    }

    filenames
}

//
// Collect Gemini-related environment variables.
// Returns a ConfigItem with all discovered environment variables.
//

fn collect_environment_variables() -> Option<ConfigItem> {
    //
    // List of Gemini-related environment variables to collect.
    //

    const ENV_VARS: &[&str] = &[
        "GEMINI_API_KEY",
        "GEMINI_MODEL",
        "GOOGLE_API_KEY",
        "GOOGLE_CLOUD_PROJECT",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "OTLP_GOOGLE_CLOUD_PROJECT",
        "GEMINI_TELEMETRY_ENABLED",
        "GEMINI_TELEMETRY_TARGET",
        "GEMINI_TELEMETRY_OTLP_ENDPOINT",
        "GEMINI_TELEMETRY_OTLP_PROTOCOL",
        "GEMINI_TELEMETRY_LOG_PROMPTS",
        "GEMINI_TELEMETRY_OUTFILE",
        "GEMINI_TELEMETRY_USE_COLLECTOR",
        "GOOGLE_CLOUD_LOCATION",
        "GEMINI_SANDBOX",
        "GEMINI_SYSTEM_MD",
        "GEMINI_WRITE_SYSTEM_MD",
        "DEBUG",
        "DEBUG_MODE",
        "NO_COLOR",
        "CLI_TITLE",
        "CODE_ASSIST_ENDPOINT",
    ];

    let mut env_lines = Vec::new();

    for var_name in ENV_VARS {
        if let Ok(value) = std::env::var(var_name) {
            env_lines.push(format!("{}={}", var_name, value));
        }
    }

    //
    // Only return a ConfigItem if we found at least one variable.
    //

    if env_lines.is_empty() {
        return None;
    }

    Some(ConfigItem {
        path: "environment:gemini".to_string(),
        contents: env_lines.join("\n"),
        config_type: "env_vars".to_string(),
    })
}

pub fn enumerate() -> anyhow::Result<EnumerationData> {
    common::log_info!("Enumerating Gemini configurations across all users");

    let mut config_items = Vec::new();
    let mut project_paths_set = HashSet::new();

    //
    // Get system defaults (lowest precedence).
    //

    if let Some(system_defaults_path) = get_system_defaults_path() {
        if let Ok(contents) = fs::read_to_string(&system_defaults_path) {
            config_items.push(ConfigItem {
                path: system_defaults_path.to_string_lossy().to_string(),
                contents,
                config_type: "system_defaults".to_string(),
            });
            common::log_debug!("Found system defaults: {}", system_defaults_path.display());
        }
    }

    //
    // Collect user settings from all user homes.
    //

    let user_homes = enumerate_user_homes();
    let user_homes_set: HashSet<&std::path::Path> = user_homes
        .iter()
        .map(|p| p.as_path())
        .collect();

    for home in &user_homes {
        let gemini_dir = home.join(".gemini");

        //
        // Collect settings.json.
        //

        let global_settings = gemini_dir.join("settings.json");
        if let Ok(contents) = fs::read_to_string(&global_settings) {
            config_items.push(ConfigItem {
                path: global_settings.to_string_lossy().to_string(),
                contents,
                config_type: "user_settings".to_string(),
            });
        }

        //
        // Collect google_accounts.json.
        //

        let google_accounts = gemini_dir.join("google_accounts.json");
        if let Ok(contents) = fs::read_to_string(&google_accounts) {
            config_items.push(ConfigItem {
                path: google_accounts.to_string_lossy().to_string(),
                contents,
                config_type: "user_google_accounts".to_string(),
            });
        }

        //
        // Collect oauth_creds.json.
        //

        let oauth_creds = gemini_dir.join("oauth_creds.json");
        if let Ok(contents) = fs::read_to_string(&oauth_creds) {
            config_items.push(ConfigItem {
                path: oauth_creds.to_string_lossy().to_string(),
                contents,
                config_type: "user_oauth_creds".to_string(),
            });
        }
    }

    //
    // Scan for project-level settings.json files in .gemini directories.
    //

    let settings_configs = scan_directories_for_config_files(
        &user_homes,
        "settings.json",
        |path| {
            //
            // Check if this settings.json is inside a .gemini directory.
            //

            if let Some(parent) = path.parent() {
                if parent.file_name().map_or(false, |n| n == ".gemini") {
                    if let Some(project_dir) = parent.parent() {
                        //
                        // Skip ~/.gemini directories.
                        //

                        if !user_homes_set.contains(project_dir) {
                            let project_path = project_dir.to_string_lossy().to_string();
                            project_paths_set.insert(project_path.clone());
                            return format!("project_settings:{}", project_path);
                        }
                    }
                }
            }
            String::new()
        },
        MAX_SCAN_DEPTH,
    );

    config_items.extend(settings_configs.into_iter().filter(|item| !item.config_type.is_empty()));

    //
    // Find system settings.
    //

    if let Some(system_settings_path) = get_system_settings_path() {
        if let Ok(contents) = fs::read_to_string(&system_settings_path) {
            config_items.push(ConfigItem {
                path: system_settings_path.to_string_lossy().to_string(),
                contents,
                config_type: "system_settings".to_string(),
            });
        }
    }

    //
    // Discover context file names from all settings files.
    //

    let mut context_filenames_set = HashSet::new();

    //
    // Always include GEMINI.md as default.
    //

    context_filenames_set.insert("GEMINI.md".to_string());

    //
    // Extract from system defaults.
    //

    if let Some(system_defaults_path) = get_system_defaults_path() {
        if let Ok(contents) = fs::read_to_string(&system_defaults_path) {
            for filename in extract_context_filenames(&contents) {
                context_filenames_set.insert(filename);
            }
        }
    }

    //
    // Extract from user settings.
    //

    for home in &user_homes {
        let global_settings = home.join(".gemini").join("settings.json");
        if let Ok(contents) = fs::read_to_string(&global_settings) {
            for filename in extract_context_filenames(&contents) {
                context_filenames_set.insert(filename);
            }
        }
    }

    //
    // Extract from project settings.
    //

    for item in &config_items {
        if item.config_type.starts_with("project_settings:") {
            for filename in extract_context_filenames(&item.contents) {
                context_filenames_set.insert(filename);
            }
        }
    }

    //
    // Extract from system settings.
    //

    if let Some(system_settings_path) = get_system_settings_path() {
        if let Ok(contents) = fs::read_to_string(&system_settings_path) {
            for filename in extract_context_filenames(&contents) {
                context_filenames_set.insert(filename);
            }
        }
    }

    common::log_info!("Scanning for {} context file names", context_filenames_set.len());

    //
    // Collect global context files from user homes.
    //

    for home in &user_homes {
        let gemini_dir = home.join(".gemini");
        for filename in &context_filenames_set {
            let context_path = gemini_dir.join(filename);
            if let Ok(contents) = fs::read_to_string(&context_path) {
                config_items.push(ConfigItem {
                    path: context_path.to_string_lossy().to_string(),
                    contents,
                    config_type: "user_context".to_string(),
                });
            }
        }
    }

    //
    // Scan for project-level context files.
    //

    for filename in context_filenames_set {
        let context_configs = scan_directories_for_config_files(
            &user_homes,
            &filename,
            |path| {
                if let Some(parent) = path.parent() {
                    //
                    // Skip home directories and .gemini directories.
                    //

                    if user_homes_set.contains(parent) {
                        return String::new();
                    }

                    if parent.file_name().map_or(false, |n| n == ".gemini") {
                        return String::new();
                    }

                    let project_path = parent.to_string_lossy().to_string();
                    project_paths_set.insert(project_path.clone());
                    return format!("project_context:{}", project_path);
                }
                String::new()
            },
            MAX_SCAN_DEPTH,
        );

        config_items.extend(context_configs.into_iter().filter(|item| !item.config_type.is_empty()));
    }

    //
    // Collect environment variables.
    //

    if let Some(env_config) = collect_environment_variables() {
        config_items.push(env_config);
    }

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

