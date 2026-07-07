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
// Apply the "no console window" creation flag to a tokio Command. Used for
// stdio child processes we spawn asynchronously (MCP servers, ACP agents),
// which would otherwise flash a terminal window on Windows. No-op on other
// platforms.
//

#[cfg(windows)]
pub fn silence_tokio_command(cmd: &mut tokio::process::Command) {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn silence_tokio_command(_cmd: &mut tokio::process::Command) {}

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
            "advfirewall",
            "firewall",
            "show",
            "rule",
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
            "advfirewall",
            "firewall",
            "add",
            "rule",
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

#[allow(dead_code)]
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
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={}", rule_name),
        ])
        .output();

    matches!(result, Ok(output) if output.status.success())
}

#[allow(dead_code)]
#[cfg(not(windows))]
pub fn remove_firewall_rule() -> bool {
    true
}

#[allow(dead_code)]
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

//
// Terminate a process and all its descendants efficiently with a single system scan.
// Kills children first (depth-first), then the parent.
// Returns the number of processes killed.
//

pub fn terminate_process_tree(pid: u32) -> usize {
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, false);

    //
    // First, collect all descendant PIDs using the already-loaded process list.
    //

    let mut descendants = Vec::new();
    let mut to_check = vec![pid];

    while let Some(check_pid) = to_check.pop() {
        let parent = Pid::from_u32(check_pid);
        for (child_pid, process) in sys.processes() {
            if process.parent() == Some(parent) {
                let child_u32 = child_pid.as_u32();
                if !descendants.contains(&child_u32) && child_u32 != pid {
                    descendants.push(child_u32);
                    to_check.push(child_u32);
                }
            }
        }
    }

    //
    // Kill descendants first (in reverse order - deepest children first).
    //

    let mut killed = 0;
    for &dpid in descendants.iter().rev() {
        if let Some(process) = sys.process(Pid::from_u32(dpid)) {
            if process.kill() {
                killed += 1;
            }
        }
    }

    //
    // Then kill the parent.
    //

    if let Some(process) = sys.process(Pid::from_u32(pid)) {
        if process.kill() {
            killed += 1;
        }
    }

    killed
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

//
// Thread-safe wrapper for Windows desktop handle.
//

#[cfg(windows)]
pub struct HiddenDesktop {
    pub handle: isize,
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
            let hdesk = std::mem::transmute::<
                isize,
                windows::Win32::System::StationsAndDesktops::HDESK,
            >(self.handle);
            let _ = CloseDesktop(hdesk);
        }
    }
}

//
// Minimize a process window by PID. On Windows, finds visible windows belonging
// to the process or any of its descendants and minimizes them.
// Returns true if at least one window was minimized.
//

#[cfg(windows)]
mod minimize_impl {
    use crate::utils::LockExt;
    use once_cell::sync::Lazy;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SW_MINIMIZE, ShowWindow,
    };

    static FOUND_HWNDS: Lazy<Mutex<Vec<isize>>> = Lazy::new(|| Mutex::new(Vec::new()));
    static TARGET_PIDS: Lazy<Mutex<HashSet<u32>>> = Lazy::new(|| Mutex::new(HashSet::new()));

    unsafe extern "system" fn enum_callback(hwnd: HWND, _lparam: LPARAM) -> windows::core::BOOL {
        let mut window_pid: u32 = 0;

        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut window_pid));

            let pids = TARGET_PIDS.lock_safe();
            if pids.contains(&window_pid) && IsWindowVisible(hwnd).as_bool() {
                let mut hwnds = FOUND_HWNDS.lock_safe();
                hwnds.push(hwnd.0 as isize);
            }
        }
        windows::core::BOOL(1) // Continue enumeration to find all windows
    }

    pub fn minimize(target_pids: HashSet<u32>) -> bool {
        let pid_count = target_pids.len();
        {
            let mut hwnds = FOUND_HWNDS.lock_safe();
            hwnds.clear();
            let mut pids = TARGET_PIDS.lock_safe();
            *pids = target_pids;
        }

        unsafe {
            let _ = EnumWindows(Some(enum_callback), LPARAM(0));
        }

        let hwnds = FOUND_HWNDS.lock_safe();
        common::log_debug!(
            "minimize: checked {} PIDs, found {} visible windows",
            pid_count,
            hwnds.len()
        );
        let mut minimized = false;
        for &hwnd_value in hwnds.iter() {
            let hwnd = HWND(hwnd_value as *mut _);
            common::log_debug!("minimize: calling ShowWindow for hwnd {:x}", hwnd_value);
            unsafe {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            minimized = true;
        }
        minimized
    }
}

