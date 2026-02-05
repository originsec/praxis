use anyhow::{anyhow, Result};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

//
// Enum for selecting which prompt to use for internal tools discovery.
//

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum ToolDiscoveryPrompt {
    //
    // Standard prompt asking for a list of internal tools with descriptions.
    //
    ListInternalTools,

    //
    // JSON format prompt for tools discovery.
    //
    JsonFormat,

    //
    // High level overview prompt for tools discovery.
    //
    HighLevel,
}

impl ToolDiscoveryPrompt {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolDiscoveryPrompt::ListInternalTools => {
                "List all your internal/built-in tools with their descriptions. Do NOT include MCP tools - only internal tools that are part of your core functionality."
            }
            ToolDiscoveryPrompt::JsonFormat => {
                "What tools do you have that you can use to help me? High level overview. Respond as json in format - complete this json: { tools: [{'toolName': toolname, 'toolDescription:' ..."
            }
            ToolDiscoveryPrompt::HighLevel => {
                "What tools do you have that you can use to help me? High level overview of each tool with a name an description. Don't leave any out..."
            }
        }
    }
}

//
// Maximum characters per batch when extracting metadata from config files.
// Config files are batched together until this threshold would be exceeded.
//

const METADATA_BATCH_CHAR_THRESHOLD: usize = 15000;

//
// Directories to skip during recursive scanning.
// Includes common build artifacts, version control, caches, and OS-specific
// directories for Windows, Linux, and macOS.
//

pub const SKIP_DIRS: &[&str] = &[
    // Build artifacts and dependencies
    "node_modules",
    "target",
    "build",
    "dist",
    "out",
    ".next",
    ".nuxt",
    "bower_components",
    // Version control
    ".git",
    ".svn",
    ".hg",
    // Python
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".pytest_cache",
    ".mypy_cache",
    // Caches and package managers
    ".cache",
    ".npm",
    ".yarn",
    ".pnpm",
    ".cargo",
    ".rustup",
    ".m2",
    ".gradle",
    // IDE and editors
    ".idea",
    ".vscode",
    ".vs",
    // macOS specific
    "Library",
    "Applications",
    ".Trash",
    "Pictures",
    "Music",
    "Movies",
    "Downloads",
    // Windows specific
    "AppData",
    "$Recycle.Bin",
    "System Volume Information",
    // Linux/Unix
    ".local",
    ".config",
    // Temporary
    "tmp",
    "temp",
    ".tmp",
];

