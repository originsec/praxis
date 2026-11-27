use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

struct ChildEnvironment {
    active: Vec<(String, String)>,
    recovered: BTreeMap<String, Option<String>>,
}

static CHILD_ENV: RwLock<ChildEnvironment> = RwLock::new(ChildEnvironment {
    active: Vec::new(),
    recovered: BTreeMap::new(),
});

fn intercept_environment(cert_path: &Path, proxy_addr: Option<&str>) -> Vec<(String, String)> {
    let cert = cert_path.to_string_lossy().into_owned();
    let mut vars = vec![
        ("NODE_EXTRA_CA_CERTS".into(), cert.clone()),
        ("CODEX_CA_CERTIFICATE".into(), cert),
    ];
    if let Some(addr) = proxy_addr {
        let url = format!("http://{addr}");
        for name in ["HTTP_PROXY", "HTTPS_PROXY"] {
            vars.push((name.into(), url.clone()));
        }
        #[cfg(target_os = "linux")]
        {
            for name in ["http_proxy", "https_proxy"] {
                vars.push((name.into(), url.clone()));
            }
            for name in ["NO_PROXY", "no_proxy"] {
                vars.push((name.into(), "localhost,127.0.0.1".into()));
            }
        }
    }
    vars
}

pub fn apply_to_command(command: &mut std::process::Command) {
    let (active, recovered) = {
        let env = CHILD_ENV.read().unwrap_or_else(|e| e.into_inner());
        (env.active.clone(), env.recovered.clone())
    };
    for (name, original) in recovered {
        if let Some(value) = original {
            command.env(name, value);
        } else {
            command.env_remove(name);
        }
    }
    command.envs(active);
}

fn publish_child_environment(vars: Vec<(String, String)>) {
    CHILD_ENV.write().unwrap_or_else(|e| e.into_inner()).active = vars;
}

fn inherited_intercept_environment() -> BTreeMap<String, String> {
    [
        "CODEX_CA_CERTIFICATE",
        "NODE_EXTRA_CA_CERTS",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "http_proxy",
        "https_proxy",
        "NO_PROXY",
        "no_proxy",
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok().map(|value| (name.into(), value)))
    .collect()
}

fn clear_child_environment() {
    let cert = cert_export_path();
    let inherited = inherited_intercept_environment();
    let mut names = legacy_environment_names(&inherited, &cert);
    if inherited.get("CODEX_CA_CERTIFICATE").map(String::as_str)
        == Some(cert.to_string_lossy().as_ref())
    {
        names.push("CODEX_CA_CERTIFICATE".into());
    }
    let mut env = CHILD_ENV.write().unwrap_or_else(|e| e.into_inner());
    env.active.clear();
    for name in names {
        env.recovered.entry(name).or_insert(None);
    }
}

fn remember_restored_child_environment(original: BTreeMap<String, Option<String>>) {
    let recovered = restored_child_environment(
        original,
        &inherited_intercept_environment(),
        &cert_export_path(),
    );
    CHILD_ENV
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .recovered
        .extend(recovered);
}

