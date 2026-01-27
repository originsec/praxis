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

/// Remove the hosts file entry for the domain
#[allow(dead_code)]
pub fn remove_hosts_entry(domain: &str) -> Result<()> {
    let hosts_path = PathBuf::from(HOSTS_FILE_PATH);

    let file = fs::File::open(&hosts_path).context("Failed to open hosts file for reading")?;
    let reader = BufReader::new(file);

    let mut new_lines: Vec<String> = Vec::new();
    for line in reader.lines() {
        let line = line?;
        //
        // Skip lines with our marker for this domain.
        //
        if !(line.contains(domain) && line.contains(INTERCEPT_MARKER)) {
            new_lines.push(line);
        }
    }

    fs::write(&hosts_path, new_lines.join("\n")).context("Failed to write updated hosts file")?;

    common::log_info!("Removed hosts entry for {}", domain);
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
        common::log_info!("Removed {} praxis intercept entries from hosts file", removed_count);
    }
    Ok(())
}