//
// Extract the user home directory from a path.
//
// Given a path like /home/depmod/code/project, returns /home/depmod.
// This is useful when running as root but needing to access config files
// in the original user's home directory.
//
// - Linux/Unix: Extracts /home/<user> or /root from path
// - Windows: Extracts C:\Users\<user> from path
// - Falls back to dirs::home_dir() if pattern doesn't match
//
pub fn extract_user_home_from_path(path: &str) -> Option<PathBuf> {
    let path = std::path::Path::new(path);

    #[cfg(not(target_os = "windows"))]
    {
        //
        // Check for /home/<user>/... pattern.
        //
        let mut components = path.components();
        if let (Some(std::path::Component::RootDir), Some(std::path::Component::Normal(first))) =
            (components.next(), components.next())
        {
            if first == "home" {
                if let Some(std::path::Component::Normal(user)) = components.next() {
                    return Some(PathBuf::from("/home").join(user));
                }
            } else if first == "root" {
                return Some(PathBuf::from("/root"));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        //
        // Check for C:\Users\<user>\... pattern.
        //
        let path_str = path.to_string_lossy().to_lowercase();
        if path_str.contains("\\users\\") || path_str.contains("/users/") {
            let mut components = path.components();
            //
            // Skip prefix (C:) and root (\).
            //
            let _ = components.next();
            let _ = components.next();

            if let Some(std::path::Component::Normal(first)) = components.next() {
                if first.to_string_lossy().to_lowercase() == "users" {
                    if let Some(std::path::Component::Normal(user)) = components.next() {
                        return Some(PathBuf::from("C:\\Users").join(user));
                    }
                }
            }
        }
    }

    //
    // Fallback to current user's home.
    //
    dirs::home_dir()
}

//
// Enumerate all user home directories on the system.
// Returns a list of home directories that can be accessed.
//
// - Windows: Enumerates C:\Users\*
// - Linux/Unix: Enumerates /home/* and /root
// - Always includes current user's home as fallback
//
pub fn enumerate_user_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();

    #[cfg(target_os = "windows")]
    {
        //
        // On Windows, enumerate C:\Users\*
        //
        if let Ok(entries) = fs::read_dir("C:\\Users") {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() {
                    homes.push(path);
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        //
        // On Linux/Unix, enumerate /home/* and /root.
        //
        if let Ok(entries) = fs::read_dir("/home") {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if !name.starts_with('.') {
                        homes.push(path);
                    }
                }
            }
        }

        //
        // Add /root if it exists.
        //
        let root_path = PathBuf::from("/root");
        if root_path.is_dir() {
            homes.push(root_path);
        }
    }

    //
    // Always include current user's home directory as fallback.
    //
    if let Some(current_home) = dirs::home_dir() {
        if !homes.contains(&current_home) {
            homes.push(current_home);
        }
    }

    common::log_info!("Found {} user home directories to scan", homes.len());
    homes
}



//
// Expand environment variables in a path template.
//

pub fn expand_path(template: &str) -> String {
    let mut result = template.to_string();
    if let Ok(home) = std::env::var("HOME") {
        result = result.replace("${HOME}", &home);
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        result = result.replace("${USERPROFILE}", &userprofile);
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        result = result.replace("${APPDATA}", &appdata);
    }
    result
}

//
// Build a command for the given executable path.
//
/// On Windows, we need to ensure the real node.exe is found first in PATH,
/// otherwise npm batch scripts may accidentally run praxis_node.exe instead
/// (because Windows matches "node" to executables containing "node" in the name).
/// Also, .cmd files need to be run through cmd.exe.
#[cfg(windows)]
pub fn build_command(path: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    //
    // Get the directory containing the script - this is where node.exe should
    // be.
    //
    let script_dir = std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    //
    // Also check common Node.js installation paths.
    //
    let program_files = std::env::var("ProgramFiles").unwrap_or_default();
    let nodejs_path = format!("{}\\nodejs", program_files);

    //
    // Get current PATH and prepend Node.js directories.
    //
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{};{};{}", nodejs_path, script_dir, current_path);

    //
    // .cmd files need to be run through cmd.exe /c.
    //

    let mut cmd = if path.to_lowercase().ends_with(".cmd") {
        let mut c = Command::new("cmd.exe");
        c.arg("/c").arg(path);
        c
    } else {
        Command::new(path)
    };

    cmd.env("PATH", new_path);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[cfg(not(windows))]
pub fn build_command(path: &str) -> Command {
    Command::new(path)
}

//
// Get the owner uid/gid of a path. Returns None if the path doesn't exist or
// metadata can't be read.
//
#[cfg(unix)]
pub fn get_path_owner(path: &std::path::Path) -> Option<(u32, u32)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .ok()
        .map(|m| (m.uid(), m.gid()))
}

#[cfg(not(unix))]
#[allow(dead_code)]
pub fn get_path_owner(_path: &std::path::Path) -> Option<(u32, u32)> {
    None
}

//
// Configure a command to run as the owner of the specified working directory.
// Only takes effect when running as root on Unix systems. On non-Unix systems
// or when not running as root, this is a no-op.
//
#[cfg(unix)]
pub fn configure_command_for_directory(cmd: &mut Command, working_dir: &std::path::Path) {
    use std::os::unix::process::CommandExt;

    //
    // Only switch user if we're running as root.
    //
    if !nix::unistd::Uid::effective().is_root() {
        return;
    }

    if let Some((uid, gid)) = get_path_owner(working_dir) {
        //
        // Don't switch if the directory is owned by root.
        //
        if uid == 0 {
            return;
        }

        //
        // Look up the user's home directory from passwd.
        //
        let home_dir = nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|u| u.dir);

        if let Some(ref home) = home_dir {
            common::log_info!(
                "Running command as user {} (gid {}) with HOME={} for directory: {}",
                uid,
                gid,
                home.display(),
                working_dir.display()
            );
            cmd.env("HOME", home);
        } else {
            common::log_info!(
                "Running command as user {} (gid {}) for directory: {}",
                uid,
                gid,
                working_dir.display()
            );
        }

        cmd.uid(uid);
        cmd.gid(gid);
    }
}

