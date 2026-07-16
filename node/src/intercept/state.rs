use anyhow::{Context, Result};
use common::InterceptMethod;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use crate::utils::CommandOutputBounded;

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

    /// Exact Windows firewall rule name owned by this intercept session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firewall_rule_name: Option<String>,

    /// Local port scoped into the Windows firewall rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firewall_rule_port: Option<u16>,

    /// Whether system proxy was modified (Proxy method)
    pub proxy_modified: bool,

    /// Saved proxy settings for restoration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_proxy_enable: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_proxy_server: Option<String>,

    /// Routes installed for VPN interception.
    #[serde(default)]
    pub vpn_routes: Vec<String>,

    /// Interface used by the recorded VPN routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vpn_interface: Option<String>,

    /// Whether Linux policy routing was installed for the VPN proxy bypass.
    #[serde(default)]
    pub vpn_bypass_enabled: bool,

    /// Whether hosts file was modified (Hosts method)
    pub hosts_modified: bool,

    /// Proxy port used by the Linux Hosts REDIRECT rule (legacy).
    #[serde(default)]
    pub hosts_proxy_port: u16,

    /// Whether a Linux Hosts iptables REDIRECT was installed.
    /// `None` = pre-flag legacy state (may need best-effort redirect cleanup).
    /// `Some(false)` = new enables that never created a redirect.
    /// `Some(true)` = redirect was installed by this version.
    #[serde(default)]
    pub hosts_redirect_added: Option<bool>,

    /// Whether TPROXY rules were added (Tproxy method, Linux only)
    #[serde(default)]
    pub tproxy_enabled: bool,

    /// IPs that have TPROXY rules (for cleanup)
    #[serde(default)]
    pub tproxy_ips: Vec<String>,

    /// Proxy port used for TPROXY
    #[serde(default)]
    pub tproxy_port: u16,

    /// Whether recorded TPROXY iptables rules include the Praxis marker.
    #[serde(default)]
    pub tproxy_rules_tagged: bool,

    /// Whether environment variables were set
    pub env_vars_set: bool,

    /// Original Linux IPv6 disable flag, when VPN/TPROXY changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv6_original_value: Option<String>,

    /// Original Linux loopback route_localnet flag changed by TPROXY.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_localnet_original_value: Option<String>,
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
            firewall_rule_name: None,
            firewall_rule_port: None,
            proxy_modified: false,
            saved_proxy_enable: None,
            saved_proxy_server: None,
            vpn_routes: Vec::new(),
            vpn_interface: None,
            vpn_bypass_enabled: false,
            hosts_modified: false,
            hosts_proxy_port: 0,
            hosts_redirect_added: Some(false),
            tproxy_enabled: false,
            tproxy_ips: Vec::new(),
            tproxy_port: 0,
            tproxy_rules_tagged: false,
            env_vars_set: false,
            ipv6_original_value: None,
            route_localnet_original_value: None,
        }
    }
}

//
// Get the path to the state file.
//

fn get_state_file_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("praxis").join(STATE_FILE_NAME))
}

fn state_temp_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

fn state_backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

//
// Save intercept state to disk.
//

pub fn save_state(state: &InterceptState) -> Result<()> {
    let path = get_state_file_path()
        .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;

    //
    // Ensure directory exists.
    //

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create data directory")?;
    }

    let json =
        serde_json::to_string_pretty(state).context("Failed to serialize intercept state")?;

    let temp_path = state_temp_path(&path);
    let mut temp = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp_path)
        .context("Failed to create temporary intercept state file")?;
    temp.write_all(json.as_bytes())
        .context("Failed to write temporary intercept state file")?;
    temp.sync_all()
        .context("Failed to flush temporary intercept state file")?;
    drop(temp);

    #[cfg(not(target_os = "windows"))]
    fs::rename(&temp_path, &path).context("Failed to atomically replace intercept state file")?;

    #[cfg(target_os = "windows")]
    {
        let backup_path = state_backup_path(&path);
        if backup_path.exists() {
            fs::remove_file(&backup_path)
                .context("Failed to remove stale intercept state backup")?;
        }
        let had_existing = path.exists();
        if had_existing {
            fs::rename(&path, &backup_path)
                .context("Failed to preserve the previous intercept state")?;
        }
        if let Err(error) = fs::rename(&temp_path, &path) {
            if had_existing {
                let _ = fs::rename(&backup_path, &path);
            }
            return Err(error).context("Failed to install the new intercept state");
        }
        if had_existing {
            fs::remove_file(&backup_path)
                .context("Failed to remove the previous intercept state backup")?;
        }
    }

    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .context("Failed to flush intercept state directory")?;
    }

    common::log_info!("Saved intercept state to {}", path.display());
    Ok(())
}