fn restored_child_environment(
    original: BTreeMap<String, Option<String>>,
    inherited: &BTreeMap<String, String>,
    cert: &Path,
) -> BTreeMap<String, Option<String>> {
    let mut names = legacy_environment_names(inherited, cert);
    if inherited.get("CODEX_CA_CERTIFICATE").map(String::as_str)
        == Some(cert.to_string_lossy().as_ref())
    {
        names.push("CODEX_CA_CERTIFICATE".into());
    }
    original
        .into_iter()
        .filter(|(name, _)| names.contains(name))
        .collect()
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn backup_path(name: &str) -> Result<PathBuf> {
    Ok(dirs::data_local_dir()
        .context("Could not determine environment recovery directory")?
        .join("praxis")
        .join(name))
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn save_backup<T: serde::Serialize>(path: &Path, backup: &T) -> Result<()> {
    use std::io::Write;
    anyhow::ensure!(
        !path.exists(),
        "Environment recovery already exists: {}",
        path.display()
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&serde_json::to_vec(backup)?)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn load_backup<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn remove_exported_cert() -> Result<()> {
    match std::fs::remove_file(cert_export_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("Failed to remove exported CA certificate"),
    }
}
#[cfg(target_os = "linux")]
use crate::utils::CommandOutputBounded;

/// Path where we store the exported root CA certificate
pub fn cert_export_path() -> PathBuf {
    std::env::temp_dir()
        .join("praxis_certs")
        .join("praxis_root_ca.pem")
}

/// Export the root CA certificate to a file for NODE_EXTRA_CA_CERTS
pub fn export_ca_cert(cert_pem: &str) -> Result<PathBuf> {
    let cert_path = cert_export_path();

    //
    // Ensure directory exists.
    //
    if let Some(parent) = cert_path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create certificate directory")?;
    }

    std::fs::write(&cert_path, cert_pem).context("Failed to write CA certificate")?;

    common::log_info!("Exported root CA certificate to: {}", cert_path.display());
    Ok(cert_path)
}

#[cfg(target_os = "windows")]
#[derive(serde::Serialize, serde::Deserialize)]
struct RegistryValue {
    bytes: Vec<u8>,
    kind: u32,
}

#[cfg(target_os = "windows")]
impl RegistryValue {
    fn to_raw(&self) -> Result<winreg::RegValue> {
        use winreg::enums::*;
        let kinds = [
            REG_NONE,
            REG_SZ,
            REG_EXPAND_SZ,
            REG_BINARY,
            REG_DWORD,
            REG_DWORD_BIG_ENDIAN,
            REG_LINK,
            REG_MULTI_SZ,
            REG_RESOURCE_LIST,
            REG_FULL_RESOURCE_DESCRIPTOR,
            REG_RESOURCE_REQUIREMENTS_LIST,
            REG_QWORD,
        ];
        let vtype = kinds
            .into_iter()
            .find(|kind| kind.clone() as u32 == self.kind)
            .context("Unknown registry environment value type in recovery file")?;
        Ok(winreg::RegValue {
            bytes: self.bytes.clone(),
            vtype,
        })
    }
}

#[cfg(target_os = "windows")]
fn expand_windows_environment(value: &str) -> Result<String> {
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    use windows::core::PCWSTR;
    let wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    let needed = unsafe { ExpandEnvironmentStringsW(PCWSTR(wide.as_ptr()), None) };
    anyhow::ensure!(
        needed > 0,
        "Failed to expand original Windows environment value"
    );
    let mut expanded = vec![0u16; needed as usize];
    let written = unsafe { ExpandEnvironmentStringsW(PCWSTR(wide.as_ptr()), Some(&mut expanded)) };
    anyhow::ensure!(
        written > 0 && written <= needed,
        "Failed to expand original Windows environment value"
    );
    Ok(String::from_utf16(&expanded[..written as usize - 1])?)
}

#[cfg(target_os = "windows")]
fn notify_windows_environment() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    let environment: Vec<u16> = "Environment\0".encode_utf16().collect();
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(environment.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            100,
            None,
        );
    }
}

#[cfg(target_os = "windows")]
pub fn set_intercept_env_vars(cert_path: &PathBuf, proxy_addr: Option<&str>) -> Result<()> {
    use winreg::{RegKey, enums::*};
    let vars = intercept_environment(cert_path, proxy_addr);
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
    let mut backup = BTreeMap::<String, Option<RegistryValue>>::new();
    for (name, _) in &vars {
        let original = match key.get_raw_value(name) {
            Ok(value) => Some(RegistryValue {
                bytes: value.bytes,
                kind: value.vtype as u32,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).context("Failed to read original user environment"),
        };
        backup.insert(name.clone(), original);
    }
    save_backup(&backup_path("intercept_registry_env.json")?, &backup)?;
    for (name, value) in &vars {
        key.set_value(name, value)
            .with_context(|| format!("Failed to set {name}"))?;
    }
    notify_windows_environment();
    publish_child_environment(vars);
    Ok(())
}

fn legacy_environment_names(environment: &BTreeMap<String, String>, cert: &Path) -> Vec<String> {
    if environment.get("NODE_EXTRA_CA_CERTS").map(String::as_str)
        != Some(cert.to_string_lossy().as_ref())
    {
        return Vec::new();
    }
    let mut names = vec!["NODE_EXTRA_CA_CERTS".into()];
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
        if environment.get(name).is_some_and(|value| {
            value
                .strip_prefix("http://127.0.0.1:")
                .is_some_and(|port| port.parse::<u16>().is_ok())
        }) {
            names.push(name.into());
        }
    }
    for name in ["NO_PROXY", "no_proxy"] {
        if environment
            .get(name)
            .is_some_and(|value| value == "localhost,127.0.0.1")
        {
            names.push(name.into());
        }
    }
    names
}