#[cfg(not(unix))]
pub fn configure_command_for_directory(_cmd: &mut Command, _working_dir: &std::path::Path) {
    // No-op on non-Unix systems
}

//
// Execute a command and return the trimmed stdout output.
// Logs the command and output.
//

#[allow(dead_code)]
pub fn run_command(cmd: &mut Command) -> Result<String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    //
    // Log the full command line.
    //
    let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
    common::log_info!(
        "command: {} {}",
        cmd.get_program().to_string_lossy(),
        args.join(" ")
    );

    let output = cmd
        .output()
        .map_err(|e| anyhow!("Failed to execute command: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    //
    // Log stderr if present (even on success, for debugging).
    //
    if !stderr.trim().is_empty() {
        common::log_warn!("stderr: {}", stderr.trim());
    }

    if !output.status.success() {
        common::log_error!(
            "Command failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return Err(anyhow!(
            "Command exited with status {}: {}",
            output.status,
            stderr
        ));
    }

    let trimmed = stdout.trim().to_string();
    common::log_info!("output: {}", trimmed);
    Ok(trimmed)
}

//
// Execute a command silently (no logging) and return the raw output.
// Useful for internal commands like --list-sessions.
//

#[allow(dead_code)]
pub fn run_command_silent(cmd: &mut Command) -> Result<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    cmd.output()
        .map_err(|e| anyhow!("Failed to execute command: {}", e))
}

//
// Execute a command with input piped to stdin and return the trimmed stdout.
// Used for CLIs that require input via stdin (e.g., Gemini CLI).
//

#[allow(dead_code)]
pub fn run_command_with_stdin(cmd: &mut Command, input: &str) -> Result<String> {
    use std::io::Write;

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    //
    // Log the full command line and prompt.
    //
    let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
    common::log_info!(
        "command: {} {} (with stdin: {})",
        cmd.get_program().to_string_lossy(),
        args.join(" "),
        input.replace('\n', " | ")
    );

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn command: {}", e))?;

    //
    // Write input to stdin.
    //
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| anyhow!("Failed to write to stdin: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| anyhow!("Failed to wait for command: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    //
    // Log stderr if present (even on success, for debugging).
    //
    if !stderr.trim().is_empty() {
        common::log_warn!("stderr: {}", stderr.trim());
    }

    if !output.status.success() {
        common::log_error!(
            "Command failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return Err(anyhow!(
            "Command exited with status {}: {}",
            output.status,
            stderr
        ));
    }

    let trimmed = stdout.trim().to_string();
    common::log_info!("output: {}", trimmed);
    Ok(trimmed)
}

//
// Execute a command with stdin input and cancellation support via PID tracking.
//

pub fn run_command_with_stdin_cancellable(
    cmd: &mut Command,
    input: &str,
    active_pid: &AtomicU32,
) -> Result<String> {
    use std::io::Write;

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
    common::log_info!(
        "command: {} {} (with stdin: {})",
        cmd.get_program().to_string_lossy(),
        args.join(" "),
        input.replace('\n', " | ")
    );

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn command: {}", e))?;

    //
    // Store PID for potential cancellation.
    //

    let pid = child.id();
    active_pid.store(pid, Ordering::SeqCst);

    //
    // Write input to stdin.
    //

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| anyhow!("Failed to write to stdin: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| anyhow!("Failed to wait for command: {}", e))?;

    //
    // Clear PID after completion.
    //

    active_pid.store(0, Ordering::SeqCst);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !stderr.trim().is_empty() {
        common::log_warn!("stderr: {}", stderr.trim());
    }

    if !output.status.success() {
        //
        // Check if the process was killed by a signal (e.g., SIGKILL from abort).
        // This is expected behavior when we force-cancel a transaction.
        //

        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if output.status.signal().is_some() {
                common::log_warn!(
                    "Command terminated by signal {}: {}",
                    output.status,
                    stderr.trim()
                );
                return Err(anyhow!(
                    "Command terminated by signal {}: {}",
                    output.status,
                    stderr
                ));
            }
        }

        common::log_error!(
            "Command failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return Err(anyhow!(
            "Command exited with status {}: {}",
            output.status,
            stderr
        ));
    }

    let trimmed = stdout.trim().to_string();
    common::log_info!("output: {}", trimmed);
    Ok(trimmed)
}