pub fn update_state(update: impl FnOnce(&mut InterceptState)) -> Result<()> {
    let mut state = load_state()?
        .ok_or_else(|| anyhow::anyhow!("Intercept recovery state is not initialized"))?;
    update(&mut state);
    save_state(&state)
}

//
// Load intercept state from disk (if exists).
//

pub fn load_state() -> Result<Option<InterceptState>> {
    let Some(path) = get_state_file_path() else {
        return Ok(None);
    };

    let backup_path = state_backup_path(&path);
    let read_path = if path.exists() {
        path
    } else if backup_path.exists() {
        common::log_warn!("Recovering intercept state from the previous atomic-write backup");
        backup_path
    } else {
        return Ok(None);
    };

    let json = fs::read_to_string(&read_path).context("Failed to read intercept recovery state")?;
    let state = serde_json::from_str(&json)
        .context("Failed to parse intercept recovery state; refusing unsafe startup")?;
    Ok(Some(state))
}

//
// Remove the state file.
//

pub fn remove_state() -> Result<()> {
    if let Some(path) = get_state_file_path() {
        let mut failures = Vec::new();
        let temp_path = state_temp_path(&path);
        let backup_path = state_backup_path(&path);
        for candidate in [&path, &temp_path, &backup_path] {
            if candidate.exists()
                && let Err(error) = fs::remove_file(candidate)
            {
                failures.push(format!("{}: {}", candidate.display(), error));
            }
        }
        if !failures.is_empty() {
            anyhow::bail!("Failed to remove intercept state files: {}", failures.join("; "));
        }
        common::log_info!("Removed intercept state file");
    }
    Ok(())
}

//
// Whether Hosts-mode cleanup should attempt the Linux iptables REDIRECT
// teardown. New enables set `hosts_redirect_added = Some(false)` and skip.
// Legacy recovery files without the field (`None`) still best-effort clean
// when hosts_modified is true, in case an older build installed the rule.
//

pub fn should_cleanup_hosts_redirect(state: &InterceptState) -> bool {
    match state.hosts_redirect_added {
        Some(false) => false,
        Some(true) => true,
        None => state.hosts_modified,
    }
}

//
// Clean up any leftover intercept state from a previous run.
// This should be called on startup.
//

