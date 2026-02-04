use anyhow::{Context, Result};
use common::InterceptMethod;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

//
// Intercept state persistence for crash recovery.
//
// When intercept is enabled, we store what changes were made to the system.
// On startup, we check for this file and undo any changes that weren't
// properly cleaned up (e.g., if the process crashed).
//

const STATE_FILE_NAME: &str = "intercept_state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptState {
    /// The intercept method that was enabled
    pub method: InterceptMethod,

    /// Whether a certificate was installed in the system store
    pub cert_installed: bool,

    /// Certificate thumbprint (Windows)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_thumbprint: Option<String>,

    /// Certificate path (Linux)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_path: Option<String>,

    /// Linux distribution type for cert cleanup
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux_distro: Option<String>,

    /// Whether firewall rule was added (Windows)
    pub firewall_rule_added: bool,

    /// Whether system proxy was modified (Proxy method)
    pub proxy_modified: bool,

    /// Saved proxy settings for restoration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_proxy_enable: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_proxy_server: Option<String>,

    /// Whether hosts file was modified (Hosts method)
    pub hosts_modified: bool,

    /// Whether TPROXY rules were added (Tproxy method, Linux only)
    #[serde(default)]
    pub tproxy_enabled: bool,

    /// IPs that have TPROXY rules (for cleanup)
    #[serde(default)]
    pub tproxy_ips: Vec<String>,

    /// Proxy port used for TPROXY
    #[serde(default)]
    pub tproxy_port: u16,

    /// Whether environment variables were set
    pub env_vars_set: bool,
}

impl InterceptState {
    pub fn new(method: InterceptMethod) -> Self {
        Self {
            method,
            cert_installed: false,
            cert_thumbprint: None,
            cert_path: None,
            linux_distro: None,
            firewall_rule_added: false,
            proxy_modified: false,
            saved_proxy_enable: None,
            saved_proxy_server: None,
            hosts_modified: false,
            tproxy_enabled: false,
            tproxy_ips: Vec::new(),
            tproxy_port: 0,
            env_vars_set: false,
        }
    }
}

//
// Get the path to the state file.
//

fn get_state_file_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("praxis").join(STATE_FILE_NAME))
}

//
// Save intercept state to disk.
//

pub fn save_state(state: &InterceptState) -> Result<()> {
    let path = get_state_file_path().ok_or_else(|| {
        anyhow::anyhow!("Could not determine data directory")
    })?;

    //
    // Ensure directory exists.
    //

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create data directory")?;
    }

    let json = serde_json::to_string_pretty(state)
        .context("Failed to serialize intercept state")?;

    fs::write(&path, json).context("Failed to write intercept state file")?;

    common::log_info!("Saved intercept state to {}", path.display());
    Ok(())
}

//
// Load intercept state from disk (if exists).
//

pub fn load_state() -> Option<InterceptState> {
    let path = get_state_file_path()?;

    if !path.exists() {
        return None;
    }

    match fs::read_to_string(&path) {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(state) => Some(state),
            Err(e) => {
                common::log_warn!("Failed to parse intercept state file: {}", e);
                None
            }
        },
        Err(e) => {
            common::log_warn!("Failed to read intercept state file: {}", e);
            None
        }
    }
}

//
// Remove the state file.
//

pub fn remove_state() -> Result<()> {
    if let Some(path) = get_state_file_path() {
        if path.exists() {
            fs::remove_file(&path).context("Failed to remove intercept state file")?;
            common::log_info!("Removed intercept state file");
        }
    }
    Ok(())
}

//
// Clean up any leftover intercept state from a previous run.
// This should be called on startup.
//

pub fn cleanup_stale_state() {
    let state = match load_state() {
        Some(s) => s,
        None => return,
    };

    common::log_info!("Found stale intercept state - cleaning up from previous run");

    //
    // Clean up based on what was recorded.
    //

    //
    // 1. Remove hosts file entries.
    //

    if state.hosts_modified {
        common::log_info!("Cleaning up hosts file entries");
        if let Err(e) = super::hosts::remove_all_hosts_entries() {
            common::log_error!("Failed to clean up hosts file: {}", e);
        }
        super::hosts::disable_hosts_redirect();
        super::hosts::flush_dns_cache();
    }

    //
    // 2. Restore system proxy settings (Windows).
    //

    #[cfg(target_os = "windows")]
    if state.proxy_modified {
        common::log_info!("Restoring system proxy settings");
        let saved = super::SavedProxySettings {
            proxy_enable: state.saved_proxy_enable.unwrap_or(0),
            proxy_server: state.saved_proxy_server.clone(),
        };
        if let Err(e) = super::disable_system_proxy(Some(&saved)) {
            common::log_error!("Failed to restore proxy settings: {}", e);
        }
    }

    //
    // 3. Remove firewall rule (Windows).
    //

    #[cfg(windows)]
    if state.firewall_rule_added {
        common::log_info!("Removing firewall rule");
        crate::utils::remove_firewall_rule();
    }

    //
    // 4. Uninstall certificate (Windows).
    //

    #[cfg(target_os = "windows")]
    if state.cert_installed {
        if let Some(ref thumbprint) = state.cert_thumbprint {
            common::log_info!("Uninstalling certificate with thumbprint: {}", thumbprint);
            uninstall_cert_by_thumbprint(thumbprint);
        }
    }

    //
    // 5. Uninstall certificate (Linux).
    //

    #[cfg(target_os = "linux")]
    if state.cert_installed {
        if let Some(ref cert_path) = state.cert_path {
            common::log_info!("Removing certificate at: {}", cert_path);
            uninstall_linux_cert(cert_path, state.linux_distro.as_deref());
        }
    }

    //
    // 6. Clean up TPROXY rules (Linux).
    //

    #[cfg(target_os = "linux")]
    if state.tproxy_enabled {
        common::log_info!("Cleaning up TPROXY rules");
        cleanup_tproxy(&state.tproxy_ips, state.tproxy_port);
    }

    //
    // 7. Remove environment variables.
    //

    if state.env_vars_set {
        common::log_info!("Removing intercept environment variables");
        if let Err(e) = super::env_vars::remove_intercept_env_vars() {
            common::log_error!("Failed to remove environment variables: {}", e);
        }
    }

    //
    // Remove the state file now that cleanup is complete.
    //

    if let Err(e) = remove_state() {
        common::log_error!("Failed to remove state file: {}", e);
    }

    common::log_info!("Stale intercept state cleanup complete");
}

