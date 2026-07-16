use anyhow::{Context, Result};
use std::net::IpAddr;
use crate::utils::CommandOutputBounded;

#[allow(dead_code)]
/// Mark for proxy's outgoing connections to bypass VPN/TUN routing.
/// Same as TPROXY_BYPASS_MARK - both serve the same purpose.
pub const VPN_BYPASS_MARK: u32 = 0x2;

#[allow(dead_code)]
/// Routing table number for VPN bypass routes.
const VPN_BYPASS_TABLE: u32 = 200;

/// TUN interface IPv4 address
pub const TUN_IP: &str = "10.255.0.1";
#[allow(dead_code)]
/// TUN interface IPv4 netmask
pub const TUN_NETMASK: &str = "255.255.255.0";
/// TUN interface IPv6 address (ULA - Unique Local Address)
pub const TUN_IP6: &str = "fd00:255:0::1";
#[allow(dead_code)]
/// TUN interface IPv6 prefix length
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

    pub fn ensure_routes_available(
        routes: &std::collections::HashSet<IpAddr>,
    ) -> Result<()> {
        for route in routes {
            let IpAddr::V4(ip) = route else {
                continue;
            };
            let prefix = format!("{}/32", ip);
            let script = format!(
                "$route = Get-NetRoute -DestinationPrefix '{}' -ErrorAction SilentlyContinue; if ($route) {{ exit 2 }}",
                prefix
            );
            let output = crate::utils::silent_command("powershell")
                .args(["-NoProfile", "-Command", &script])
                .output_bounded()
                .context(format!("Failed to inspect route {}", prefix))?;
            match output.status.code() {
                Some(0) => {}
                Some(2) => anyhow::bail!(
                    "Refusing to replace existing route {}; remove the conflict before enabling interception",
                    prefix
                ),
                _ => anyhow::bail!(
                    "Failed to inspect route {}: {}",
                    prefix,
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            }
        }
        Ok(())
    }

    /// Configure the TUN interface with an IP address
    ///
    /// Runs: netsh interface ipv4 set address name="Praxis VPN" static 10.255.0.1 255.255.255.0
    pub fn configure_interface(&mut self) -> Result<()> {
        common::log_info!(
            "Configuring interface {} with IP {}/{}",
            self.interface_name,
            TUN_IP,
            TUN_NETMASK
        );

        let output = crate::utils::silent_command("netsh")
            .args([
                "interface",
                "ipv4",
                "set",
                "address",
                &format!("name={}", self.interface_name),
                "static",
                TUN_IP,
                TUN_NETMASK,
            ])
            .output_bounded()
            .context("Failed to execute netsh command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!(
                "netsh failed to configure interface: stdout={}, stderr={}",
                stdout.trim(),
                stderr.trim()
            );
        }

        self.interface_configured = true;
        common::log_info!(
            "Interface {} configured with IP {}",
            self.interface_name,
            TUN_IP
        );
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
                "interface",
                "ipv4",
                "add",
                "route",
                &format!("{}/32", ip_str),
                &self.interface_name,
                TUN_IP,
                "metric=1",
            ])
            .output_bounded()
            .context(format!("Failed to add route for {}", ip_str))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!(
                "netsh failed to add route for {}: stdout={}, stderr={}",
                ip_str,
                stdout.trim(),
                stderr.trim()
            );
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
                "interface",
                "ipv4",
                "delete",
                "route",
                &format!("{}/32", ip_str),
                &self.interface_name,
            ])
            .output_bounded()
            .context(format!("Failed to remove route for {}", ip_str))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            //
            // Ignore "not found" errors during cleanup.
            //
            if !stderr.contains("not found") && !stderr.contains("Element not found") {
                anyhow::bail!("Failed to remove route for {}: {}", ip_str, stderr.trim());
            }
        }

        Ok(())
    }

    /// Remove all routes that were added by this manager
    pub fn remove_all_routes(&mut self) -> Result<()> {
        common::log_info!("Removing {} routes", self.added_routes.len());

        let routes_to_remove = std::mem::take(&mut self.added_routes);
        let mut failures = Vec::new();
        for ip in routes_to_remove {
            if let Err(e) = self.remove_route(&ip) {
                failures.push(format!("{}: {}", ip, e));
                self.added_routes.push(ip);
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }

    pub fn added_routes(&self) -> &[IpAddr] {
        &self.added_routes
    }

    pub fn cleanup_stale(interface_name: &str, routes: Vec<IpAddr>) -> Result<()> {
        let mut manager = Self::new(interface_name);
        manager.added_routes = routes;
        manager.remove_all_routes()
    }

    #[allow(dead_code)]
    /// Get the interface name
    pub fn interface_name(&self) -> &str {
        &self.interface_name
    }
}

#[cfg(target_os = "windows")]
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

    pub fn ensure_routes_available(
        routes: &std::collections::HashSet<IpAddr>,
    ) -> Result<()> {
        for route in routes {
            let IpAddr::V4(ip) = route else {
                continue;
            };
            let prefix = format!("{}/32", ip);
            let output = crate::utils::silent_command("ip")
                .args(["-4", "route", "show", "exact", &prefix])
                .output_bounded()
                .context(format!("Failed to inspect route {}", prefix))?;
            if !output.status.success() {
                anyhow::bail!(
                    "Failed to inspect route {}: {}",
                    prefix,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            if !output.stdout.is_empty() {
                anyhow::bail!(
                    "Refusing to replace existing route {}; remove the conflict before enabling interception",
                    prefix
                );
            }
        }
        Ok(())
    }

    /// Configure the IPv4-only TUN interface.
    ///
    /// Runs: ip addr add 10.255.0.1/24 dev <interface>
    ///       ip link set <interface> up
    pub fn configure_interface(&mut self) -> Result<()> {
        use anyhow::Context;

        common::log_info!(
            "Configuring interface {} with IPv4 {}/24",
            self.interface_name,
            TUN_IP
        );

        //
        // Add IPv4 address to interface.
        //
        let output = crate::utils::silent_command("ip")
            .args([
                "addr",
                "add",
                &format!("{}/24", TUN_IP),
                "dev",
                &self.interface_name,
            ])
            .output_bounded()
            .context("Failed to execute ip addr add command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ip addr add (IPv4) failed: {}", stderr.trim());
        }

        //
        // Bring up the interface.
        //
        let output = crate::utils::silent_command("ip")
            .args(["link", "set", &self.interface_name, "up"])
            .output_bounded()
            .context("Failed to execute ip link set up command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ip link set up failed: {}", stderr.trim());
        }

        //
        // Configure sysctl settings for proper packet handling.
        //
        // rp_filter=0: Disable reverse path filtering so packets with our
        // virtual source IP (10.255.0.100) aren't dropped.
        //
        // accept_local=1: Allow the kernel to accept packets with source
        // addresses that belong to local interfaces (needed for our NAT).
        //
        // route_localnet=1: Allow routing of 127.x.x.x and local addresses
        // through this interface (needed for our NAT to work).
        //

        let sysctl_settings = [
            format!("net.ipv4.conf.{}.rp_filter=0", self.interface_name),
            format!("net.ipv4.conf.{}.accept_local=1", self.interface_name),
            format!("net.ipv4.conf.{}.route_localnet=1", self.interface_name),
        ];

        for setting in &sysctl_settings {
            let output = crate::utils::silent_command("sysctl")
                .args(["-w", setting])
                .output_bounded();

            match output {
                Ok(o) if o.status.success() => {
                    common::log_debug!("Set {}", setting);
                }
                Ok(o) => anyhow::bail!(
                    "Failed to set {}: {}",
                    setting,
                    String::from_utf8_lossy(&o.stderr).trim()
                ),
                Err(e) => anyhow::bail!("Failed to run sysctl for {}: {}", setting, e),
            }
        }

        self.interface_configured = true;
        common::log_info!(
            "Interface {} configured with IPv4 {}",
            self.interface_name,
            TUN_IP
        );
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
                "route",
                "add",
                &format!("{}/{}", ip_str, prefix),
                "dev",
                &self.interface_name,
            ])
            .output_bounded()
            .context(format!("Failed to add route for {}", ip_str))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ip route add for {} failed: {}", ip_str, stderr.trim());
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
                "route",
                "del",
                &format!("{}/{}", ip_str, prefix),
                "dev",
                &self.interface_name,
            ])
            .output_bounded()
            .context(format!("Failed to remove route for {}", ip_str))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            //
            // Ignore "not found" errors during cleanup.
            //
            if !stderr.contains("No such process")
                && !stderr.contains("not found")
                && !stderr.contains("Cannot find device")
            {
                anyhow::bail!("Failed to remove route for {}: {}", ip_str, stderr.trim());
            }
        }

        Ok(())
    }

    pub fn remove_all_routes(&mut self) -> Result<()> {
        common::log_info!("Removing {} routes", self.added_routes.len());

        let routes_to_remove = std::mem::take(&mut self.added_routes);
        let mut failures = Vec::new();
        for ip in routes_to_remove {
            if let Err(e) = self.remove_route(&ip) {
                failures.push(format!("{}: {}", ip, e));
                self.added_routes.push(ip);
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }

    pub fn added_routes(&self) -> &[IpAddr] {
        &self.added_routes
    }

    pub fn cleanup_stale(interface_name: &str, routes: Vec<IpAddr>) -> Result<()> {
        let mut manager = Self::new(interface_name);
        manager.added_routes = routes;
        manager.remove_all_routes()
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
    added_routes: Vec<IpAddr>,
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
impl RouteManager {
    pub fn new(_interface_name: &str) -> Self {
        Self {
            added_routes: Vec::new(),
        }
    }

    pub fn configure_interface(&mut self) -> Result<()> {
        common::log_warn!("Route management is only supported on Windows and Linux");
        Err(anyhow::anyhow!(
            "Route management is only supported on Windows and Linux"
        ))
    }

    pub fn add_route(&mut self, _destination_ip: IpAddr) -> Result<()> {
        common::log_warn!("Route management is only supported on Windows and Linux");
        Ok(())
    }

    pub fn remove_all_routes(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn added_routes(&self) -> &[IpAddr] {
        &self.added_routes
    }

    pub fn cleanup_stale(_interface_name: &str, _routes: Vec<IpAddr>) -> Result<()> {
        Ok(())
    }

    pub fn interface_name(&self) -> &str {
        "N/A"
    }
}

impl Default for RouteManager {
    fn default() -> Self {
        Self::new(TUN_INTERFACE_NAME)
    }
}

//
// VPN Bypass Manager (Linux only).
//
// Sets up policy routing so that proxy's outgoing connections (marked with
// VPN_BYPASS_MARK) bypass the TUN routes and use the real default gateway.
//

#[cfg(target_os = "linux")]
pub struct VpnBypassManager {
    /// Whether bypass routing is active.
    is_active: bool,
    /// The default gateway we're using for bypass.
    default_gateway: Option<String>,
    /// The interface for the default gateway.
    default_interface: Option<String>,
    rule_added: bool,
    route_added: bool,
}

#[cfg(target_os = "linux")]
impl VpnBypassManager {
    pub fn new() -> Self {
        Self {
            is_active: false,
            default_gateway: None,
            default_interface: None,
            rule_added: false,
            route_added: false,
        }
    }

    pub fn ensure_resources_available() -> Result<()> {
        use anyhow::Context;

        let rules = crate::utils::silent_command("ip")
            .args(["rule", "show"])
            .output_bounded()
            .context("Failed to inspect policy routing rules")?;
        if !rules.status.success() {
            anyhow::bail!(
                "Failed to inspect policy routing rules: {}",
                String::from_utf8_lossy(&rules.stderr).trim()
            );
        }
        let rules = String::from_utf8_lossy(&rules.stdout).to_ascii_lowercase();
        if rules.lines().any(|line| {
            line.contains("fwmark 0x2")
                && line.contains("lookup 200")
                && line.trim_start().starts_with("100:")
        }) {
            anyhow::bail!("policy rule priority 100 for mark 0x2/table 200 already exists");
        }

        let routes = crate::utils::silent_command("ip")
            .args(["route", "show", "table", &VPN_BYPASS_TABLE.to_string()])
            .output_bounded()
            .context("Failed to inspect VPN bypass routing table")?;
        if routes.status.success() && !routes.stdout.iter().all(u8::is_ascii_whitespace) {
            anyhow::bail!("routing table 200 is not empty");
        }
        if !routes.status.success() && !cleanup_command_missing(&routes.stderr) {
            anyhow::bail!(
                "Failed to inspect VPN bypass routing table: {}",
                String::from_utf8_lossy(&routes.stderr).trim()
            );
        }
        Ok(())
    }

    /// Start VPN bypass routing.
    ///
    /// Sets up:
    /// 1. Discover the default gateway before TUN routes are added
    /// 2. Add policy routing rule: packets with VPN_BYPASS_MARK use table VPN_BYPASS_TABLE
    /// 3. Add default route via real gateway in VPN_BYPASS_TABLE
    pub fn start(&mut self) -> Result<()> {
        use anyhow::Context;

        if self.is_active || self.rule_added || self.route_added {
            return Ok(());
        }

        common::log_info!("Setting up VPN bypass routing");

        //
        // 1. Discover the default gateway and interface.
        //
        let (gateway, interface) = self
            .discover_default_gateway()
            .context("Failed to discover default gateway")?;

        common::log_info!("Default gateway: {} via {}", gateway, interface);

        self.default_gateway = Some(gateway.clone());
        self.default_interface = Some(interface.clone());

        //
        // 2. Add policy routing rule: packets with mark use our bypass table.
        //
        let output = crate::utils::silent_command("ip")
            .args([
                "rule",
                "add",
                "fwmark",
                &VPN_BYPASS_MARK.to_string(),
                "lookup",
                &VPN_BYPASS_TABLE.to_string(),
                "priority",
                "100",
            ])
            .output_bounded()
            .context("Failed to add ip rule")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to add VPN bypass rule: {}", stderr.trim());
        }
        self.rule_added = true;

        //
        // 3. Add default route via real gateway in our bypass table.
        //
        let output = crate::utils::silent_command("ip")
            .args([
                "route",
                "add",
                "default",
                "via",
                &gateway,
                "dev",
                &interface,
                "table",
                &VPN_BYPASS_TABLE.to_string(),
            ])
            .output_bounded()
            .context("Failed to add bypass route");

        let output = match output {
            Ok(output) => output,
            Err(e) => {
                let cleanup = self.stop();
                return match cleanup {
                    Ok(()) => Err(e),
                    Err(cleanup_error) => Err(anyhow::anyhow!(
                        "{}; VPN bypass rollback also failed: {}",
                        e,
                        cleanup_error
                    )),
                };
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let cause = anyhow::anyhow!("Failed to add VPN bypass route: {}", stderr.trim());
            let cleanup = self.stop();
            return match cleanup {
                Ok(()) => Err(cause),
                Err(cleanup_error) => Err(anyhow::anyhow!(
                    "{}; VPN bypass rollback also failed: {}",
                    cause,
                    cleanup_error
                )),
            };
        }
        self.route_added = true;

        self.is_active = true;
        common::log_info!(
            "VPN bypass routing enabled (mark={}, table={})",
            VPN_BYPASS_MARK,
            VPN_BYPASS_TABLE
        );

        Ok(())
    }

    /// Stop VPN bypass routing and clean up rules.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_active && !self.rule_added && !self.route_added {
            return Ok(());
        }

        common::log_info!("Removing VPN bypass routing");

        //
        // Remove the bypass route from our table.
        //
        let mut failures = Vec::new();
        if self.route_added {
            let output = crate::utils::silent_command("ip")
                .args([
                    "route",
                    "del",
                    "default",
                    "table",
                    &VPN_BYPASS_TABLE.to_string(),
                ])
                .output_bounded();
            match output {
                Ok(output) if output.status.success() => self.route_added = false,
                Ok(output) if cleanup_command_missing(&output.stderr) => self.route_added = false,
                Ok(output) => failures.push(format!("bypass route: {}", String::from_utf8_lossy(&output.stderr).trim())),
                Err(e) => failures.push(format!("bypass route: {}", e)),
            }
        }

        //
        // Remove the policy routing rule.
        //
        if self.rule_added {
            let output = crate::utils::silent_command("ip")
                .args([
                    "rule",
                    "del",
                    "fwmark",
                    &VPN_BYPASS_MARK.to_string(),
                    "lookup",
                    &VPN_BYPASS_TABLE.to_string(),
                    "priority",
                    "100",
                ])
                .output_bounded();
            match output {
                Ok(output) if output.status.success() => self.rule_added = false,
                Ok(output) if cleanup_command_missing(&output.stderr) => self.rule_added = false,
                Ok(output) => failures.push(format!(
                    "bypass rule: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )),
                Err(e) => failures.push(format!("bypass rule: {}", e)),
            }
        }

        if failures.is_empty() {
            self.is_active = false;
            self.default_gateway = None;
            self.default_interface = None;

            common::log_info!("VPN bypass routing disabled");
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }

    /// Discover the default gateway and interface.
    ///
    /// Parses output of `ip route show default` to find the gateway.
    fn discover_default_gateway(&self) -> Result<(String, String)> {
        use anyhow::Context;

        let output = crate::utils::silent_command("ip")
            .args(["route", "show", "default"])
            .output_bounded()
            .context("Failed to run ip route show default")?;

        if !output.status.success() {
            anyhow::bail!("ip route show default failed");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        //
        // Parse output like: "default via 192.168.1.1 dev eth0 proto dhcp metric 100"
        //
        for line in stdout.lines() {
            if line.starts_with("default") {
                let parts: Vec<&str> = line.split_whitespace().collect();

                let mut gateway = None;
                let mut interface = None;

                let mut i = 0;
                while i < parts.len() {
                    if parts[i] == "via" && i + 1 < parts.len() {
                        gateway = Some(parts[i + 1].to_string());
                    }
                    if parts[i] == "dev" && i + 1 < parts.len() {
                        interface = Some(parts[i + 1].to_string());
                    }
                    i += 1;
                }

                if let (Some(gw), Some(iface)) = (gateway, interface) {
                    return Ok((gw, iface));
                }
            }
        }

        anyhow::bail!("Could not find default gateway in routing table")
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// True when this manager still owns host policy-routing resources that
    /// must be torn down via stop() — not discarded Drop with ignored errors.
    pub fn owns_host_resources(&self) -> bool {
        self.is_active || self.rule_added || self.route_added
    }

    pub fn cleanup_stale() -> Result<()> {
        let mut manager = Self::new();
        manager.is_active = true;
        manager.rule_added = true;
        manager.route_added = true;
        manager.stop()
    }
}

#[cfg(target_os = "linux")]
fn cleanup_command_missing(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    stderr.contains("no such process")
        || stderr.contains("not found")
        || stderr.contains("no such file")
        || stderr.contains("fib table does not exist")
}

#[cfg(target_os = "linux")]
impl Default for VpnBypassManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
impl Drop for VpnBypassManager {
    fn drop(&mut self) {
        if self.owns_host_resources() {
            common::log_warn!("VpnBypassManager dropped while still active, cleaning up");
            let _ = self.stop();
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod vpn_bypass_ownership_tests {
    use super::VpnBypassManager;

    #[test]
    fn owns_host_resources_when_partial_flags_set() {
        let mut manager = VpnBypassManager::new();
        assert!(!manager.owns_host_resources());

        manager.rule_added = true;
        assert!(manager.owns_host_resources());

        manager.rule_added = false;
        manager.route_added = true;
        assert!(manager.owns_host_resources());

        manager.route_added = false;
        manager.is_active = true;
        assert!(manager.owns_host_resources());
    }
}

//
// Non-Linux stub for VpnBypassManager.
//

#[cfg(not(target_os = "linux"))]
pub struct VpnBypassManager;

#[cfg(not(target_os = "linux"))]
impl VpnBypassManager {
    pub fn new() -> Self {
        Self
    }

    #[allow(dead_code)]
    pub fn start(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        false
    }

    #[allow(dead_code)]
    pub fn owns_host_resources(&self) -> bool {
        false
    }

    pub fn cleanup_stale() -> Result<()> {
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
impl Default for VpnBypassManager {
    fn default() -> Self {
        Self::new()
    }
}

//
// IPv6 Manager (Linux only).
//
// Disables IPv6 when VPN or TPROXY modes are active to avoid routing issues,
// then restores the original setting on cleanup.
//

#[cfg(target_os = "linux")]
pub struct Ipv6Manager {
    /// Whether we disabled IPv6 (need to restore).
    is_disabled: bool,
    /// Original value of net.ipv6.conf.all.disable_ipv6.
    original_value: Option<String>,
}

#[cfg(target_os = "linux")]
impl Ipv6Manager {
    pub fn new() -> Self {
        Self {
            is_disabled: false,
            original_value: None,
        }
    }

    /// Disable IPv6 system-wide.
    ///
    /// Saves the original value of `net.ipv6.conf.all.disable_ipv6` so it can
    /// be restored later.
    pub fn disable(&mut self) -> Result<()> {
        use anyhow::Context;

        if self.is_disabled {
            return Ok(());
        }

        //
        // Read current value to save for restoration.
        //

        let value = Self::current_value()?;
        common::log_info!("Original net.ipv6.conf.all.disable_ipv6 = {}", value);
        self.original_value = Some(value);

        //
        // Disable IPv6.
        //

        let output = crate::utils::silent_command("sysctl")
            .args(["-w", "net.ipv6.conf.all.disable_ipv6=1"])
            .output_bounded()
            .context("Failed to disable IPv6")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to disable IPv6: {}", stderr.trim());
        }

        self.is_disabled = true;
        common::log_info!("IPv6 disabled (net.ipv6.conf.all.disable_ipv6=1)");

        Ok(())
    }

    pub fn current_value() -> Result<String> {
        use anyhow::Context;

        let output = crate::utils::silent_command("sysctl")
            .args(["-n", "net.ipv6.conf.all.disable_ipv6"])
            .output_bounded()
            .context("Failed to read IPv6 disable status")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to read IPv6 disable status: {}", stderr.trim());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn original_value(&self) -> Option<&str> {
        self.original_value.as_deref()
    }

    pub fn restore_stale(original_value: &str) -> Result<()> {
        use anyhow::Context;

        let output = crate::utils::silent_command("sysctl")
            .args([
                "-w",
                &format!(
                    "net.ipv6.conf.all.disable_ipv6={}",
                    original_value
                ),
            ])
            .output_bounded()
            .context("Failed to restore stale IPv6 setting")?;
        if !output.status.success() {
            anyhow::bail!(
                "Failed to restore stale IPv6 setting: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// Restore IPv6 to its original state.
    pub fn restore(&mut self) -> Result<()> {
        use anyhow::Context;

        if !self.is_disabled {
            return Ok(());
        }

        let value = self.original_value.as_deref().unwrap_or("0");
        common::log_info!("Restoring net.ipv6.conf.all.disable_ipv6 to {}", value);

        let output = crate::utils::silent_command("sysctl")
            .args(["-w", &format!("net.ipv6.conf.all.disable_ipv6={}", value)])
            .output_bounded()
            .context("Failed to restore IPv6")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to restore IPv6: {}", stderr.trim());
        }

        self.is_disabled = false;
        self.original_value = None;

        common::log_info!("IPv6 restored");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Default for Ipv6Manager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
impl Drop for Ipv6Manager {
    fn drop(&mut self) {
        if self.is_disabled {
            common::log_warn!("Ipv6Manager dropped while IPv6 still disabled, restoring");
            let _ = self.restore();
        }
    }
}

//
// Non-Linux stub for Ipv6Manager.
//

#[cfg(not(target_os = "linux"))]
pub struct Ipv6Manager;

#[cfg(not(target_os = "linux"))]
impl Ipv6Manager {
    pub fn new() -> Self {
        Self
    }

    #[allow(dead_code)]
    pub fn disable(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn restore(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
impl Default for Ipv6Manager {
    fn default() -> Self {
        Self::new()
    }
}
