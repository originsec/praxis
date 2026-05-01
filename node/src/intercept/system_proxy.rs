#[allow(unused_imports)]
use anyhow::{Context, Result};
#[allow(unused_imports)]

/// Saved proxy settings for restoration
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SavedProxySettings {
    pub proxy_enable: u32,
    pub proxy_server: Option<String>,
}

/// Enable the system proxy to point to a local address
#[cfg(target_os = "windows")]
pub fn enable_system_proxy(proxy_addr: &str) -> Result<SavedProxySettings> {
    use winreg::RegKey;
    use winreg::enums::*;

    common::log_info!("Enabling system proxy: {}", proxy_addr);

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let internet_settings = hkcu
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            KEY_READ | KEY_WRITE,
        )
        .context("Failed to open Internet Settings registry key")?;

    //
    // Save current settings.
    //
    let saved = SavedProxySettings {
        proxy_enable: internet_settings
            .get_value::<u32, _>("ProxyEnable")
            .unwrap_or(0),
        proxy_server: internet_settings.get_value::<String, _>("ProxyServer").ok(),
    };

    //
    // Set new proxy settings.
    //
    internet_settings
        .set_value("ProxyEnable", &1u32)
        .context("Failed to set ProxyEnable")?;

    internet_settings
        .set_value("ProxyServer", &proxy_addr)
        .context("Failed to set ProxyServer")?;

    //
    // Notify the system of the change.
    //
    notify_proxy_change();

    common::log_info!("System proxy enabled successfully");
    Ok(saved)
}

/// Enable the system proxy on Linux
///
/// On Linux, proxy configuration is primarily handled via environment variables
/// (see env_vars.rs). This function logs the proxy address for reference.
#[cfg(target_os = "linux")]
pub fn enable_system_proxy(proxy_addr: &str) -> Result<SavedProxySettings> {
    common::log_info!("Linux system proxy: environment variables will be configured");
    common::log_info!("Proxy address: {}", proxy_addr);
    Ok(SavedProxySettings {
        proxy_enable: 1,
        proxy_server: Some(proxy_addr.to_string()),
    })
}

/// Enable the system proxy (non-Windows/non-Linux stub)
#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub fn enable_system_proxy(proxy_addr: &str) -> Result<SavedProxySettings> {
    common::log_warn!("System proxy configuration not implemented for this platform");
    common::log_info!("Would set proxy to: {}", proxy_addr);
    Ok(SavedProxySettings {
        proxy_enable: 0,
        proxy_server: None,
    })
}

/// Disable the system proxy and restore previous settings
#[cfg(target_os = "windows")]
pub fn disable_system_proxy(saved: Option<&SavedProxySettings>) -> Result<()> {
    use winreg::RegKey;
    use winreg::enums::*;

    common::log_info!("Disabling system proxy");

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let internet_settings = hkcu
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            KEY_READ | KEY_WRITE,
        )
        .context("Failed to open Internet Settings registry key")?;

    if let Some(saved) = saved {
        //
        // Restore saved settings.
        //
        internet_settings
            .set_value("ProxyEnable", &saved.proxy_enable)
            .context("Failed to restore ProxyEnable")?;

        if let Some(ref server) = saved.proxy_server {
            internet_settings
                .set_value("ProxyServer", server)
                .context("Failed to restore ProxyServer")?;
        } else {
            //
            // Delete the key if there was no previous proxy.
            //
            let _ = internet_settings.delete_value("ProxyServer");
        }
    } else {
        //
        // Just disable proxy.
        //
        internet_settings
            .set_value("ProxyEnable", &0u32)
            .context("Failed to disable proxy")?;
    }

    //
    // Notify the system of the change.
    //
    notify_proxy_change();

    common::log_info!("System proxy disabled successfully");
    Ok(())
}

/// Disable the system proxy on Linux
///
/// On Linux, proxy cleanup is handled via env_vars.rs.
#[cfg(target_os = "linux")]
pub fn disable_system_proxy(_saved: Option<&SavedProxySettings>) -> Result<()> {
    common::log_info!("Linux system proxy disabled");
    Ok(())
}

/// Disable the system proxy (non-Windows/non-Linux stub)
#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub fn disable_system_proxy(_saved: Option<&SavedProxySettings>) -> Result<()> {
    common::log_warn!("System proxy configuration not implemented for this platform");
    Ok(())
}

/// Get the current system proxy settings
#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn get_proxy_settings() -> Result<SavedProxySettings> {
    use winreg::RegKey;
    use winreg::enums::*;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let internet_settings = hkcu
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            KEY_READ,
        )
        .context("Failed to open Internet Settings registry key")?;

    Ok(SavedProxySettings {
        proxy_enable: internet_settings
            .get_value::<u32, _>("ProxyEnable")
            .unwrap_or(0),
        proxy_server: internet_settings.get_value::<String, _>("ProxyServer").ok(),
    })
}

/// Get the current system proxy settings on Linux
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub fn get_proxy_settings() -> Result<SavedProxySettings> {
    //
    // Check if HTTP_PROXY is set in the environment.
    //
    let proxy_server = std::env::var("HTTP_PROXY")
        .or_else(|_| std::env::var("http_proxy"))
        .ok();
    let proxy_enable = if proxy_server.is_some() { 1 } else { 0 };

    Ok(SavedProxySettings {
        proxy_enable,
        proxy_server,
    })
}

/// Get the current system proxy settings (non-Windows/non-Linux stub)
#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
#[allow(dead_code)]
pub fn get_proxy_settings() -> Result<SavedProxySettings> {
    Ok(SavedProxySettings {
        proxy_enable: 0,
        proxy_server: None,
    })
}

/// Notify the system that proxy settings have changed
/// This causes applications to pick up the new settings
#[cfg(target_os = "windows")]
fn notify_proxy_change() {
    //
    // Use InternetSetOption to notify of the change
    // This requires calling the Windows API
    // For simplicity, we'll use PowerShell to do this.
    //
    let _ = crate::utils::silent_command("powershell")
        .args([
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            r#"
            # Refresh proxy settings
            $signature = @"
            [DllImport("wininet.dll", SetLastError = true)]
            public static extern bool InternetSetOption(IntPtr hInternet, int dwOption, IntPtr lpBuffer, int dwBufferLength);
"@
            $type = Add-Type -MemberDefinition $signature -Name WinINet -Namespace Native -PassThru
            $INTERNET_OPTION_SETTINGS_CHANGED = 39
            $INTERNET_OPTION_REFRESH = 37
            $type::InternetSetOption([IntPtr]::Zero, $INTERNET_OPTION_SETTINGS_CHANGED, [IntPtr]::Zero, 0) | Out-Null
            $type::InternetSetOption([IntPtr]::Zero, $INTERNET_OPTION_REFRESH, [IntPtr]::Zero, 0) | Out-Null
            "#,
        ])
        .output();
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
fn notify_proxy_change() {
    //
    // No-op on non-Windows.
    //
}