//
// Discover internal tools semantically by creating a temporary session, sending
// a discovery prompt, and parsing the response through the semantic parser.
//
// Takes a closure that creates a session for the specific agent type.
// The agent_name parameter is used for logging.
// The prompt_type parameter selects which discovery prompt to use.
//
pub async fn discover_internal_tools_semantically<F>(
    agent_name: &str,
    prompt_type: ToolDiscoveryPrompt,
    create_session: F,
) -> Vec<common::AgentTool>
where
    F: FnOnce() -> Result<std::sync::Arc<dyn crate::agent_connectors::traits::AgentSession>>,
{
    common::log_info!("{}: Starting internal tools discovery", agent_name);

    //
    // Create a temporary session using the provided closure.
    //
    let temp_session = match create_session() {
        Ok(session) => session,
        Err(e) => {
            common::log_warn!(
                "{}: Failed to create temporary session: {}",
                agent_name,
                e
            );
            return Vec::new();
        }
    };

    //
    // Send the prompt to list internal tools.
    //
    let prompt = prompt_type.as_str();
    common::log_info!("{}: Sending internal tools discovery prompt", agent_name);
    let response = match temp_session.transact(prompt) {
        Ok(response) => response,
        Err(e) => {
            common::log_warn!(
                "{}: Failed to get internal tools list from agent: {}",
                agent_name,
                e
            );
            temp_session.close();
            return Vec::new();
        }
    };

    temp_session.close();

    //
    // Strip "Generating response" text that some agents prepend.
    //

    let response = response.replace("Generating response", "");

    //
    // Parse the response through the semantic parser.
    //
    common::log_info!(
        "{}: Parsing internal tools response through semantic parser",
        agent_name
    );
    let semantic_client = match crate::utils::semantic_parser::get_client() {
        Some(client) => client,
        None => {
            common::log_warn!("{}: Semantic parser client not available", agent_name);
            return Vec::new();
        }
    };

    //
    // Use the internal tools schema to parse the response.
    //

    match semantic_client
        .parse(
            crate::utils::semantic_parser::INTERNAL_TOOLS_PROMPT.to_string(),
            response,
            crate::utils::semantic_parser::INTERNAL_TOOLS_SCHEMA.to_string(),
        )
        .await
    {
        Ok(parser_response) => {
            if parser_response.success {
                if let Some(json) = parser_response.json {
                    if let Some(internal_tools) =
                        crate::utils::semantic_parser::parse_internal_tools_from_json(&json)
                    {
                        common::log_info!(
                            "{}: Discovered {} internal tools",
                            agent_name,
                            internal_tools.len()
                        );
                        return internal_tools;
                    }
                }
            }
            common::log_warn!(
                "{}: Semantic parser failed for internal tools: {:?}",
                agent_name,
                parser_response.error
            );
        }
        Err(e) => {
            common::log_warn!(
                "{}: Semantic parser request failed for internal tools: {}",
                agent_name,
                e
            );
        }
    }

    Vec::new()
}

//
// Extract metadata (user identities, API keys) from config files using the
// semantic parser. Batches config files together up to a character threshold
// before sending to semantic parser for efficiency.
//

