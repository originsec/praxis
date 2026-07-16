use std::io;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use sysinfo::{Pid, ProcessesToUpdate, System};

///
/// Default bound for intercept host child processes (iptables, netsh, sysctl,
/// cert tools, etc.). Timeout is treated as unknown host state: callers must
/// retain write-ahead recovery / CleanupRequired rather than claiming success.
///
pub const HOST_CMD_TIMEOUT: Duration = Duration::from_secs(30);

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
// Run a fully-configured Command with a wall-clock timeout. On timeout the
// child is killed; the error kind is TimedOut so callers can retain recovery
// ownership (unknown host state). Prefer this over Command::output() on
// intercept enable/disable/cleanup paths.
//

pub fn output_with_timeout(cmd: &mut Command, timeout: Duration) -> io::Result<Output> {
    //
    // Own a process group/session so timeout can kill the whole tree.
    //
    prepare_owned_process_group(cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn()?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(_) => {
            kill_owned_process_tree(pid);
            //
            // Reap the killed tree synchronously in the common case. The
            // waiter thread owns the Child and is detached: if the process
            // does not die within this window (e.g. uninterruptible I/O), the
            // thread stays blocked in wait_with_output until the kill finally
            // takes effect, then exits on its own. It is not leaked
            // permanently unless the process is truly unkillable.
            //
            let _ = rx.recv_timeout(Duration::from_secs(2));
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "host command timed out after {}s (pid {}); process tree kill attempted; treat host state as unknown",
                    timeout.as_secs(),
                    pid
                ),
            ))
        }
    }
}

/// Spawn children in their own process group (Unix) for tree kill on timeout.
fn prepare_owned_process_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        //
        // process_group(0) makes the child the leader of a new group equal to
        // its pid so kill(-pid) targets the whole tree.
        //
        cmd.process_group(0);
    }
    let _ = cmd;
}