pub fn cleanup_stale_state() -> Result<()> {
    let state = match load_state()? {
        Some(s) => s,
        None => return Ok(()),
    };

    common::log_info!("Found stale intercept state - cleaning up from previous run");
    let mut failures = Vec::new();

    //
    // Clean up based on what was recorded.
    //

    //
    // 1. Remove hosts file entries.
    //

    if state.hosts_modified {
        common::log_info!("Cleaning up hosts file entries");
        if let Err(e) = super::hosts::remove_all_hosts_entries() {
            failures.push(format!("hosts entries: {}", e));
        }
        if should_cleanup_hosts_redirect(&state) {
            if let Err(e) = super::hosts::disable_hosts_redirect(Some(state.hosts_proxy_port)) {
                failures.push(format!("hosts redirect: {}", e));
            }
        }
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
            failures.push(format!("system proxy: {}", e));
        }
    }

    //
    // Remove only the VPN routes and Linux policy-routing bypass explicitly
    // recorded as Praxis-owned resources.
    //
    if state.method == InterceptMethod::Vpn && !state.vpn_routes.is_empty() {
        let mut routes = Vec::new();
        for value in &state.vpn_routes {
            match value.parse() {
                Ok(ip) => routes.push(ip),
                Err(error) => failures.push(format!(
                    "VPN route recovery address '{}': {}",
                    value, error
                )),
            }
        }
        if let Some(ref interface) = state.vpn_interface {
            if !routes.is_empty()
                && let Err(e) = super::routing::RouteManager::cleanup_stale(interface, routes)
            {
                failures.push(format!("VPN routes: {}", e));
            }
        } else {
            failures.push("VPN routes: recovery state has no interface".to_string());
        }
    }

    #[cfg(target_os = "linux")]
    if state.vpn_bypass_enabled {
        if let Err(e) = super::routing::VpnBypassManager::cleanup_stale() {
            failures.push(format!("VPN bypass: {}", e));
        }
    }

    //
    // 3. Remove firewall rule (Windows).
    //

    #[cfg(windows)]
    if state.firewall_rule_added {
        common::log_info!("Removing firewall rule");
        let removed = if let Some(ref name) = state.firewall_rule_name {
            crate::utils::remove_firewall_rule_named(name)
        } else {
            //
            // Legacy state without a stored name: try the old generic rule.
            //
            crate::utils::remove_legacy_firewall_rule()
        };
        if !removed {
            failures.push("Windows firewall rule removal failed".to_string());
        }
    }

    //
    // 4. Uninstall certificate (Windows).
    //

    #[cfg(target_os = "windows")]
    if state.cert_installed {
        if let Some(ref thumbprint) = state.cert_thumbprint {
            common::log_info!("Uninstalling certificate with thumbprint: {}", thumbprint);
            if let Err(e) = uninstall_cert_by_thumbprint(thumbprint) {
                failures.push(format!("root CA: {}", e));
            }
        }
    }

    //
    // 5. Uninstall certificate (Linux).
    //

    #[cfg(target_os = "linux")]
    if state.cert_installed {
        if let Some(ref cert_path) = state.cert_path {
            common::log_info!("Removing certificate at: {}", cert_path);
            if let Err(e) = uninstall_linux_cert(cert_path, state.linux_distro.as_deref()) {
                failures.push(format!("root CA: {}", e));
            }
        }
    }

    //
    // 6. Clean up TPROXY rules (Linux).
    //

    #[cfg(target_os = "linux")]
    if state.tproxy_enabled {
        common::log_info!("Cleaning up TPROXY rules");
        let mut ips = Vec::new();
        let mut addresses_valid = true;
        for value in &state.tproxy_ips {
            match value.parse() {
                Ok(ip) => ips.push(ip),
                Err(error) => {
                    addresses_valid = false;
                    failures.push(format!(
                        "TPROXY recovery address '{}': {}",
                        value, error
                    ));
                }
            }
        }
        if state.tproxy_port == 0 {
            failures.push("TPROXY recovery state has no proxy port".to_string());
        } else if ips.is_empty() {
            failures.push("TPROXY recovery state has no target addresses".to_string());
        } else if addresses_valid
            && let Err(e) = super::TproxyManager::cleanup_stale(
                state.tproxy_port,
                ips,
                state.route_localnet_original_value.clone(),
                state.tproxy_rules_tagged,
            )
        {
            failures.push(format!("TPROXY: {}", e));
        }
    }

    //
    // 7. Remove environment variables.
    //

    if state.env_vars_set {
        common::log_info!("Removing intercept environment variables");
        if let Err(e) = super::env_vars::remove_intercept_env_vars() {
            failures.push(format!("environment: {}", e));
        }
    }

    #[cfg(target_os = "linux")]
    if let Some(ref original) = state.ipv6_original_value {
        if let Err(e) = super::routing::Ipv6Manager::restore_stale(original) {
            failures.push(format!("IPv6: {}", e));
        }
    }

    //
    // Remove the state file now that cleanup is complete.
    //

    if !failures.is_empty() {
        anyhow::bail!(
            "Stale intercept cleanup incomplete; recovery state retained: {}",
            failures.join("; ")
        );
    }

    remove_state()?;

    common::log_info!("Stale intercept state cleanup complete");
    Ok(())
}

