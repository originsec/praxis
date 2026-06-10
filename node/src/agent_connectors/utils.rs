use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        result = result.replace("${LOCALAPPDATA}", &localappdata);
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
    std::fs::metadata(path).ok().map(|m| (m.uid(), m.gid()))
}

#[allow(dead_code)]
#[cfg(not(unix))]
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