/// Kill the owned process tree created by [`prepare_owned_process_group`].
fn kill_owned_process_tree(pid: u32) {
    #[cfg(unix)]
    {
        unsafe {
            //
            // Negative pid = process group. Also kill the pid directly in case
            // process_group setup failed on a platform edge.
            //
            let _ = libc::kill(-(pid as i32), libc::SIGKILL);
            let _ = libc::kill(pid as i32, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        //
        // Bounded taskkill: never hang the timeout path on an unbounded helper.
        //
        let mut kill_cmd = silent_command("taskkill");
        kill_cmd
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Ok(mut child) = kill_cmd.spawn() {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(child.wait());
            });
            if rx.recv_timeout(Duration::from_secs(3)).is_err() {
                //
                // Last resort: kill taskkill itself by pid is unavailable;
                // abandon after 3s so Reset/Disable stay responsive.
                //
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

/// Convenience: silent_command(program) + args + HOST_CMD_TIMEOUT.
#[allow(dead_code)] // used by unit tests + call sites that prefer program/args form
pub fn run_host_output(program: &str, args: &[&str]) -> io::Result<Output> {
    let mut cmd = silent_command(program);
    cmd.args(args);
    output_with_timeout(&mut cmd, HOST_CMD_TIMEOUT)
}



/// Pure: map I/O error from timed host commands to a cleanup-oriented message.
#[allow(dead_code)] // pure helper for call sites and unit tests
pub fn host_cmd_timeout_message(err: &io::Error) -> Option<String> {
    if err.kind() == io::ErrorKind::TimedOut {
        Some(err.to_string())
    } else {
        None
    }
}

///
/// Classify a host-command I/O error for cleanup paths that claim restored
/// state. TimedOut (and other hard I/O failures) must not be treated as
/// success; NotFound is optional tooling absence.
///
pub fn host_cleanup_io_is_failure(err: &io::Error) -> bool {
    match err.kind() {
        io::ErrorKind::TimedOut => true,
        io::ErrorKind::NotFound => false,
        _ => true,
    }
}

///
/// Pure classification for systemd unset-environment results used by
/// `unset_systemd_user_env`. Unit tests drive this shipped helper.
///
pub fn classify_systemd_unset_result(
    result: Result<std::process::Output, io::Error>,
) -> Result<(), String> {
    match result {
        Ok(o) if o.status.success() => Ok(()),
        //
        // systemctl --user unset-environment already succeeds when the
        // variables are absent. A non-zero exit is a real failure (e.g. no
        // user manager); do not claim cleanup succeeded.
        //
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let detail = stderr.trim();
            if detail.is_empty() {
                Err(format!(
                    "systemctl unset-environment exited with {}; host env state unknown",
                    o.status
                ))
            } else {
                Err(format!(
                    "systemctl unset-environment failed ({}): {}; host env state unknown",
                    o.status, detail
                ))
            }
        }
        Err(e) if !host_cleanup_io_is_failure(&e) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::TimedOut => Err(format!(
            "systemctl unset-environment timed out; host env state unknown: {}",
            e
        )),
        Err(e) => Err(format!("systemctl unset-environment failed: {}", e)),
    }
}

///
/// Drop-in for `Command::output()` on intercept paths: same builder API, but
/// bounded wait + kill on timeout.
///
pub trait CommandOutputBounded {
    fn output_bounded(&mut self) -> io::Result<Output>;
}

impl CommandOutputBounded for Command {
    fn output_bounded(&mut self) -> io::Result<Output> {
        output_with_timeout(self, HOST_CMD_TIMEOUT)
    }
}

#[cfg(test)]
mod host_cmd_tests {
    use super::{host_cmd_timeout_message, run_host_output, HOST_CMD_TIMEOUT};
    use std::io;
    use std::time::Duration;

    #[test]
    fn host_cmd_timeout_message_detects_timed_out() {
        let err = io::Error::new(io::ErrorKind::TimedOut, "timed out");
        assert!(host_cmd_timeout_message(&err).is_some());
        let other = io::Error::new(io::ErrorKind::NotFound, "missing");
        assert!(host_cmd_timeout_message(&other).is_none());
    }

    #[test]
    fn classify_systemd_unset_treats_timed_out_as_cleanup_failure() {
        use super::{classify_systemd_unset_result, host_cleanup_io_is_failure};
        let timed = io::Error::new(io::ErrorKind::TimedOut, "host command timed out");
        assert!(host_cleanup_io_is_failure(&timed));
        let err = classify_systemd_unset_result(Err(timed)).unwrap_err();
        assert!(
            err.contains("timed out") || err.contains("unknown"),
            "expected timeout failure message, got: {err}"
        );
        // NotFound (no systemctl) is not a hard cleanup failure.
        let missing = io::Error::new(io::ErrorKind::NotFound, "no systemctl");
        assert!(!host_cleanup_io_is_failure(&missing));
        assert!(classify_systemd_unset_result(Err(missing)).is_ok());
    }

    #[test]
    fn classify_systemd_unset_treats_nonzero_exit_as_cleanup_failure() {
        use super::classify_systemd_unset_result;
        use std::process::Output;

        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            use std::process::ExitStatus;

            let failed = Output {
                status: ExitStatus::from_raw(1 << 8),
                stdout: Vec::new(),
                stderr: b"Failed to connect to bus".to_vec(),
            };
            let err = classify_systemd_unset_result(Ok(failed)).unwrap_err();
            assert!(
                err.contains("Failed to connect to bus") || err.contains("unknown"),
                "expected non-zero failure message, got: {err}"
            );

            let ok = Output {
                status: ExitStatus::from_raw(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
            assert!(classify_systemd_unset_result(Ok(ok)).is_ok());
        }
        #[cfg(not(unix))]
        {
            let _ = classify_systemd_unset_result;
            let _ = Output {
                status: Default::default(),
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
        }
    }

    #[test]
    fn run_host_output_true_exits_quickly() {
        //
        // Drive the real bounded runner with a trivial command.
        //
        #[cfg(unix)]
        {
            let out = run_host_output("true", &[]).expect("true should run");
            assert!(out.status.success());
        }
        let _ = HOST_CMD_TIMEOUT;
        let _ = Duration::from_secs(1);
    }

    #[test]
    fn run_host_output_times_out_long_sleep() {
        #[cfg(unix)]
        {
            use super::output_with_timeout;
            use super::silent_command;
            let mut cmd = silent_command("sleep");
            cmd.arg("30");
            let err = output_with_timeout(&mut cmd, Duration::from_millis(200)).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::TimedOut);
            assert!(host_cmd_timeout_message(&err).is_some());
        }
    }
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
// Intercept-specific Windows firewall identity (unique per listen port).
// Must not use a generic "Praxis Node" name that collides with user rules.
// Pure helpers are also exercised by unit tests; call sites are Windows-only.
//

#[cfg_attr(not(windows), allow(dead_code))]
pub fn intercept_firewall_rule_name(port: u16) -> String {
    format!("Praxis Intercept VPN port {}", port)
}

/// Build netsh add-rule args. Pure helper for unit tests and the I/O wrapper.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn firewall_rule_add_args(exe_path: &str, port: u16) -> Vec<String> {
    let name = intercept_firewall_rule_name(port);
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "add".into(),
        "rule".into(),
        format!("name={}", name),
        "dir=in".into(),
        "action=allow".into(),
        format!("program={}", exe_path),
        "protocol=tcp".into(),
        format!("localport={}", port),
        "enable=yes".into(),
    ]
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn firewall_rule_delete_args(rule_name: &str) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "delete".into(),
        "rule".into(),
        format!("name={}", rule_name),
    ]
}

//
// Write-ahead ownership of an intercept firewall rule. Create-failure
// cleanup may only clear this after exact named removal is confirmed.
//

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct FirewallOwnershipState {
    pub added: bool,
    pub name: Option<String>,
    pub port: Option<u16>,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl FirewallOwnershipState {
    pub fn write_ahead(name: String, port: u16) -> Self {
        Self {
            added: true,
            name: Some(name),
            port: Some(port),
        }
    }

    /// After create failed: clear only when exact-rule removal is confirmed
    /// (success or idempotent no-match). Otherwise retain ownership for recovery.
    pub fn after_create_failed(self, remove_confirmed: bool) -> Self {
        if remove_confirmed {
            Self {
                added: false,
                name: None,
                port: None,
            }
        } else {
            self
        }
    }
}

//
// Classify `netsh advfirewall firewall show rule` output. Exit status alone is
// not proof of a match — Windows may return success with "No rules match".
// Ownership requires the expected rule name and local port in the listing.
//

#[cfg_attr(not(windows), allow(dead_code))]
pub fn firewall_show_indicates_owned_rule(
    stdout: &str,
    stderr: &str,
    expected_name: &str,
    expected_local_port: u16,
) -> bool {
    let full = format!("{}\n{}", stdout, stderr);
    let lower = full.to_ascii_lowercase();
    if lower.contains("no rules match") || lower.contains("no rule match") {
        return false;
    }
    if !lower.contains(&expected_name.to_ascii_lowercase()) {
        return false;
    }
    let port = expected_local_port.to_string();
    for line in full.lines() {
        let line_l = line.to_ascii_lowercase();
        if !line_l.contains("localport") {
            continue;
        }
        //
        // Match the port as a whole token among non-digit separators so
        // "443" does not match "4430" and vice versa.
        //
        let mut start = 0usize;
        let bytes = line_l.as_bytes();
        while start < bytes.len() {
            if !bytes[start].is_ascii_digit() {
                start += 1;
                continue;
            }
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if &line_l[start..end] == port {
                return true;
            }
            start = end;
        }
    }
    false
}

/// True when delete output indicates success or idempotent no-match.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn firewall_delete_confirmed(stdout: &str, stderr: &str, process_success: bool) -> bool {
    if process_success {
        return true;
    }
    let message = format!("{} {}", stdout, stderr).to_ascii_lowercase();
    message.contains("no rules match") || message.contains("no rule match")
}

#[cfg(windows)]
pub fn firewall_rule_exists(rule_name: &str, port: u16) -> bool {
    match silent_command("netsh")
        .args([
            "advfirewall",
            "firewall",
            "show",
            "rule",
            &format!("name={}", rule_name),
        ])
        .output_bounded()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            firewall_show_indicates_owned_rule(&stdout, &stderr, rule_name, port)
        }
        Err(_) => false,
    }
}