//
// Windows-specific certificate uninstallation by thumbprint.
//

#[cfg(target_os = "windows")]
fn uninstall_cert_by_thumbprint(thumbprint: &str) -> Result<()> {
    let ps_script = format!(
        r#"
        $thumbprint = "{thumbprint}"
        $failed = $false
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
                Write-Error "Could not access $location store: $_"
                $failed = $true
            }}
        }}
        if ($failed) {{ exit 1 }}
        "#,
        thumbprint = thumbprint
    );

    let output = crate::utils::silent_command("powershell")
        .args(["-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .output_bounded();

    match output {
        Ok(o) if o.status.success() => {
            common::log_info!("Certificate uninstalled successfully");
        }
        Ok(o) => anyhow::bail!(
            "Certificate uninstallation failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => return Err(e).context("Failed to run certificate uninstallation"),
    }

    //
    // Clean up temp files.
    //

    let temp_dir = std::env::temp_dir().join("praxis_certs");
    let _ = fs::remove_dir_all(&temp_dir);
    Ok(())
}

//
// Linux-specific certificate uninstallation.
//

#[cfg(target_os = "linux")]
fn uninstall_linux_cert(cert_path: &str, distro: Option<&str>) -> Result<()> {
    let path = std::path::Path::new(cert_path);

    if path.exists() {
        if let Err(e) = fs::remove_file(path) {
            return Err(e).context("Failed to remove certificate file");
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
        match std::process::Command::new(cmd[0]).args(&cmd[1..]).output_bounded() {
            Ok(o) if o.status.success() => {
                common::log_info!("System certificate store updated after removal");
            }
            Ok(o) => anyhow::bail!(
                "Certificate update command returned non-zero: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ),
            Err(e) => return Err(e).context("Failed to run certificate update command"),
        }
    }

    //
    // Clean up temp files.
    //

    let temp_dir = std::env::temp_dir().join("praxis_certs");
    let _ = fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[cfg(test)]
mod hosts_redirect_ownership_tests {
    use super::{should_cleanup_hosts_redirect, InterceptState};
    use common::InterceptMethod;

    #[test]
    fn new_enables_skip_redirect_cleanup() {
        let state = InterceptState::new(InterceptMethod::Hosts);
        assert_eq!(state.hosts_redirect_added, Some(false));
        assert!(!should_cleanup_hosts_redirect(&state));
    }

    #[test]
    fn explicit_redirect_ownership_runs_cleanup() {
        let mut state = InterceptState::new(InterceptMethod::Hosts);
        state.hosts_modified = true;
        state.hosts_redirect_added = Some(true);
        assert!(should_cleanup_hosts_redirect(&state));
    }

    #[test]
    fn legacy_missing_flag_cleans_when_hosts_modified() {
        let mut state = InterceptState::new(InterceptMethod::Hosts);
        state.hosts_modified = true;
        state.hosts_redirect_added = None;
        assert!(should_cleanup_hosts_redirect(&state));

        state.hosts_modified = false;
        assert!(!should_cleanup_hosts_redirect(&state));
    }

    #[test]
    fn serde_default_missing_flag_is_none() {
        let json = r#"{"method":"Hosts","cert_installed":false,"firewall_rule_added":false,"proxy_modified":false,"hosts_modified":true,"env_vars_set":false}"#;
        let state: InterceptState = serde_json::from_str(json).unwrap();
        assert_eq!(state.hosts_redirect_added, None);
        assert!(should_cleanup_hosts_redirect(&state));
    }
}
