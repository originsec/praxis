use anyhow::{Context, Result};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

#[cfg(target_os = "windows")]
const HOSTS_FILE_PATH: &str = r"C:\Windows\System32\drivers\etc\hosts";

#[cfg(target_os = "linux")]
const HOSTS_FILE_PATH: &str = "/etc/hosts";

#[cfg(target_os = "macos")]
const HOSTS_FILE_PATH: &str = "/etc/hosts";
const INTERCEPT_MARKER: &str = "# PRAXIS-INTERCEPT";
const LOCALHOST: &str = "127.0.0.1";

/// Add an entry to the Windows hosts file to redirect the domain to localhost
pub fn add_hosts_entry(domain: &str) -> Result<()> {
    let hosts_path = PathBuf::from(HOSTS_FILE_PATH);

    //
    // Read current content.
    //
    let content = fs::read_to_string(&hosts_path).context("Failed to read hosts file")?;

    //
    // Check if entry already exists.
    //
    let entry = format!("{} {} {}", LOCALHOST, domain, INTERCEPT_MARKER);
    if content.contains(&entry) {
        common::log_info!("Hosts entry already exists for {}", domain);
        return Ok(());
    }

    //
    // Append new entry.
    //
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&hosts_path)
        .context("Failed to open hosts file for writing")?;

    writeln!(file, "\n{}", entry).context("Failed to write to hosts file")?;

    common::log_info!("Added hosts entry: {} -> {}", domain, LOCALHOST);
    Ok(())
}

//
// Flush the Windows DNS cache so hosts file changes take effect immediately.
//

#[cfg(target_os = "windows")]
pub fn flush_dns_cache() {
    match crate::utils::silent_command("ipconfig")
        .args(["/flushdns"])
        .output()
    {
        Ok(output) if output.status.success() => {
            common::log_info!("DNS cache flushed successfully");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            common::log_warn!("Failed to flush DNS cache: {}", stderr);
        }
        Err(e) => {
            common::log_warn!("Failed to run ipconfig /flushdns: {}", e);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn flush_dns_cache() {
    //
    // On Linux/macOS, DNS caching behavior varies by system.
    // systemd-resolved, nscd, or no caching at all.
    //
}

//
// On Linux, redirect traffic from 127.0.0.1:443 to the proxy port.
// This is needed because the hosts file redirects domains to 127.0.0.1,
// but the proxy listens on a random port.
//

#[cfg(target_os = "linux")]
pub fn enable_hosts_redirect(proxy_port: u16) -> Result<()> {
    use std::process::Command;

    let port_str = proxy_port.to_string();

    let output = Command::new("iptables")
        .args([
            "-t",
            "nat",
            "-A",
            "OUTPUT",
            "-p",
            "tcp",
            "-d",
            "127.0.0.1",
            "--dport",
            "443",
            "-j",
            "REDIRECT",
            "--to-ports",
            &port_str,
        ])
        .output()
        .context("Failed to run iptables")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("iptables redirect failed: {}", stderr.trim());
    }

    common::log_info!(
        "Added iptables REDIRECT rule: 127.0.0.1:443 -> port {}",
        proxy_port
    );
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn disable_hosts_redirect() {
    use std::process::Command;

    //
    // Remove all REDIRECT rules for 127.0.0.1:443 (may have multiple from retries).
    //

    for _ in 0..5 {
        let output = Command::new("iptables")
            .args([
                "-t",
                "nat",
                "-D",
                "OUTPUT",
                "-p",
                "tcp",
                "-d",
                "127.0.0.1",
                "--dport",
                "443",
                "-j",
                "REDIRECT",
            ])
            .output();

        match output {
            Ok(o) if o.status.success() => continue,
            _ => break,
        }
    }

    common::log_info!("Removed iptables REDIRECT rules for hosts mode");
}

#[cfg(not(target_os = "linux"))]
pub fn enable_hosts_redirect(_proxy_port: u16) -> Result<()> {
    //
    // On non-Linux, hosts mode connects directly to the proxy port.
    // This requires the proxy to listen on port 443 or use platform-specific redirect.
    //
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn disable_hosts_redirect() {
    // No-op on non-Linux.
}

/// Remove ALL praxis intercept entries from the hosts file
pub fn remove_all_hosts_entries() -> Result<()> {
    let hosts_path = PathBuf::from(HOSTS_FILE_PATH);

    let file = fs::File::open(&hosts_path).context("Failed to open hosts file for reading")?;
    let reader = BufReader::new(file);

    let mut new_lines: Vec<String> = Vec::new();
    let mut removed_count = 0;
    for line in reader.lines() {
        let line = line?;
        //
        // Skip any lines with our marker.
        //
        if line.contains(INTERCEPT_MARKER) {
            removed_count += 1;
        } else {
            new_lines.push(line);
        }
    }

    fs::write(&hosts_path, new_lines.join("\n")).context("Failed to write updated hosts file")?;

    if removed_count > 0 {
        common::log_info!(
            "Removed {} praxis intercept entries from hosts file",
            removed_count
        );
    }
    Ok(())
}