#[cfg(target_os = "windows")]
pub fn remove_intercept_env_vars() -> Result<()> {
    use winreg::{RegKey, enums::*};
    clear_child_environment();
    let path = backup_path("intercept_registry_env.json")?;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
    if let Some(backup) = load_backup::<BTreeMap<String, Option<RegistryValue>>>(&path)? {
        let mut child_original = BTreeMap::new();
        for (name, original) in backup {
            if let Some(value) = original {
                use winreg::types::FromRegValue;
                let raw = value.to_raw()?;
                if let Ok(value) = String::from_reg_value(&raw) {
                    let value = if raw.vtype == REG_EXPAND_SZ {
                        expand_windows_environment(&value)?
                    } else {
                        value
                    };
                    child_original.insert(name.clone(), Some(value));
                }
                key.set_raw_value(&name, &value.to_raw()?)?;
            } else {
                child_original.insert(name.clone(), None);
                match key.delete_value(&name) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                }
            }
        }
        remember_restored_child_environment(child_original);
        std::fs::remove_file(path)?;
    } else {
        let environment = ["NODE_EXTRA_CA_CERTS", "HTTP_PROXY", "HTTPS_PROXY"]
            .into_iter()
            .filter_map(|name| {
                key.get_value::<String, _>(name)
                    .ok()
                    .map(|value| (name.into(), value))
            })
            .collect();
        for name in legacy_environment_names(&environment, &cert_export_path()) {
            key.delete_value(&name)?;
        }
    }
    notify_windows_environment();
    remove_exported_cert()
}

/// Linux: set environment variables via shell profile modification
///
/// When running as root, this will configure env vars for all users with
/// home directories in /home/.
#[cfg(target_os = "linux")]
pub fn set_intercept_env_vars(cert_path: &PathBuf, proxy_addr: Option<&str>) -> Result<()> {
    let vars = intercept_environment(cert_path, proxy_addr);
    let mut script_content = String::from("# Praxis intercept environment variables\n");
    for (name, value) in &vars {
        script_content.push_str(&format!("export {name}={}\n", shell_quote(value)));
    }

    //
    // Get all home directories to configure.
    //
    let home_dirs = get_all_home_dirs();
    common::log_info!(
        "Configuring intercept env vars for {} user(s)",
        home_dirs.len()
    );

    for home_dir in &home_dirs {
        set_intercept_env_vars_for_home(home_dir, &script_content)
            .with_context(|| format!("Failed to configure env vars for {}", home_dir.display()))?;
    }

    //
    // Try to set systemd user environment (for user services).
    //
    set_systemd_user_env(&vars)?;
    publish_child_environment(vars);

    common::log_info!("Environment variables configured for new terminal sessions");

    Ok(())
}

/// Set intercept env vars for a specific home directory
#[cfg(target_os = "linux")]
fn set_intercept_env_vars_for_home(home_dir: &PathBuf, script_content: &str) -> Result<()> {
    //
    // Create the praxis config directory for this user.
    //
    let config_dir = home_dir.join(".config").join("praxis");
    std::fs::create_dir_all(&config_dir).context("Failed to create praxis config directory")?;

    //
    // Write the proxy environment script.
    //
    let env_script_path = config_dir.join("proxy_env.sh");
    std::fs::write(&env_script_path, script_content)
        .context("Failed to write proxy environment script")?;

    //
    // Try to fix ownership if running as root.
    //
    fix_ownership_for_home(home_dir, &config_dir);
    fix_ownership_for_home(home_dir, &env_script_path);

    common::log_info!(
        "Created proxy environment script: {}",
        env_script_path.display()
    );

    //
    // Add source line to shell profiles.
    //
    let script = shell_quote(&env_script_path.to_string_lossy());
    let source_line = format!("[ -f {script} ] && . {script}");

    for profile_path in get_shell_profiles_for_home(home_dir) {
        add_to_shell_profile(&profile_path, &source_line)
            .with_context(|| format!("Failed to update {}", profile_path.display()))?;
    }

    Ok(())
}

