use anyhow::Result;
#[cfg(target_os = "windows")]
use anyhow::Context;
use std::net::IpAddr;

/// TUN interface IPv4 address
#[allow(dead_code)]
pub const TUN_IP: &str = "10.255.0.1";
/// TUN interface IPv4 netmask
#[allow(dead_code)]
pub const TUN_NETMASK: &str = "255.255.255.0";
/// TUN interface IPv6 address (ULA - Unique Local Address)
#[allow(dead_code)]
pub const TUN_IP6: &str = "fd00:255:0::1";
/// TUN interface IPv6 prefix length
#[allow(dead_code)]
pub const TUN_IP6_PREFIX: &str = "64";
/// TUN interface name (must match wintun adapter name)
pub const TUN_INTERFACE_NAME: &str = "Praxis VPN";

/// Route manager for Windows
///
/// Uses netsh commands to manage routing table entries.
#[cfg(target_os = "windows")]
pub struct RouteManager {
    /// Interface name for routing
    interface_name: String,
    /// List of routes we've added (for cleanup)
    added_routes: Vec<IpAddr>,
    /// Whether the interface has been configured
    interface_configured: bool,
}

#[cfg(target_os = "windows")]
impl RouteManager {
    /// Create a new route manager for the given interface
    pub fn new(interface_name: &str) -> Self {
        Self {
            interface_name: interface_name.to_string(),
            added_routes: Vec::new(),
            interface_configured: false,
        }
    }

    /// Configure the TUN interface with an IP address
    ///
    /// Runs: netsh interface ipv4 set address name="Praxis VPN" static 10.255.0.1 255.255.255.0
    pub fn configure_interface(&mut self) -> Result<()> {
        common::log_info!("Configuring interface {} with IP {}/{}", self.interface_name, TUN_IP, TUN_NETMASK);

        let output = crate::utils::silent_command("netsh")
            .args([
                "interface", "ipv4", "set", "address",
                &format!("name={}", self.interface_name),
                "static",
                TUN_IP,
                TUN_NETMASK,
            ])
            .output()
            .context("Failed to execute netsh command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            //
            // Sometimes netsh returns non-zero even when it works.
            //
            if !stderr.is_empty() || !stdout.is_empty() {
                common::log_warn!("netsh set address output: stdout={}, stderr={}", stdout.trim(), stderr.trim());
            }
        }

        self.interface_configured = true;
        common::log_info!("Interface {} configured with IP {}", self.interface_name, TUN_IP);
        Ok(())
    }

    /// Add a route for a specific IP through the TUN interface
    ///
    /// Runs: netsh interface ipv4 add route <IP>/32 "Praxis VPN" 10.255.0.1
    pub fn add_route(&mut self, destination_ip: IpAddr) -> Result<()> {
        //
        // Only route IPv4 for now.
        //
        let ip_str = match destination_ip {
            IpAddr::V4(v4) => v4.to_string(),
            IpAddr::V6(_) => {
                common::log_warn!("IPv6 routing not supported, skipping {}", destination_ip);
                return Ok(());
            }
        };

        common::log_debug!("Adding route for {} via {}", ip_str, self.interface_name);

        let output = crate::utils::silent_command("netsh")
            .args([
                "interface", "ipv4", "add", "route",
                &format!("{}/32", ip_str),
                &self.interface_name,
                TUN_IP,
                "metric=1",
            ])
            .output()
            .context(format!("Failed to add route for {}", ip_str))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            //
            // Check if route already exists.
            //
            if stderr.contains("exists") || stdout.contains("exists") {
                common::log_debug!("Route for {} already exists", ip_str);
            } else if !stderr.is_empty() || !stdout.is_empty() {
                common::log_warn!("netsh add route for {} output: stdout={}, stderr={}",
                      ip_str, stdout.trim(), stderr.trim());
            }
        }