#[allow(dead_code)]
#[cfg(not(windows))]
pub fn firewall_rule_exists(_rule_name: &str, _port: u16) -> bool {
    false
}

/// Create the port-scoped intercept VPN rule. Never deletes other rules.
#[cfg(windows)]
pub fn ensure_firewall_rule_for_port(port: u16) -> bool {
    let exe_path = match std::env::current_exe() {
        Ok(path) => path.to_string_lossy().to_string(),
        Err(_) => return false,
    };
    let rule_name = intercept_firewall_rule_name(port);
    if firewall_rule_exists(&rule_name, port) {
        return true;
    }
    let args = firewall_rule_add_args(&exe_path, port);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut cmd = silent_command("netsh");
    cmd.args(arg_refs);
    let result = output_with_timeout(&mut cmd, HOST_CMD_TIMEOUT);
    matches!(result, Ok(output) if output.status.success())
}

#[allow(dead_code)]
#[cfg(not(windows))]
pub fn ensure_firewall_rule_for_port(_port: u16) -> bool {
    true
}

/// Remove only the intercept-owned rule with the given exact name.
#[cfg(windows)]
pub fn remove_firewall_rule_named(rule_name: &str) -> bool {
    let args = firewall_rule_delete_args(rule_name);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut cmd = silent_command("netsh");
    cmd.args(arg_refs);
    match output_with_timeout(&mut cmd, HOST_CMD_TIMEOUT) {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            firewall_delete_confirmed(&stdout, &stderr, output.status.success())
        }
        Err(_) => false,
    }
}