/// Linux: remove intercept environment variables
///
/// When running as root, this will clean up env vars for all users with
/// home directories in /home/.
#[cfg(target_os = "linux")]
pub fn remove_intercept_env_vars() -> Result<()> {
    clear_child_environment();
    //
    // Get all home directories to clean up.
    //
    let home_dirs = get_all_home_dirs();
    common::log_info!(
        "Removing intercept env vars for {} user(s)",
        home_dirs.len()
    );

    let mut failures = Vec::new();
    for home_dir in &home_dirs {
        if let Err(e) = remove_intercept_env_vars_for_home(home_dir) {
            failures.push(format!("{}: {}", home_dir.display(), e));
        }
    }

    //
    // Unset systemd user environment.
    //
    if let Err(e) = unset_systemd_user_env() {
        failures.push(format!("systemd user env: {}", e));
    }

    //
    // Clean up the exported cert file.
    //
    if failures.is_empty() {
        if let Err(e) = remove_exported_cert() {
            failures.push(e.to_string());
        }
    }

    if failures.is_empty() {
        common::log_info!("Removed intercept environment variables");
        Ok(())
    } else {
        anyhow::bail!(
            "intercept env cleanup incomplete (host state may be unknown): {}",
            failures.join("; ")
        )
    }
}

/// Remove intercept env vars for a specific home directory
#[cfg(target_os = "linux")]
fn remove_intercept_env_vars_for_home(home_dir: &PathBuf) -> Result<()> {
    //
    // Remove source lines from shell profiles.
    //
    let mut failures = Vec::new();
    for profile_path in get_shell_profiles_for_home(home_dir) {
        if let Err(e) = remove_from_shell_profile(&profile_path) {
            failures.push(format!("{}: {e}", profile_path.display()));
        }
    }
    let env_script_path = home_dir.join(".config/praxis/proxy_env.sh");
    match std::fs::remove_file(&env_script_path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => failures.push(format!("{}: {e}", env_script_path.display())),
    }
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));

    Ok(())
}

/// Get all home directories to configure.
///
/// When running as root, returns all user home directories in /home/ plus /root.
/// Otherwise, returns just the current user's home directory.
#[cfg(target_os = "linux")]
fn get_all_home_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    //
    // Check if running as root.
    //
    let is_root = unsafe { libc::geteuid() == 0 };

    if is_root {
        //
        // Include /root if it exists.
        //
        let root_home = PathBuf::from("/root");
        if root_home.exists() {
            dirs.push(root_home);
        }

        //
        // Enumerate all user directories in /home/.
        //
        if let Ok(entries) = std::fs::read_dir("/home") {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                }
            }
        }

        if dirs.is_empty() {
            //
            // Fallback to current HOME if no dirs found.
            //
            if let Ok(home) = std::env::var("HOME") {
                dirs.push(PathBuf::from(home));
            }
        }
    } else {
        //
        // Not root - just use current user's home.
        //
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home));
        }
    }

    dirs
}

/// Get list of shell profile paths that exist for a given home directory
#[cfg(target_os = "linux")]
fn get_shell_profiles_for_home(home: &PathBuf) -> Vec<PathBuf> {
    [".bashrc", ".zshrc", ".profile"]
        .iter()
        .map(|p| home.join(p))
        .filter(|p| p.exists())
        .collect()
}

/// Fix ownership of a file/directory to match the home directory owner.
///
/// When running as root and creating files in user home directories,
/// we need to chown them to the correct user.
#[cfg(target_os = "linux")]
fn fix_ownership_for_home(home_dir: &PathBuf, path: &PathBuf) {
    use std::os::unix::fs::MetadataExt;

    //
    // Only do this if running as root.
    //
    if unsafe { libc::geteuid() != 0 } {
        return;
    }

    //
    // Get the uid/gid of the home directory.
    //
    let metadata = match std::fs::metadata(home_dir) {
        Ok(m) => m,
        Err(_) => return,
    };

    let uid = metadata.uid();
    let gid = metadata.gid();

    //
    // Change ownership of the path.
    //
    let c_path = match std::ffi::CString::new(path.to_string_lossy().as_bytes()) {
        Ok(p) => p,
        Err(_) => return,
    };

    unsafe {
        libc::chown(c_path.as_ptr(), uid, gid);
    }
}

#[cfg(target_os = "linux")]
const PRAXIS_MARKER_START: &str = "# PRAXIS-INTERCEPT-START";
#[cfg(target_os = "linux")]
const PRAXIS_MARKER_END: &str = "# PRAXIS-INTERCEPT-END";

