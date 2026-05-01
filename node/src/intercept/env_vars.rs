use anyhow::{Context, Result};
use std::path::PathBuf;
#[allow(unused_imports)]

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

/// Set intercept environment variables system-wide
///
/// - NODE_EXTRA_CA_CERTS: Always set to the exported CA cert path
/// - HTTP_PROXY / HTTPS_PROXY: Only set if proxy_addr is provided
#[cfg(target_os = "windows")]
pub fn set_intercept_env_vars(cert_path: &PathBuf, proxy_addr: Option<&str>) -> Result<()> {
    let cert_path_str = cert_path.to_string_lossy();

    //
    // Set NODE_EXTRA_CA_CERTS.
    //
    windows_env::set("NODE_EXTRA_CA_CERTS", &cert_path_str)
        .context("Failed to set NODE_EXTRA_CA_CERTS")?;
    common::log_info!("Set NODE_EXTRA_CA_CERTS={}", cert_path_str);

    //
    // Set proxy env vars if in proxy mode.
    //
    if let Some(addr) = proxy_addr {
        let proxy_url = format!("http://{}", addr);
        windows_env::set("HTTP_PROXY", &proxy_url).context("Failed to set HTTP_PROXY")?;
        windows_env::set("HTTPS_PROXY", &proxy_url).context("Failed to set HTTPS_PROXY")?;
        common::log_info!("Set HTTP_PROXY={}", proxy_url);
        common::log_info!("Set HTTPS_PROXY={}", proxy_url);
    }

    Ok(())
}

/// Remove intercept environment variables
#[cfg(target_os = "windows")]
pub fn remove_intercept_env_vars() -> Result<()> {
    //
    // Remove all intercept-related env vars (ignore errors if they don't
    // exist).
    //
    let _ = windows_env::remove("NODE_EXTRA_CA_CERTS");
    let _ = windows_env::remove("HTTP_PROXY");
    let _ = windows_env::remove("HTTPS_PROXY");

    common::log_info!("Removed intercept environment variables");

    //
    // Also clean up the exported cert file.
    //
    let cert_path = cert_export_path();
    if cert_path.exists() {
        if let Err(e) = std::fs::remove_file(&cert_path) {
            common::log_warn!("Failed to remove exported CA cert: {}", e);
        }
    }

    Ok(())
}

/// Linux: set environment variables via shell profile modification
///
/// When running as root, this will configure env vars for all users with
/// home directories in /home/.
#[cfg(target_os = "linux")]
pub fn set_intercept_env_vars(cert_path: &PathBuf, proxy_addr: Option<&str>) -> Result<()> {
    let cert_path_str = cert_path.to_string_lossy();

    //
    // Build the script content (shared across all users).
    //
    let mut script_content = String::new();
    script_content.push_str("# Praxis intercept environment variables\n");
    script_content.push_str("# This file is auto-generated - do not edit manually\n\n");
    script_content.push_str(&format!(
        "export NODE_EXTRA_CA_CERTS=\"{}\"\n",
        cert_path_str
    ));

    if let Some(addr) = proxy_addr {
        let proxy_url = format!("http://{}", addr);
        script_content.push_str(&format!("export HTTP_PROXY=\"{}\"\n", proxy_url));
        script_content.push_str(&format!("export HTTPS_PROXY=\"{}\"\n", proxy_url));
        script_content.push_str(&format!("export http_proxy=\"{}\"\n", proxy_url));
        script_content.push_str(&format!("export https_proxy=\"{}\"\n", proxy_url));
        script_content.push_str("export NO_PROXY=\"localhost,127.0.0.1\"\n");
        script_content.push_str("export no_proxy=\"localhost,127.0.0.1\"\n");
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
        if let Err(e) = set_intercept_env_vars_for_home(home_dir, &script_content) {
            common::log_warn!(
                "Failed to configure env vars for {}: {}",
                home_dir.display(),
                e
            );
        }
    }

    //
    // Try to set systemd user environment (for user services).
    //
    if let Err(e) = set_systemd_user_env(cert_path_str.as_ref(), proxy_addr) {
        common::log_warn!("Failed to set systemd user environment: {}", e);
    }

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
    let source_line = format!(
        "[ -f \"{}\" ] && source \"{}\"",
        env_script_path.display(),
        env_script_path.display()
    );

    for profile_path in get_shell_profiles_for_home(home_dir) {
        if let Err(e) = add_to_shell_profile(&profile_path, &source_line) {
            common::log_warn!("Failed to update {}: {}", profile_path.display(), e);
        } else {
            common::log_info!("Updated shell profile: {}", profile_path.display());
        }
    }

    Ok(())
}

/// Linux: remove intercept environment variables
///
/// When running as root, this will clean up env vars for all users with
/// home directories in /home/.
#[cfg(target_os = "linux")]
pub fn remove_intercept_env_vars() -> Result<()> {
    //
    // Get all home directories to clean up.
    //
    let home_dirs = get_all_home_dirs();
    common::log_info!(
        "Removing intercept env vars for {} user(s)",
        home_dirs.len()
    );

    for home_dir in &home_dirs {
        if let Err(e) = remove_intercept_env_vars_for_home(home_dir) {
            common::log_warn!(
                "Failed to clean up env vars for {}: {}",
                home_dir.display(),
                e
            );
        }
    }

    //
    // Unset systemd user environment.
    //
    if let Err(e) = unset_systemd_user_env() {
        common::log_warn!("Failed to unset systemd user environment: {}", e);
    }

    //
    // Clean up the exported cert file.
    //
    let cert_path = cert_export_path();
    if cert_path.exists() {
        if let Err(e) = std::fs::remove_file(&cert_path) {
            common::log_warn!("Failed to remove exported CA cert: {}", e);
        }
    }

    common::log_info!("Removed intercept environment variables");
    Ok(())
}

