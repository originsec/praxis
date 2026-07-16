use anyhow::{Context, Result};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use crate::utils::CommandOutputBounded;

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
    for raw_line in content.lines() {
        //
        // Skip our own entries (any formatting) so leftover residue from a
        // crash-interrupted cleanup doesn't block re-enable; only foreign
        // mappings should trip the override guard.
        //
        if raw_line.contains(INTERCEPT_MARKER) {
            continue;
        }
        let line = raw_line.split('#').next().unwrap_or_default();
        let mut fields = line.split_whitespace();
        let Some(address) = fields.next() else {
            continue;
        };
        if fields.any(|existing| existing.eq_ignore_ascii_case(domain)) {
            anyhow::bail!(
                "Refusing to override existing hosts mapping for {} ({})",
                domain,
                address
            );
        }
    }

    let mut updated = content.trim_end_matches(['\r', '\n']).to_string();
    updated.push('\n');
    updated.push_str(&entry);
    updated.push('\n');
    replace_hosts_file(&hosts_path, &updated)?;

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
        .output_bounded()
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
// Hosts mode binds the proxy to 127.0.0.1:443 directly, so a REDIRECT is not
// required for new enables. enable_hosts_redirect remains a no-op; disable
// still tries to remove any stale Praxis-tagged REDIRECT from older builds.
//

//
// Kept for recovery/cleanup of older installs that added a REDIRECT; new
// enables do not call this.
//
#[allow(dead_code)]
#[cfg(target_os = "linux")]
pub fn enable_hosts_redirect(_proxy_port: u16) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn disable_hosts_redirect(proxy_port: Option<u16>) -> Result<()> {
    use std::process::Command;

    let proxy_port = proxy_port
        .filter(|port| *port != 0)
        .ok_or_else(|| anyhow::anyhow!("Hosts redirect proxy port is unavailable"))?;
    let port = proxy_port.to_string();

    //
    // Remove all Praxis REDIRECT rules for this proxy port. The complete rule
    // is specified so unrelated localhost redirects are never removed.
    //

    let mut removed = 0usize;
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
                "-m",
                "comment",
                "--comment",
                "PRAXIS-INTERCEPT",
                "-j",
                "REDIRECT",
                "--to-ports",
                &port,
            ])
            .output_bounded()
            .context("Failed to run iptables while removing Hosts redirect")?;

        match output {
            o if o.status.success() => {
                removed += 1;
                continue;
            }
            o if hosts_rule_missing(&o.stderr) => break,
            o => anyhow::bail!(
                "Failed to remove Hosts redirect: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ),
        }
    }

    common::log_info!("Removed {} iptables REDIRECT rule(s) for hosts mode", removed);
    Ok(())
}

#[cfg(target_os = "linux")]
fn hosts_rule_missing(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    stderr.contains("bad rule")
        || stderr.contains("matching rule exist")
        || stderr.contains("no chain/target/match")
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
pub fn disable_hosts_redirect(_proxy_port: Option<u16>) -> Result<()> {
    Ok(())
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

    let mut updated = new_lines.join("\n");
    updated.push('\n');
    replace_hosts_file(&hosts_path, &updated)?;

    if removed_count > 0 {
        common::log_info!(
            "Removed {} praxis intercept entries from hosts file",
            removed_count
        );
    }
    Ok(())
}

fn replace_hosts_file(path: &PathBuf, content: &str) -> Result<()> {
    let temp_path = path.with_extension(format!("praxis-{}.tmp", std::process::id()));
    let original_permissions = fs::metadata(path)
        .context("Failed to read hosts file metadata")?
        .permissions();
    let write_result = (|| -> Result<()> {
        fs::write(&temp_path, content).context("Failed to write temporary hosts file")?;
        fs::set_permissions(&temp_path, original_permissions)
            .context("Failed to preserve hosts file permissions")?;
        fs::File::open(&temp_path)
            .and_then(|file| file.sync_all())
            .context("Failed to flush temporary hosts file")?;
        #[cfg(not(target_os = "windows"))]
        fs::rename(&temp_path, path).context("Failed to atomically replace the hosts file")?;
        #[cfg(target_os = "windows")]
        {
            //
            // Atomic replace so a crash mid-swap cannot truncate the live
            // hosts file. MOVEFILE_REPLACE_EXISTING overwrites the destination
            // and MOVEFILE_WRITE_THROUGH flushes before returning.
            //
            use std::os::windows::ffi::OsStrExt;
            use windows::core::PCWSTR;
            use windows::Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            };

            let to_wide = |p: &std::path::Path| -> Vec<u16> {
                p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
            };
            let src = to_wide(&temp_path);
            let dst = to_wide(path);
            unsafe {
                MoveFileExW(
                    PCWSTR(src.as_ptr()),
                    PCWSTR(dst.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            }
            .context("Failed to atomically replace the hosts file")?;
        }
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}