/// Add the source line to a shell profile
#[cfg(target_os = "linux")]
fn add_to_shell_profile(profile_path: &PathBuf, source_line: &str) -> Result<()> {
    use std::io::Write;

    let content = std::fs::read_to_string(profile_path).context("Failed to read shell profile")?;

    //
    // Check if already present.
    //
    if content.contains(PRAXIS_MARKER_START) {
        return Ok(());
    }

    //
    // Append the source block.
    //
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(profile_path)
        .context("Failed to open shell profile for writing")?;

    writeln!(file)?;
    writeln!(file, "{}", PRAXIS_MARKER_START)?;
    writeln!(file, "{}", source_line)?;
    writeln!(file, "{}", PRAXIS_MARKER_END)?;

    Ok(())
}

/// Remove the praxis source lines from a shell profile
#[cfg(target_os = "linux")]
fn remove_from_shell_profile(profile_path: &PathBuf) -> Result<()> {
    let content = std::fs::read_to_string(profile_path).context("Failed to read shell profile")?;

    let Some(start) = content.find(PRAXIS_MARKER_START) else {
        return Ok(());
    };
    let end = content[start..]
        .find(PRAXIS_MARKER_END)
        .map(|offset| start + offset + PRAXIS_MARKER_END.len())
        .context("Incomplete Praxis environment block in shell profile")?;
    let start = if content[..start].ends_with('\n') {
        start - 1
    } else {
        start
    };
    let end = if content[end..].starts_with('\n') {
        end + 1
    } else {
        end
    };
    let restored = format!("{}{}", &content[..start], &content[end..]);
    std::fs::write(profile_path, restored).context("Failed to write updated shell profile")?;

    Ok(())
}

//
// Whether a systemd user manager is reachable. `systemctl --user` needs a
// user D-Bus session; when the node runs as a system service / root neither
// of these is set and the call can only fail. In that context the real
// mechanism is the per-home proxy_env.sh shell profiles, so skipping the
// user-manager calls is correct, not a cleanup failure.
//
#[cfg(target_os = "linux")]
fn user_bus_available() -> bool {
    std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
        || std::env::var_os("XDG_RUNTIME_DIR").is_some()
}