#[allow(dead_code)]
#[cfg(not(windows))]
pub fn remove_firewall_rule_named(_rule_name: &str) -> bool {
    true
}

/// Cleanup path for pre-unique-name rules named "Praxis Node".
#[cfg(windows)]
pub fn remove_legacy_firewall_rule() -> bool {
    remove_firewall_rule_named("Praxis Node")
}

#[allow(dead_code)]
#[cfg(not(windows))]
pub fn remove_legacy_firewall_rule() -> bool {
    true
}

#[cfg(test)]
mod firewall_tests {
    use super::{
        firewall_delete_confirmed, firewall_rule_add_args, firewall_rule_delete_args,
        firewall_show_indicates_owned_rule, intercept_firewall_rule_name, FirewallOwnershipState,
    };

    #[test]
    fn firewall_rule_is_port_scoped_and_intercept_named() {
        let port = 54321u16;
        let name = intercept_firewall_rule_name(port);
        assert!(name.contains("Intercept"));
        assert!(name.contains(&port.to_string()));
        assert_ne!(name, "Praxis Node");

        let args = firewall_rule_add_args(r"C:\praxis_node.exe", port);
        assert!(args.iter().any(|a| a == &format!("name={}", name)));
        assert!(args.iter().any(|a| a == "localport=54321"));
        assert!(args.iter().any(|a| a.starts_with("program=")));
        assert!(!args.iter().any(|a| a == "name=Praxis Node"));

        let del = firewall_rule_delete_args(&name);
        assert!(del.iter().any(|a| a == &format!("name={}", name)));
    }

    #[test]
    fn ownership_cleared_only_after_confirmed_remove() {
        let name = intercept_firewall_rule_name(9);
        let owned = FirewallOwnershipState::write_ahead(name.clone(), 9);
        assert!(owned.added);
        assert_eq!(owned.name.as_deref(), Some(name.as_str()));

        let retained = owned.clone().after_create_failed(false);
        assert!(retained.added);
        assert_eq!(retained.name.as_deref(), Some(name.as_str()));
        assert_eq!(retained.port, Some(9));

        let cleared = owned.after_create_failed(true);
        assert!(!cleared.added);
        assert!(cleared.name.is_none());
        assert!(cleared.port.is_none());
    }

    #[test]
    fn show_output_rejects_no_rules_match() {
        let name = intercept_firewall_rule_name(54321);
        assert!(!firewall_show_indicates_owned_rule(
            "No rules match the specified criteria.",
            "",
            &name,
            54321
        ));
        assert!(!firewall_show_indicates_owned_rule(
            "",
            "No rules match the specified criteria.",
            &name,
            54321
        ));
        //
        // Exit-status-style empty success with no properties is not ownership.
        //
        assert!(!firewall_show_indicates_owned_rule("", "", &name, 54321));
    }

    #[test]
    fn show_output_requires_name_and_local_port() {
        let name = intercept_firewall_rule_name(54321);
        let listing = format!(
            "Rule Name:                            {name}\n\
             Enabled:                              Yes\n\
             Direction:                            In\n\
             Profiles:                             Domain,Private,Public\n\
             Grouping:\n\
             LocalIP:                              Any\n\
             RemoteIP:                             Any\n\
             Protocol:                             TCP\n\
             LocalPort:                            54321\n\
             RemotePort:                           Any\n\
             Edge traversal:                       No\n\
             Action:                               Allow\n"
        );
        assert!(firewall_show_indicates_owned_rule(
            &listing, "", &name, 54321
        ));
        // Wrong port in listing — not our ownership.
        let wrong_port = listing.replace("54321", "443");
        assert!(!firewall_show_indicates_owned_rule(
            &wrong_port, "", &name, 54321
        ));
        // Name mismatch.
        assert!(!firewall_show_indicates_owned_rule(
            &listing, "", "Other Rule", 54321
        ));
        // Port substring must not false-match (443 vs 4430).
        let port4430 = listing
            .replace("54321", "4430")
            .replace(&name, &intercept_firewall_rule_name(4430));
        assert!(!firewall_show_indicates_owned_rule(
            &port4430,
            "",
            &intercept_firewall_rule_name(4430),
            443
        ));
    }

    #[test]
    fn delete_confirmed_on_success_or_no_match() {
        assert!(firewall_delete_confirmed("Ok.", "", true));
        assert!(firewall_delete_confirmed(
            "No rules match the specified criteria.",
            "",
            false
        ));
        assert!(!firewall_delete_confirmed("Access is denied.", "", false));
    }
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