/// Remove intercept env vars for a specific home directory
#[cfg(target_os = "linux")]
fn remove_intercept_env_vars_for_home(home_dir: &PathBuf) -> Result<()> {
    //
    // Remove source lines from shell profiles.
    //
    for profile_path in get_shell_profiles_for_home(home_dir) {
        if let Err(e) = remove_from_shell_profile(&profile_path) {
            common::log_warn!("Failed to clean up {}: {}", profile_path.display(), e);
        } else {
            common::log_info!("Cleaned up shell profile: {}", profile_path.display());
        }
    }

    //
    // Remove the proxy environment script.
    //
    let config_dir = home_dir.join(".config").join("praxis");
    let env_script_path = config_dir.join("proxy_env.sh");
    if env_script_path.exists() {
        if let Err(e) = std::fs::remove_file(&env_script_path) {
            common::log_warn!("Failed to remove proxy environment script: {}", e);
        } else {
            common::log_info!(
                "Removed proxy environment script: {}",
                env_script_path.display()
            );
        }
    }

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

    //
    // Check if our markers are present.
    //
    if !content.contains(PRAXIS_MARKER_START) {
        return Ok(());
    }

    //
    // Remove lines between markers (inclusive).
    //
    let mut new_lines = Vec::new();
    let mut in_praxis_block = false;

    for line in content.lines() {
        if line.contains(PRAXIS_MARKER_START) {
            in_praxis_block = true;
            continue;
        }
        if line.contains(PRAXIS_MARKER_END) {
            in_praxis_block = false;
            continue;
        }
        if !in_praxis_block {
            new_lines.push(line);
        }
    }

    //
    // Remove trailing empty lines that were added before our block.
    //
    while new_lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        new_lines.pop();
    }

    let new_content = new_lines.join("\n");
    std::fs::write(profile_path, new_content).context("Failed to write updated shell profile")?;

    Ok(())
}

/// Set systemd user environment variables
#[cfg(target_os = "linux")]
fn set_systemd_user_env(cert_path: &str, proxy_addr: Option<&str>) -> Result<()> {
    let mut args = vec![
        "--user".to_string(),
        "set-environment".to_string(),
        format!("NODE_EXTRA_CA_CERTS={}", cert_path),
    ];

    if let Some(addr) = proxy_addr {
        let proxy_url = format!("http://{}", addr);
        args.push(format!("HTTP_PROXY={}", proxy_url));
        args.push(format!("HTTPS_PROXY={}", proxy_url));
        args.push(format!("http_proxy={}", proxy_url));
        args.push(format!("https_proxy={}", proxy_url));
        args.push("NO_PROXY=localhost,127.0.0.1".to_string());
        args.push("no_proxy=localhost,127.0.0.1".to_string());
    }

    let output = std::process::Command::new("systemctl").args(&args).output();

    match output {
        Ok(o) if o.status.success() => {
            common::log_info!("Set systemd user environment variables");
            Ok(())
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            Err(anyhow::anyhow!("systemctl failed: {}", stderr))
        }
        Err(e) => Err(anyhow::anyhow!("Failed to run systemctl: {}", e)),
    }
}

/// Unset systemd user environment variables
#[cfg(target_os = "linux")]
fn unset_systemd_user_env() -> Result<()> {
    let args = [
        "--user",
        "unset-environment",
        "NODE_EXTRA_CA_CERTS",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "http_proxy",
        "https_proxy",
        "NO_PROXY",
        "no_proxy",
    ];

    let output = std::process::Command::new("systemctl").args(&args).output();

    match output {
        Ok(o) if o.status.success() => {
            common::log_info!("Unset systemd user environment variables");
            Ok(())
        }
        Ok(_) => Ok(()),  // Ignore errors - vars might not have been set
        Err(_) => Ok(()), // Ignore if systemctl not available
    }
}

/// Non-Linux Unix stub: set environment variables
#[cfg(all(unix, not(target_os = "linux")))]
pub fn set_intercept_env_vars(cert_path: &PathBuf, proxy_addr: Option<&str>) -> Result<()> {
    common::log_info!("Environment variable setting not implemented for this platform");
    common::log_info!("Manually set NODE_EXTRA_CA_CERTS={}", cert_path.display());
    if let Some(addr) = proxy_addr {
        common::log_info!("Manually set HTTP_PROXY=http://{}", addr);
        common::log_info!("Manually set HTTPS_PROXY=http://{}", addr);
    }
    Ok(())
}

/// Non-Linux Unix stub: remove environment variables
#[cfg(all(unix, not(target_os = "linux")))]
pub fn remove_intercept_env_vars() -> Result<()> {
    common::log_info!("Environment variable removal not implemented for this platform");
    Ok(())
}