pub async fn extract_metadata_from_configs(
    agent_name: &str,
    config_items: &[common::ConfigItem],
) -> Option<common::ReconMetadata> {
    if config_items.is_empty() {
        return None;
    }

    common::log_info!(
        "{}: Extracting metadata from {} config files",
        agent_name,
        config_items.len()
    );

    //
    // Get the semantic parser client.
    //

    let semantic_client = match crate::utils::semantic_parser::get_client() {
        Some(client) => client,
        None => {
            common::log_warn!(
                "{}: Semantic parser client not available for metadata extraction",
                agent_name
            );
            return None;
        }
    };

    //
    // Batch config files together until threshold is reached, then send to
    // semantic parser.
    //

    let mut all_user_identities = Vec::new();
    let mut all_api_keys = Vec::new();

    let mut current_batch = String::new();
    let mut batch_count = 0;

    for item in config_items {
        //
        // Skip items without contents (lazy-loaded).
        //
        let contents = match &item.contents {
            Some(c) => c,
            None => continue,
        };

        //
        // Format this config file.
        //

        let config_content = format!("=== {} ({}) ===\n{}\n\n", item.path, item.config_type, contents);
        let content_len = config_content.len();

        //
        // If this single file exceeds threshold, send it alone.
        //

        if content_len > METADATA_BATCH_CHAR_THRESHOLD {
            //
            // First, send any pending batch.
            //

            if !current_batch.is_empty() {
                process_metadata_batch(
                    &semantic_client,
                    agent_name,
                    &current_batch,
                    batch_count,
                    &mut all_user_identities,
                    &mut all_api_keys,
                )
                .await;
                current_batch.clear();
                batch_count = 0;
            }

            //
            // Send this large file alone.
            //

            process_metadata_batch(
                &semantic_client,
                agent_name,
                &config_content,
                1,
                &mut all_user_identities,
                &mut all_api_keys,
            )
            .await;
            continue;
        }

        //
        // If adding this file would exceed threshold, send current batch first.
        //

        if !current_batch.is_empty() && current_batch.len() + content_len > METADATA_BATCH_CHAR_THRESHOLD {
            process_metadata_batch(
                &semantic_client,
                agent_name,
                &current_batch,
                batch_count,
                &mut all_user_identities,
                &mut all_api_keys,
            )
            .await;
            current_batch.clear();
            batch_count = 0;
        }

        //
        // Add to current batch.
        //

        current_batch.push_str(&config_content);
        batch_count += 1;
    }

    //
    // Send any remaining batch.
    //

    if !current_batch.is_empty() {
        process_metadata_batch(
            &semantic_client,
            agent_name,
            &current_batch,
            batch_count,
            &mut all_user_identities,
            &mut all_api_keys,
        )
        .await;
    }

    //
    // Deduplicate results.
    //

    all_user_identities.sort();
    all_user_identities.dedup();

    all_api_keys.sort();
    all_api_keys.dedup();

    let has_identities = !all_user_identities.is_empty();
    let has_keys = !all_api_keys.is_empty();

    if has_identities || has_keys {
        common::log_info!(
            "{}: Extracted {} user identities, {} API keys",
            agent_name,
            all_user_identities.len(),
            all_api_keys.len()
        );

        return Some(common::ReconMetadata {
            user_identities: if has_identities {
                Some(all_user_identities)
            } else {
                None
            },
            api_keys: if has_keys { Some(all_api_keys) } else { None },
        });
    }

    None
}

//
// Process a batch of config files through the semantic parser.
//

async fn process_metadata_batch(
    semantic_client: &crate::utils::semantic_parser::SemanticParserClient,
    agent_name: &str,
    batch_content: &str,
    file_count: usize,
    all_user_identities: &mut Vec<String>,
    all_api_keys: &mut Vec<String>,
) {
    common::log_debug!(
        "{}: Processing metadata batch with {} files ({} chars)",
        agent_name,
        file_count,
        batch_content.len()
    );

    match semantic_client
        .parse(
            crate::utils::semantic_parser::METADATA_EXTRACTION_PROMPT.to_string(),
            batch_content.to_string(),
            crate::utils::semantic_parser::METADATA_EXTRACTION_SCHEMA.to_string(),
        )
        .await
    {
        Ok(parser_response) => {
            if parser_response.success {
                if let Some(json) = parser_response.json {
                    if let Some(extracted) = crate::utils::semantic_parser::parse_metadata_from_json(&json) {
                        all_user_identities.extend(extracted.user_identities);
                        all_api_keys.extend(extracted.api_keys);
                    }
                }
            }
        }
        Err(e) => {
            common::log_debug!(
                "{}: Semantic parser request failed for batch: {}",
                agent_name,
                e
            );
        }
    }
}
