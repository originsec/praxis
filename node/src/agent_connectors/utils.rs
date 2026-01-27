use anyhow::{anyhow, Result};
use std::process::{Command, Output, Stdio};

/// Build a command for the given executable path.
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

/// Execute a command and return the trimmed stdout output.
/// Logs the command and output.
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

/// Execute a command silently (no logging) and return the raw output.
/// Useful for internal commands like --list-sessions.
pub fn run_command_silent(cmd: &mut Command) -> Result<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    cmd.output()
        .map_err(|e| anyhow!("Failed to execute command: {}", e))
}