        self.added_routes.push(destination_ip);
        common::log_info!("Added route: {} -> {}", ip_str, self.interface_name);
        Ok(())
    }

    /// Remove a specific route
    fn remove_route(&self, destination_ip: &IpAddr) -> Result<()> {
        let ip_str = match destination_ip {
            IpAddr::V4(v4) => v4.to_string(),
            IpAddr::V6(_) => return Ok(()),
        };

        common::log_debug!("Removing route for {}", ip_str);

        let output = crate::utils::silent_command("netsh")
            .args([
                "interface", "ipv4", "delete", "route",
                &format!("{}/32", ip_str),
                &self.interface_name,
            ])
            .output()
            .context(format!("Failed to remove route for {}", ip_str))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            //
            // Ignore "not found" errors during cleanup.
            //
            if !stderr.contains("not found") && !stderr.contains("Element not found") {
                common::log_warn!("Failed to remove route for {}: {}", ip_str, stderr.trim());
            }
        }

        Ok(())
    }

    /// Remove all routes that were added by this manager
    pub fn remove_all_routes(&mut self) -> Result<()> {
        common::log_info!("Removing {} routes", self.added_routes.len());

        let routes_to_remove: Vec<_> = self.added_routes.drain(..).collect();
        for ip in routes_to_remove {
            if let Err(e) = self.remove_route(&ip) {
                common::log_error!("Error removing route for {}: {}", ip, e);
            }
        }

        Ok(())
    }

    /// Get the interface name
    #[allow(dead_code)]
    pub fn interface_name(&self) -> &str {
        &self.interface_name
    }
}

#[cfg(target_os = "windows")]
impl Drop for RouteManager {
    fn drop(&mut self) {
        if !self.added_routes.is_empty() {
            common::log_warn!("RouteManager dropped with {} routes still active, cleaning up",
                  self.added_routes.len());
            let _ = self.remove_all_routes();
        }
    }
}

//
// Linux implementation using ip route commands.
//
#[cfg(target_os = "linux")]
pub struct RouteManager {
    interface_name: String,
    added_routes: Vec<IpAddr>,
    interface_configured: bool,
}

#[cfg(target_os = "linux")]
impl RouteManager {
    pub fn new(interface_name: &str) -> Self {
        Self {
            interface_name: interface_name.to_string(),
            added_routes: Vec::new(),
            interface_configured: false,
        }
    }

    /// Configure the TUN interface with IPv4 and IPv6 addresses.
    ///
    /// Runs: ip addr add 10.255.0.1/24 dev <interface>
    ///       ip -6 addr add fd00:255:0::1/64 dev <interface>
    ///       ip link set <interface> up
    pub fn configure_interface(&mut self) -> Result<()> {
        use anyhow::Context;

        common::log_info!(
            "Configuring interface {} with IPv4 {}/24 and IPv6 {}/{}",
            self.interface_name, TUN_IP, TUN_IP6, TUN_IP6_PREFIX
        );

        //
        // Add IPv4 address to interface.
        //
        let output = crate::utils::silent_command("ip")
            .args([
                "addr", "add",
                &format!("{}/24", TUN_IP),
                "dev", &self.interface_name,
            ])
            .output()
            .context("Failed to execute ip addr add command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            //
            // Ignore "already exists" errors.
            //
            if !stderr.contains("RTNETLINK answers: File exists") {
                common::log_error!("ip addr add (IPv4) failed: {}", stderr.trim());
            }
        }

        //
        // Add IPv6 address to interface.
        //
        let output = crate::utils::silent_command("ip")
            .args([
                "-6", "addr", "add",
                &format!("{}/{}", TUN_IP6, TUN_IP6_PREFIX),
                "dev", &self.interface_name,
            ])
            .output()
            .context("Failed to execute ip -6 addr add command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            //
            // Ignore "already exists" errors.
            //
            if !stderr.contains("RTNETLINK answers: File exists") {
                common::log_debug!("ip addr add (IPv6) warning: {}", stderr.trim());
            }
        }

        //
        // Bring up the interface.
        //
        let output = crate::utils::silent_command("ip")
            .args(["link", "set", &self.interface_name, "up"])
            .output()
            .context("Failed to execute ip link set up command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            common::log_warn!("ip link set up warning: {}", stderr.trim());
        }

        self.interface_configured = true;
        common::log_info!("Interface {} configured with IPv4 {} and IPv6 {}", self.interface_name, TUN_IP, TUN_IP6);
        Ok(())
    }

