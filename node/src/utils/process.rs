#![allow(dead_code)]
use glob::glob;
use std::process::Command;
use sysinfo::{Pid, ProcessesToUpdate, System};

//
// Create a Command that won't show a console window on Windows.
//

#[cfg(windows)]
pub fn silent_command(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[cfg(not(windows))]
pub fn silent_command(program: &str) -> Command {
    Command::new(program)
}

//
// Find an executable in PATH using which (Unix) or where (Windows).
// Returns the path to the executable if found, None otherwise.
//
pub fn find_executable_in_path(executable_name: &str) -> Option<String> {
    #[cfg(windows)]
    let which_result = silent_command("where").arg(executable_name).output();

    #[cfg(not(windows))]
    let which_result = silent_command("which").arg(executable_name).output();

    if let Ok(output) = which_result {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(path) = stdout.lines().next() {
                let path = path.trim().to_string();
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }
    }

    None
}

//
// Find all instances of an executable in PATH using which (Unix) or where
// (Windows). Returns a Vec of all paths found.
// Useful when 'where' on Windows returns multiple results.
//
pub fn find_all_executables_in_path(executable_name: &str) -> Vec<String> {
    #[cfg(windows)]
    let which_result = silent_command("where").arg(executable_name).output();

    #[cfg(not(windows))]
    let which_result = silent_command("which").arg(executable_name).output();

    if let Ok(output) = which_result {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return stdout
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();
        }
    }

    Vec::new()
}

//
// Add Windows Firewall rule for the current executable to allow inbound
// connections without prompting the user. Returns true if rule was added
// or already exists.
//

#[cfg(windows)]
pub fn ensure_firewall_rule() -> bool {
    let exe_path = match std::env::current_exe() {
        Ok(path) => path.to_string_lossy().to_string(),
        Err(_) => return false,
    };

    let rule_name = "Praxis Node";

    //
    // Check if rule already exists.
    //

    let check = silent_command("netsh")
        .args([
            "advfirewall", "firewall", "show", "rule",
            &format!("name={}", rule_name),
        ])
        .output();

    if let Ok(output) = check {
        if output.status.success() {
            return true;
        }
    }

    //
    // Add inbound rule for TCP.
    //

    let result = silent_command("netsh")
        .args([
            "advfirewall", "firewall", "add", "rule",
            &format!("name={}", rule_name),
            "dir=in",
            "action=allow",
            &format!("program={}", exe_path),
            "protocol=tcp",
            "enable=yes",
        ])
        .output();

    matches!(result, Ok(output) if output.status.success())
}

#[cfg(not(windows))]
pub fn ensure_firewall_rule() -> bool {
    true
}

//
// Remove Windows Firewall rule for the current executable.
//

#[cfg(windows)]
pub fn remove_firewall_rule() -> bool {
    let rule_name = "Praxis Node";

    let result = silent_command("netsh")
        .args([
            "advfirewall", "firewall", "delete", "rule",
            &format!("name={}", rule_name),
        ])
        .output();

    matches!(result, Ok(output) if output.status.success())
}

#[cfg(not(windows))]
pub fn remove_firewall_rule() -> bool {
    true
}

pub fn is_process_running(process_name: &str) -> bool {
    use std::ffi::OsStr;
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, false);
    let found = sys
        .processes_by_name(OsStr::new(process_name))
        .next()
        .is_some();
    found
}

pub fn get_running_process_path(process_name: &str) -> Option<String> {
    use std::ffi::OsStr;
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, false);

    let result = sys
        .processes_by_name(OsStr::new(process_name))
        .next()
        .and_then(|p| p.exe().map(|path| path.to_string_lossy().to_string()));
    result
}

pub fn get_process_pid_by_name(process_name: &str) -> Option<u32> {
    use std::ffi::OsStr;
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, false);

    sys.processes_by_name(OsStr::new(process_name))
        .next()
        .map(|p| p.pid().as_u32())
}

/// Get all child process IDs for a given parent process ID
pub fn get_child_pids(parent_pid: u32) -> Vec<u32> {
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, false);

    let parent = Pid::from_u32(parent_pid);
    sys.processes()
        .iter()
        .filter_map(|(pid, process)| {
            if process.parent() == Some(parent) {
                Some(pid.as_u32())
            } else {
                None
            }
        })
        .collect()
}

/// Get all descendant process IDs (children, grandchildren, etc.) for a given parent process ID
pub fn get_descendant_pids(parent_pid: u32) -> Vec<u32> {
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, false);

    let mut descendants = Vec::new();
    let mut to_check = vec![parent_pid];

    while let Some(pid) = to_check.pop() {
        let parent = Pid::from_u32(pid);
        for (child_pid, process) in sys.processes() {
            if process.parent() == Some(parent) {
                let child_u32 = child_pid.as_u32();
                if !descendants.contains(&child_u32) {
                    descendants.push(child_u32);
                    to_check.push(child_u32);
                }
            }
        }
    }

    descendants
}