//
// Windows-specific certificate uninstallation by thumbprint.
//

#[cfg(target_os = "windows")]
fn uninstall_cert_by_thumbprint(thumbprint: &str) {
    let ps_script = format!(
        r#"
        $thumbprint = "{thumbprint}"
        foreach ($location in @("CurrentUser", "LocalMachine")) {{
            try {{
                $store = New-Object System.Security.Cryptography.X509Certificates.X509Store("Root", $location)
                $store.Open("ReadWrite")
                $cert = $store.Certificates | Where-Object {{ $_.Thumbprint -eq $thumbprint }}
                if ($cert) {{
                    $store.Remove($cert)
                    Write-Host "Removed from $location store"
                }}
                $store.Close()
            }} catch {{
                Write-Host "Could not access $location store: $_"
            }}
        }}
        "#,
        thumbprint = thumbprint
    );

    let output = crate::utils::silent_command("powershell")
        .args(["-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            common::log_info!("Certificate uninstalled successfully");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            common::log_warn!("Certificate uninstallation may have failed: {}", stderr);
        }
        Err(e) => {
            common::log_error!("Failed to run PowerShell for certificate uninstallation: {}", e);
        }
    }

    //
    // Clean up temp files.
    //

    let temp_dir = std::env::temp_dir().join("praxis_certs");
    let _ = fs::remove_dir_all(&temp_dir);
}

//
// Linux-specific certificate uninstallation.
//

#[cfg(target_os = "linux")]
fn uninstall_linux_cert(cert_path: &str, distro: Option<&str>) {
    let path = std::path::Path::new(cert_path);

    if path.exists() {
        if let Err(e) = fs::remove_file(path) {
            common::log_error!("Failed to remove certificate file: {}", e);
            return;
        }
        common::log_info!("Removed certificate from: {}", cert_path);
    }

    //
    // Run update command to refresh the store.
    //

    let update_cmd: Option<Vec<&str>> = match distro {
        Some("debian") => Some(vec!["update-ca-certificates"]),
        Some("rhel") => Some(vec!["update-ca-trust"]),
        Some("arch") => Some(vec!["trust", "extract-compat"]),
        _ => None,
    };

    if let Some(cmd) = update_cmd {
        match std::process::Command::new(cmd[0]).args(&cmd[1..]).output() {
            Ok(o) if o.status.success() => {
                common::log_info!("System certificate store updated after removal");
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                common::log_warn!("Certificate update command returned non-zero: {}", stderr);
            }
            Err(e) => {
                common::log_warn!("Failed to run certificate update command: {}", e);
            }
        }
    }

    //
    // Clean up temp files.
    //

    let temp_dir = std::env::temp_dir().join("praxis_certs");
    let _ = fs::remove_dir_all(&temp_dir);
}

//
// Linux-specific TPROXY cleanup.
//

#[cfg(target_os = "linux")]
fn cleanup_tproxy(ips: &[String], proxy_port: u16) {
    use std::process::Command;

    //
    // Remove iptables rules for each IP.
    //

    for ip in ips {
        let _ = Command::new("iptables")
            .args([
                "-t",
                "mangle",
                "-D",
                "OUTPUT",
                "-p",
                "tcp",
                "-d",
                ip,
                "--dport",
                "443",
                "-j",
                "MARK",
                "--set-mark",
                "1",
            ])
            .output();

        let _ = Command::new("iptables")
            .args([
                "-t",
                "mangle",
                "-D",
                "PREROUTING",
                "-p",
                "tcp",
                "-d",
                ip,
                "--dport",
                "443",
                "-j",
                "TPROXY",
                "--on-port",
                &proxy_port.to_string(),
                "--tproxy-mark",
                "1",
            ])
            .output();
    }

    //
    // Remove bypass rule.
    //

    let _ = Command::new("iptables")
        .args([
            "-t",
            "mangle",
            "-D",
            "OUTPUT",
            "-m",
            "mark",
            "--mark",
            "2",
            "-j",
            "RETURN",
        ])
        .output();

    //
    // Remove policy routing.
    //

    let _ = Command::new("ip")
        .args(["route", "del", "local", "0.0.0.0/0", "dev", "lo", "table", "100"])
        .output();

    let _ = Command::new("ip")
        .args(["rule", "del", "fwmark", "1", "lookup", "100"])
        .output();

    //
    // Disable route_localnet.
    //

    let _ = Command::new("sysctl")
        .args(["-w", "net.ipv4.conf.lo.route_localnet=0"])
        .output();

    common::log_info!("TPROXY cleanup complete");
}