#[cfg(windows)]
pub fn minimize_process_window(pid: u32) -> bool {
    use std::collections::HashSet;

    //
    // Collect all PIDs to check: the parent and all descendants.
    //

    let mut target_pids: HashSet<u32> = HashSet::new();
    target_pids.insert(pid);
    let descendants = get_descendant_pids(pid);
    common::log_debug!(
        "minimize_process_window: parent={}, descendants={:?}",
        pid,
        descendants
    );
    for descendant in descendants {
        target_pids.insert(descendant);
    }

    let result = minimize_impl::minimize(target_pids.clone());
    common::log_debug!(
        "minimize_process_window: target_pids={:?}, result={}",
        target_pids,
        result
    );
    result
}

#[cfg(not(windows))]
pub fn minimize_process_window(_pid: u32) -> bool {
    false
}

//
// Spawn a process on a hidden desktop. The process runs normally but on a
// desktop that isn't displayed to the user.
//

#[cfg(windows)]
pub fn spawn_on_hidden_desktop(
    path: &str,
    env_pair: Option<(&str, &str)>,
    extra_args: &[&str],
    desktop_name: &str,
) -> anyhow::Result<u32> {
    use anyhow::anyhow;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Threading::{
        CreateProcessW, PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWMAXIMIZED;
    use windows::core::PWSTR;

    //
    // Set environment variable for the current process (inherited by child).
    //

    if let Some((var, value)) = env_pair {
        // SAFETY: We're setting a single env var before spawning a child process,
        // and removing it immediately after. No other threads access this var.
        unsafe { std::env::set_var(var, value) };
    }

    //
    // Prepare command line.
    //

    let mut cmd_line = format!("\"{}\"", path);
    for arg in extra_args {
        cmd_line.push(' ');
        cmd_line.push_str(arg);
    }
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

    if let Some((var, _)) = env_pair {
        // SAFETY: Removing the env var we just set; no other threads access it.
        unsafe { std::env::remove_var(var) };
    }

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

//
// Result from spawn_process_detached. Callers decide what to do with the
// hidden desktop (CDP stores it in the connection; standalone callers leak it).
//

pub struct SpawnResult {
    pub pid: u32,
    #[cfg(windows)]
    pub hidden_desktop: Option<HiddenDesktop>,
}

//
// Spawn a process detached (no wait). On Windows, tries hidden desktop first,
// then falls back to normal spawn with minimize. Shared by CDP and standalone
// process launching.
//

pub fn spawn_process_detached(
    path: &str,
    env_pair: Option<(&str, &str)>,
    extra_args: &[&str],
    use_hidden_desktop: bool,
) -> anyhow::Result<SpawnResult> {
    #[cfg(windows)]
    {
        let not_hidden = std::env::var("PRAXIS_NOT_HIDDEN")
            .map(|v| v == "1")
            .unwrap_or(cfg!(debug_assertions));
        let want_hidden = use_hidden_desktop && !not_hidden;

        if !want_hidden {
            common::log_debug!(
                "spawn_process_detached: hidden desktop disabled (use_hidden_desktop={}, not_hidden={})",
                use_hidden_desktop,
                not_hidden
            );
        }

        if want_hidden {
            let desktop = HiddenDesktop::new();
            if let Some(d) = desktop {
                let pid = spawn_on_hidden_desktop(path, env_pair, extra_args, &d.name)?;
                common::log_info!(
                    "spawn_process_detached: spawned on hidden desktop '{}' with PID {}",
                    d.name,
                    pid
                );
                return Ok(SpawnResult {
                    pid,

                    hidden_desktop: Some(d),
                });
            }
            common::log_warn!(
                "spawn_process_detached: failed to create hidden desktop, spawning normally"
            );
        }

        let mut cmd = std::process::Command::new(path);
        if let Some((var, val)) = env_pair {
            cmd.env(var, val);
        }
        for arg in extra_args {
            cmd.arg(arg);
        }
        let process = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn {}: {}", path, e))?;
        let pid = process.id();
        common::log_info!("spawn_process_detached: spawned with PID {}", pid);
        Ok(SpawnResult {
            pid,

            hidden_desktop: None,
        })
    }

    #[cfg(not(windows))]
    {
        let _ = use_hidden_desktop;
        let mut cmd = std::process::Command::new(path);
        if let Some((var, val)) = env_pair {
            cmd.env(var, val);
        }
        for arg in extra_args {
            cmd.arg(arg);
        }
        let process = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn {}: {}", path, e))?;
        let pid = process.id();
        common::log_info!("spawn_process_detached: spawned with PID {}", pid);
        Ok(SpawnResult { pid })
    }
}