    /// Add a route for a specific IP through the TUN interface.
    ///
    /// Runs: ip route add <IP>/32 dev <interface> (IPv4)
    ///       ip -6 route add <IP>/128 dev <interface> (IPv6)
    pub fn add_route(&mut self, destination_ip: IpAddr) -> Result<()> {
        use anyhow::Context;

        let (ip_str, prefix, ipv6_flag) = match destination_ip {
            IpAddr::V4(v4) => (v4.to_string(), "32", false),
            IpAddr::V6(v6) => (v6.to_string(), "128", true),
        };

        common::log_debug!("Adding route for {} via {}", ip_str, self.interface_name);

        let mut cmd = crate::utils::silent_command("ip");
        if ipv6_flag {
            cmd.arg("-6");
        }
        let output = cmd
            .args([
                "route", "add",
                &format!("{}/{}", ip_str, prefix),
                "dev", &self.interface_name,
            ])
            .output()
            .context(format!("Failed to add route for {}", ip_str))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            //
            // Ignore "already exists" errors.
            //
            if stderr.contains("RTNETLINK answers: File exists") {
                common::log_debug!("Route for {} already exists", ip_str);
            } else if !stderr.is_empty() {
                common::log_warn!("ip route add for {} warning: {}", ip_str, stderr.trim());
            }
        }

        self.added_routes.push(destination_ip);
        common::log_info!("Added route: {} -> {}", ip_str, self.interface_name);
        Ok(())
    }

    fn remove_route(&self, destination_ip: &IpAddr) -> Result<()> {
        use anyhow::Context;

        let (ip_str, prefix, ipv6_flag) = match destination_ip {
            IpAddr::V4(v4) => (v4.to_string(), "32", false),
            IpAddr::V6(v6) => (v6.to_string(), "128", true),
        };

        common::log_debug!("Removing route for {}", ip_str);

        let mut cmd = crate::utils::silent_command("ip");
        if ipv6_flag {
            cmd.arg("-6");
        }
        let output = cmd
            .args([
                "route", "del",
                &format!("{}/{}", ip_str, prefix),
                "dev", &self.interface_name,
            ])
            .output()
            .context(format!("Failed to remove route for {}", ip_str))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            //
            // Ignore "not found" errors during cleanup.
            //
            if !stderr.contains("No such process") && !stderr.contains("not found") {
                common::log_warn!("Failed to remove route for {}: {}", ip_str, stderr.trim());
            }
        }

        Ok(())
    }

    pub fn remove_all_routes(&mut self) -> Result<()> {

        common::log_info!("Removing {} routes", self.added_routes.len());

        let routes_to_remove: Vec<_> = self.added_routes.drain(..).collect();
        for ip in routes_to_remove {
            if let Err(e) = self.remove_route(&ip) {
                common::log_error!("Error removing route for {}: {}", ip, e);
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn interface_name(&self) -> &str {
        &self.interface_name
    }
}

#[cfg(target_os = "linux")]
impl Drop for RouteManager {
    fn drop(&mut self) {
        if !self.added_routes.is_empty() {
            common::log_warn!(
                "RouteManager dropped with {} routes still active, cleaning up",
                self.added_routes.len()
            );
            let _ = self.remove_all_routes();
        }
    }
}

//
// Non-Windows/non-Linux stub implementation.
//
#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub struct RouteManager {
    #[allow(dead_code)]
    added_routes: Vec<IpAddr>,
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
impl RouteManager {
    pub fn new(_interface_name: &str) -> Self {
        Self {
            added_routes: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn configure_interface(&mut self) -> Result<()> {
        common::log_warn!("Route management is only supported on Windows and Linux");
        Err(anyhow::anyhow!("Route management is only supported on Windows and Linux"))
    }

    #[allow(dead_code)]
    pub fn add_route(&mut self, _destination_ip: IpAddr) -> Result<()> {
        common::log_warn!("Route management is only supported on Windows and Linux");
        Ok(())
    }

    pub fn remove_all_routes(&mut self) -> Result<()> {
        Ok(())
    }

    #[allow(dead_code)]
    pub fn interface_name(&self) -> &str {
        "N/A"
    }
}

impl Default for RouteManager {
    fn default() -> Self {
        Self::new(TUN_INTERFACE_NAME)
    }
}