#[cfg(target_os = "linux")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(target_os = "linux")]
fn systemctl(args: &[String]) -> Result<Vec<u8>> {
    let output = std::process::Command::new("systemctl")
        .arg("--user")
        .arg("--no-pager")
        .env("LC_ALL", "C")
        .args(args)
        .output_bounded()
        .context("Failed to run systemctl")?;
    anyhow::ensure!(
        output.status.success(),
        "systemctl failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}

#[cfg(target_os = "linux")]
fn user_manager_unavailable(error: &anyhow::Error) -> bool {
    if error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return true;
    }
    let message = error.to_string();
    message.contains("Failed to connect to")
        && (message.contains("No such file or directory")
            || message.contains("No medium found")
            || message.contains("Connection refused")
            || message.contains("not set"))
}

#[cfg(target_os = "linux")]
fn parse_systemd_environment(output: &[u8]) -> Result<BTreeMap<String, String>> {
    if let Ok(environment) = serde_json::from_slice::<Option<BTreeMap<String, String>>>(output) {
        return Ok(environment.unwrap_or_default());
    }

    //
    // Older systemctl versions ignore JSON output for show-environment.
    // Decode their ANSI-C quoting as data, without evaluating shell code.
    //

    let mut environment = BTreeMap::new();
    for line in std::str::from_utf8(output)?.lines() {
        let (name, value) = line
            .split_once('=')
            .context("Invalid systemd environment entry")?;
        let decoded = if let Some(quoted) = value.strip_prefix("$'") {
            let quoted = quoted
                .strip_suffix('\'')
                .context("Unterminated systemd environment value")?;
            let mut bytes = quoted.bytes();
            let mut decoded = Vec::new();
            while let Some(byte) = bytes.next() {
                if byte != b'\\' {
                    decoded.push(byte);
                    continue;
                }
                let escaped = bytes
                    .next()
                    .context("Incomplete systemd environment escape")?;
                decoded.push(match escaped {
                    b'a' => 7,
                    b'b' => 8,
                    b'f' => 12,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'v' => 11,
                    b'\\' | b'\'' | b'"' => escaped,
                    b'0'..=b'7' => {
                        let second = bytes
                            .next()
                            .filter(|b| (b'0'..=b'7').contains(b))
                            .context("Invalid systemd octal escape")?;
                        let third = bytes
                            .next()
                            .filter(|b| (b'0'..=b'7').contains(b))
                            .context("Invalid systemd octal escape")?;
                        let value = (escaped - b'0') as u16 * 64
                            + (second - b'0') as u16 * 8
                            + (third - b'0') as u16;
                        u8::try_from(value).context("Invalid systemd octal byte")?
                    }
                    _ => anyhow::bail!("Unsupported systemd environment escape"),
                });
            }
            String::from_utf8(decoded)?
        } else {
            value.to_string()
        };
        environment.insert(name.to_string(), decoded);
    }
    Ok(environment)
}

#[cfg(target_os = "linux")]
fn set_systemd_user_env(vars: &[(String, String)]) -> Result<()> {
    if !user_bus_available() {
        return Ok(());
    }
    install_systemd_environment(&backup_path("intercept_systemd_env.json")?, vars, systemctl)
}

#[cfg(target_os = "linux")]
fn install_systemd_environment(
    path: &Path,
    vars: &[(String, String)],
    mut run: impl FnMut(&[String]) -> Result<Vec<u8>>,
) -> Result<()> {
    let output = match run(&["show-environment".into(), "--output=json".into()]) {
        Ok(output) => output,
        Err(error) if user_manager_unavailable(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    let environment = parse_systemd_environment(&output)?;
    let backup: BTreeMap<String, Option<String>> = vars
        .iter()
        .map(|(name, _)| (name.clone(), environment.get(name).cloned()))
        .collect();
    save_backup(path, &backup)?;
    let mut args = vec!["set-environment".into()];
    args.extend(vars.iter().map(|(name, value)| format!("{name}={value}")));
    run(&args)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn restore_systemd_environment(
    path: &Path,
    mut run: impl FnMut(&[String]) -> Result<()>,
) -> Result<()> {
    let Some(backup) = load_backup::<BTreeMap<String, Option<String>>>(path)? else {
        return Ok(());
    };
    let mut set = vec!["set-environment".into()];
    let mut unset = vec!["unset-environment".into()];
    for (name, value) in &backup {
        match value {
            Some(value) => set.push(format!("{name}={value}")),
            None => unset.push(name.clone()),
        }
    }
    if set.len() > 1 {
        run(&set)?;
    }
    if unset.len() > 1 {
        run(&unset)?;
    }
    remember_restored_child_environment(backup);
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn unset_systemd_user_env() -> Result<()> {
    let path = backup_path("intercept_systemd_env.json")?;
    if path.exists() {
        return restore_systemd_environment(&path, |args| systemctl(args).map(|_| ()));
    }
    if !user_bus_available() {
        return Ok(());
    }
    let output = match systemctl(&["show-environment".into(), "--output=json".into()]) {
        Ok(output) => output,
        Err(error) if user_manager_unavailable(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    let environment = parse_systemd_environment(&output)?;
    let names = legacy_environment_names(&environment, &cert_export_path());
    if !names.is_empty() {
        let mut args = vec!["unset-environment".into()];
        args.extend(names);
        systemctl(&args)?;
    }
    Ok(())
}

/// Non-Linux Unix stub: set environment variables
#[cfg(all(unix, not(target_os = "linux")))]
pub fn set_intercept_env_vars(cert_path: &PathBuf, proxy_addr: Option<&str>) -> Result<()> {
    common::log_info!("Environment variable setting not implemented for this platform");
    common::log_info!("Manually set NODE_EXTRA_CA_CERTS={}", cert_path.display());
    common::log_info!("Manually set CODEX_CA_CERTIFICATE={}", cert_path.display());
    publish_child_environment(intercept_environment(cert_path, proxy_addr));
    if let Some(addr) = proxy_addr {
        common::log_info!("Manually set HTTP_PROXY=http://{}", addr);
        common::log_info!("Manually set HTTPS_PROXY=http://{}", addr);
    }
    Ok(())
}

/// Non-Linux Unix stub: remove environment variables
#[cfg(all(unix, not(target_os = "linux")))]
pub fn remove_intercept_env_vars() -> Result<()> {
    clear_child_environment();
    common::log_info!("Environment variable removal not implemented for this platform");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_commands_receive_ca_and_proxy_without_mutating_node_environment() {
        let original = std::env::var_os("CODEX_CA_CERTIFICATE");
        let cert = PathBuf::from("/test/corporate path/praxis.pem");
        publish_child_environment(intercept_environment(&cert, Some("127.0.0.1:18080")));
        let mut command = std::process::Command::new("unused");
        apply_to_command(&mut command);
        let env: BTreeMap<_, _> = command.get_envs().collect();
        assert_eq!(
            env.get(std::ffi::OsStr::new("CODEX_CA_CERTIFICATE")),
            Some(&Some(cert.as_os_str()))
        );
        assert_eq!(
            env.get(std::ffi::OsStr::new("HTTPS_PROXY")),
            Some(&Some(std::ffi::OsStr::new("http://127.0.0.1:18080")))
        );
        assert_eq!(std::env::var_os("CODEX_CA_CERTIFICATE"), original);
        publish_child_environment(Vec::new());
        let mut disabled = std::process::Command::new("unused");
        disabled.env("CODEX_CA_CERTIFICATE", "corporate.pem");
        apply_to_command(&mut disabled);
        assert_eq!(
            disabled
                .get_envs()
                .find(|(key, _)| *key == "CODEX_CA_CERTIFICATE")
                .unwrap()
                .1
                .unwrap(),
            "corporate.pem"
        );
    }

    #[cfg(target_os = "linux")]
    struct TestDirectory(PathBuf);

    #[cfg(target_os = "linux")]
    impl TestDirectory {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("praxis-env-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn shell_configuration_restores_corporate_ca_and_preserves_profile_bytes() {
        let dir = TestDirectory::new();
        let home = dir.0.join("user with 'quotes'");
        std::fs::create_dir(&home).unwrap();
        let profile = home.join(".profile");
        let original = "export CODEX_CA_CERTIFICATE='/corporate/root.pem'\n\n";
        std::fs::write(&profile, original).unwrap();
        let cert = "/new path/ca's.pem";
        let script = format!("export CODEX_CA_CERTIFICATE={}\n", shell_quote(cert));
        set_intercept_env_vars_for_home(&home, &script).unwrap();
        let read_profile = || {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(". \"$1\"; printf '%s' \"$CODEX_CA_CERTIFICATE\"")
                .arg("test")
                .arg(&profile)
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap()
        };
        assert_eq!(read_profile(), cert);
        remove_intercept_env_vars_for_home(&home).unwrap();
        assert_eq!(read_profile(), "/corporate/root.pem");
        assert_eq!(std::fs::read_to_string(&profile).unwrap(), original);
        remove_intercept_env_vars_for_home(&home).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_restores_original_values_and_unsets_only_previously_absent_values() {
        let dir = TestDirectory::new();
        let path = dir.0.join("backup.json");
        let vars = intercept_environment(Path::new("/praxis.pem"), None);
        let mut environment = BTreeMap::from([
            (
                "CODEX_CA_CERTIFICATE".to_string(),
                "/corporate ca/root.pem".to_string(),
            ),
            (
                "HTTPS_PROXY".to_string(),
                "http://corporate:8080".to_string(),
            ),
        ]);
        let original = environment.clone();
        install_systemd_environment(&path, &vars, |args| {
            if args[0] == "show-environment" {
                return Ok(serde_json::to_vec(&environment)?);
            }
            assert!(path.exists());
            for assignment in &args[1..] {
                let (key, value) = assignment.split_once('=').unwrap();
                environment.insert(key.into(), value.into());
            }
            Ok(Vec::new())
        })
        .unwrap();
        assert_eq!(environment["CODEX_CA_CERTIFICATE"], "/praxis.pem");
        restore_systemd_environment(&path, |args| {
            for value in &args[1..] {
                if args[0] == "set-environment" {
                    let (key, value) = value.split_once('=').unwrap();
                    environment.insert(key.into(), value.into());
                } else {
                    environment.remove(value);
                }
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(environment, original);
        assert!(!path.exists());
        restore_systemd_environment(&path, |_| panic!("restoration must be idempotent")).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn partial_systemd_failure_retains_original_snapshot_until_cleanup_succeeds() {
        let dir = TestDirectory::new();
        let path = dir.0.join("backup.json");
        let vars = intercept_environment(Path::new("/praxis.pem"), None);
        assert!(
            install_systemd_environment(&path, &vars, |args| {
                if args[0] == "show-environment" {
                    Ok(br#"{"CODEX_CA_CERTIFICATE":"/corporate.pem"}"#.to_vec())
                } else {
                    anyhow::bail!("simulated failed install")
                }
            })
            .is_err()
        );
        let bytes = std::fs::read(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(restore_systemd_environment(&path, |_| anyhow::bail!("unavailable bus")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(save_backup(&path, &BTreeMap::<String, String>::new()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let mut restored = Vec::new();
        restore_systemd_environment(&path, |args| {
            restored.push(args.to_vec());
            Ok(())
        })
        .unwrap();
        assert!(
            restored
                .iter()
                .any(|args| args.contains(&"CODEX_CA_CERTIFICATE=/corporate.pem".into()))
        );
        assert!(!path.exists());
    }

    #[test]
    fn legacy_cleanup_requires_praxis_marker_and_preserves_other_settings() {
        let cert = Path::new("/praxis.pem");
        let mut environment = BTreeMap::from([
            ("NODE_EXTRA_CA_CERTS".into(), "/corporate.pem".into()),
            ("HTTP_PROXY".into(), "http://127.0.0.1:8080".into()),
            ("HTTPS_PROXY".into(), "http://corporate:8080".into()),
            ("CODEX_CA_CERTIFICATE".into(), "/corporate.pem".into()),
        ]);
        assert!(legacy_environment_names(&environment, cert).is_empty());
        environment.insert("NODE_EXTRA_CA_CERTS".into(), "/praxis.pem".into());
        assert_eq!(
            legacy_environment_names(&environment, cert),
            vec!["NODE_EXTRA_CA_CERTS", "HTTP_PROXY"]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn old_systemctl_output_preserves_escaped_values_without_evaluating_them() {
        let output = br#"CODEX_CA_CERTIFICATE=$'/corporate ca/it\'s\\ca.pem'
HTTPS_PROXY=$'http://example/$(do-not-execute)'
MULTILINE=$'one\ntwo\t\033'
EMPTY=
"#;
        let environment = parse_systemd_environment(output).unwrap();
        assert_eq!(
            environment["CODEX_CA_CERTIFICATE"],
            "/corporate ca/it's\\ca.pem"
        );
        assert_eq!(
            environment["HTTPS_PROXY"],
            "http://example/$(do-not-execute)"
        );
        assert_eq!(environment["MULTILINE"], "one\ntwo\t\x1b");
        assert_eq!(environment["EMPTY"], "");
    }

    #[test]
    fn restart_recovery_replaces_only_inherited_intercept_values_for_children() {
        let original = BTreeMap::from([
            ("CODEX_CA_CERTIFICATE".into(), Some("/corporate.pem".into())),
            ("NODE_EXTRA_CA_CERTS".into(), None),
            ("HTTPS_PROXY".into(), Some("http://corporate:8080".into())),
        ]);
        let mut inherited = BTreeMap::from([
            ("CODEX_CA_CERTIFICATE".into(), "/praxis.pem".into()),
            ("NODE_EXTRA_CA_CERTS".into(), "/praxis.pem".into()),
            ("HTTPS_PROXY".into(), "http://127.0.0.1:18080".into()),
        ]);
        assert_eq!(
            restored_child_environment(original.clone(), &inherited, Path::new("/praxis.pem")),
            original
        );
        inherited.insert("CODEX_CA_CERTIFICATE".into(), "/own.pem".into());
        assert!(
            !restored_child_environment(original, &inherited, Path::new("/praxis.pem"))
                .contains_key("CODEX_CA_CERTIFICATE")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_systemd_manager_is_optional_before_any_mutation_only() {
        let dir = TestDirectory::new();
        let path = dir.0.join("backup.json");
        let vars = intercept_environment(Path::new("/praxis.pem"), None);
        install_systemd_environment(&path, &vars, |_| {
            anyhow::bail!("systemctl failed: Failed to connect to bus: No medium found")
        })
        .unwrap();
        assert!(!path.exists());
        assert!(
            install_systemd_environment(&path, &vars, |args| {
                if args[0] == "show-environment" {
                    Ok(b"{}".to_vec())
                } else {
                    anyhow::bail!("systemctl failed: Failed to connect to bus: No medium found")
                }
            })
            .is_err()
        );
        assert!(path.exists());
        assert!(
            restore_systemd_environment(&path, |_| {
                anyhow::bail!("systemctl failed: Failed to connect to bus: No medium found")
            })
            .is_err()
        );
        assert!(path.exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn registry_backup_retains_expandable_string_type_and_bytes() {
        let raw = winreg::RegValue {
            bytes: "%USERPROFILE%\\corporate.pem\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect(),
            vtype: winreg::enums::REG_EXPAND_SZ,
        };
        let saved = RegistryValue {
            bytes: raw.bytes.clone(),
            kind: raw.vtype.clone() as u32,
        };
        let decoded: RegistryValue =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(decoded.to_raw().unwrap(), raw);
    }
}
