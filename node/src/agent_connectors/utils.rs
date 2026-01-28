use anyhow::{anyhow, Result};
use std::process::{Command, Output, Stdio};

//
// Prompt for discovering internal/built-in tools from an agent.
//

pub const INTERNAL_TOOLS_DISCOVERY_PROMPT: &str = "List all your internal/built-in tools with their descriptions. Do NOT include MCP tools - only internal tools that are part of your core functionality.";

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

    let mut cmd = Command::new(path);
    cmd.env("PATH", new_path);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[cfg(not(windows))]
pub fn build_command(path: &str) -> Command {
    Command::new(path)
}

//
// Execute a command and return the trimmed stdout output.
// Logs the command and output.
//

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

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "Command exited with status {}: {}",
            output.status,
            stderr
        ));
    }

    let response = String::from_utf8_lossy(&output.stdout).to_string();
    let trimmed = response.trim().to_string();
    common::log_info!("output: {}", trimmed);
    Ok(trimmed)
}

//
// Execute a command silently (no logging) and return the raw output.
// Useful for internal commands like --list-sessions.
//

pub fn run_command_silent(cmd: &mut Command) -> Result<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    cmd.output()
        .map_err(|e| anyhow!("Failed to execute command: {}", e))
}

//
// Discover internal tools semantically by creating a temporary session, sending
// a discovery prompt, and parsing the response through the semantic parser.
//
// Takes a closure that creates a session for the specific agent type.
// The agent_name parameter is used for logging.
//
pub async fn discover_internal_tools_semantically<F>(
    agent_name: &str,
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
    let prompt = INTERNAL_TOOLS_DISCOVERY_PROMPT;
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
    let discovery_prompt = crate::utils::semantic_parser::build_internal_tools_prompt(&response);
    match semantic_client
        .parse(
            discovery_prompt,
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