/// Terminate a process by PID
pub fn terminate_process(pid: u32) -> bool {
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, false);

    if let Some(process) = sys.process(Pid::from_u32(pid)) {
        process.kill()
    } else {
        false
    }
}

/// Kill all processes with the given name
pub fn kill_processes_by_name(process_name: &str) -> usize {
    use std::ffi::OsStr;
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, false);

    let pids: Vec<Pid> = sys
        .processes_by_name(OsStr::new(process_name))
        .map(|p| p.pid())
        .collect();

    let mut killed = 0;
    for pid in pids {
        if let Some(process) = sys.process(pid) {
            if process.kill() {
                killed += 1;
            }
        }
    }
    killed
}

pub fn find_file_in_path(file_name: &str, search_path: &str) -> bool {
    let paths = std::env::split_paths(search_path);
    for path in paths {
        let pattern = path.join(file_name);
        if let Ok(entries) = glob(pattern.to_str().unwrap_or("")) {
            for entry in entries.flatten() {
                if entry.exists() {
                    return true;
                }
            }
        }
    }
    false
}

//
// Thread-safe wrapper for Windows desktop handle.
//

#[cfg(windows)]
pub struct HiddenDesktop {
    handle: isize,
    pub name: String,
}

#[cfg(windows)]
unsafe impl Send for HiddenDesktop {}
#[cfg(windows)]
unsafe impl Sync for HiddenDesktop {}

#[cfg(windows)]
impl HiddenDesktop {
    pub fn new() -> Option<Self> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::System::StationsAndDesktops::CreateDesktopW;
        use windows::core::PCWSTR;

        let name = format!("PraxisHidden_{}", std::process::id());
        let name_wide: Vec<u16> = OsStr::new(&name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            CreateDesktopW(
                PCWSTR(name_wide.as_ptr()),
                PCWSTR::null(),
                None,
                windows::Win32::System::StationsAndDesktops::DESKTOP_CONTROL_FLAGS(0),
                0x1FF, // GENERIC_ALL access
                None,
            )
        };

        match handle {
            Ok(h) => {
                if h.is_invalid() {
                    None
                } else {
                    Some(Self {
                        handle: h.0 as isize,
                        name,
                    })
                }
            }
            Err(_) => None,
        }
    }
}

#[cfg(windows)]
impl Drop for HiddenDesktop {
    fn drop(&mut self) {
        use windows::Win32::System::StationsAndDesktops::CloseDesktop;

        unsafe {
            let hdesk = std::mem::transmute::<isize, windows::Win32::System::StationsAndDesktops::HDESK>(self.handle);
            let _ = CloseDesktop(hdesk);
        }
    }
}

//
// Spawn a process on a hidden desktop. The process runs normally but on a
// desktop that isn't displayed to the user.
//

#[cfg(windows)]
pub fn spawn_on_hidden_desktop(
    path: &str,
    env_var: &str,
    env_value: &str,
    desktop_name: &str,
) -> anyhow::Result<u32> {
    use anyhow::anyhow;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Threading::{
        CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW, STARTF_USESHOWWINDOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWMAXIMIZED;
    use windows::core::PWSTR;

    //
    // Set environment variable for the current process (inherited by child).
    //

    // SAFETY: We're setting a single env var before spawning a child process,
    // and removing it immediately after. No other threads access this var.
    unsafe { std::env::set_var(env_var, env_value) };

    //
    // Prepare command line.
    //

    let cmd_line = format!("\"{}\"", path);
    let mut cmd_wide: Vec<u16> = OsStr::new(&cmd_line)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    //
    // Prepare desktop name.
    //

    let mut desktop_wide: Vec<u16> = OsStr::new(desktop_name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    si.lpDesktop = PWSTR(desktop_wide.as_mut_ptr());
    si.dwFlags = STARTF_USESHOWWINDOW;
    si.wShowWindow = SW_SHOWMAXIMIZED.0 as u16;

    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    let result = unsafe {
        CreateProcessW(
            None,
            Some(PWSTR(cmd_wide.as_mut_ptr())),
            None,
            None,
            false,
            Default::default(),
            None,
            None,
            &si,
            &mut pi,
        )
    };

    //
    // Remove environment variable.
    //

    // SAFETY: Removing the env var we just set; no other threads access it.
    unsafe { std::env::remove_var(env_var) };

    match result {
        Ok(_) => {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(pi.hProcess);
                let _ = windows::Win32::Foundation::CloseHandle(pi.hThread);
            }
            Ok(pi.dwProcessId)
        }
        Err(e) => Err(anyhow!("CreateProcessW failed: {}", e)),
    }
}
