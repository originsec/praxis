//
// TPROXY-based transparent proxy support for Linux.
//
// Uses iptables TPROXY + policy routing to intercept traffic without NAT.
// This avoids the kernel limitation where loopback-to-loopback traffic
// bypasses TC eBPF hooks.
//

use anyhow::{Context, Result};
use crate::utils::CommandOutputBounded;
use std::net::Ipv4Addr;
use std::process::Command;
use tokio_util::sync::CancellationToken;

const TPROXY_MARK: u32 = 0x1;
const TPROXY_TABLE: u32 = 100;
/// Mark for proxy's outgoing connections to bypass TPROXY.
pub const TPROXY_BYPASS_MARK: u32 = 0x2;

/// TPROXY manager for iptables-based interception.
pub struct TproxyManager {
    /// Proxy port for TPROXY redirect.
    proxy_port: u16,
    /// IPs currently being intercepted.
    intercept_ips: Vec<Ipv4Addr>,
    /// Whether TPROXY is active.
    is_active: bool,
    route_localnet_original: Option<String>,
    route_localnet_changed: bool,
    policy_rule_added: bool,
    policy_route_added: bool,
    bypass_rule_added: bool,
    target_rules_started: bool,
    rules_tagged: bool,
}

impl TproxyManager {
    pub fn new() -> Self {
        Self {
            proxy_port: 0,
            intercept_ips: Vec::new(),
            is_active: false,
            route_localnet_original: None,
            route_localnet_changed: false,
            policy_rule_added: false,
            policy_route_added: false,
            bypass_rule_added: false,
            target_rules_started: false,
            rules_tagged: true,
        }
    }

    pub fn ensure_resources_available() -> Result<()> {
        let rules = Command::new("ip")
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
            line.contains("fwmark 0x1")
                && (line.contains("lookup 100") || line.contains("lookup tproxy"))
        }) {
            anyhow::bail!("policy rule for mark 0x1/table 100 already exists");
        }

        let routes = Command::new("ip")
            .args(["route", "show", "table", &TPROXY_TABLE.to_string()])
            .output_bounded()
            .context("Failed to inspect TPROXY routing table")?;
        if routes.status.success() && !routes.stdout.iter().all(u8::is_ascii_whitespace) {
            anyhow::bail!("routing table 100 is not empty");
        }
        if !routes.status.success() && !cleanup_error_missing(&routes.stderr) {
            anyhow::bail!(
                "Failed to inspect TPROXY routing table: {}",
                String::from_utf8_lossy(&routes.stderr).trim()
            );
        }

        let iptables = Command::new("iptables")
            .args(["-t", "mangle", "-S", "OUTPUT"])
            .output_bounded()
            .context("Failed to inspect existing TPROXY rules")?;
        if !iptables.status.success() {
            anyhow::bail!(
                "Failed to inspect existing TPROXY rules: {}",
                String::from_utf8_lossy(&iptables.stderr).trim()
            );
        }
        if String::from_utf8_lossy(&iptables.stdout).contains("PRAXIS-INTERCEPT") {
            anyhow::bail!("Praxis-tagged TPROXY rules already exist");
        }
        Ok(())
    }

    pub fn current_route_localnet() -> Result<String> {
        let output = Command::new("sysctl")
            .args(["-n", "net.ipv4.conf.lo.route_localnet"])
            .output_bounded()
            .context("Failed to read route_localnet")?;
        if !output.status.success() {
            anyhow::bail!(
                "Failed to read route_localnet: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Start TPROXY interception.
    ///
    /// Sets up:
    /// 1. Policy routing table and rule for marked packets
    /// 2. route_localnet sysctl to allow local routing of external IPs
    /// 3. iptables TPROXY rules in mangle table
    ///
    /// When `cancel` is set, checked between host-mutating steps (including
    /// per-IP rule install) so Reset/Ctrl+C can abort without waiting for
    /// every address's command timeout.
    pub fn start(
        &mut self,
        proxy_port: u16,
        intercept_ips: &[Ipv4Addr],
        cancel: Option<&CancellationToken>,
    ) -> Result<()> {
        if self.has_resources() {
            common::log_info!("TPROXY already active");
            return Ok(());
        }

        common::log_info!("Starting TPROXY interception on port {}", proxy_port);
        self.proxy_port = proxy_port;
        self.intercept_ips = intercept_ips.to_vec();

        if let Err(cause) = self.start_inner(cancel) {
            return match self.stop() {
                Ok(()) => Err(cause),
                Err(cleanup_error) => Err(anyhow::anyhow!(
                    "{}; TPROXY rollback also failed: {}",
                    cause,
                    cleanup_error
                )),
            };
        }

        self.is_active = true;
        common::log_info!(
            "TPROXY interception started for {} IPs",
            self.intercept_ips.len()
        );

        Ok(())
    }

    fn check_start_cancelled(cancel: Option<&CancellationToken>) -> Result<()> {
        if cancel.is_some_and(|t| t.is_cancelled()) {
            anyhow::bail!("TPROXY setup cancelled");
        }
        Ok(())
    }

    fn start_inner(&mut self, cancel: Option<&CancellationToken>) -> Result<()> {
        Self::check_start_cancelled(cancel)?;

        //
        // 1. Enable route_localnet to allow routing external IPs through loopback.
        //

        self.route_localnet_original = Some(Self::current_route_localnet()?);

        run_command("sysctl", &["-w", "net.ipv4.conf.lo.route_localnet=1"])
            .context("Failed to enable route_localnet")?;
        self.route_localnet_changed = true;
        Self::check_start_cancelled(cancel)?;

        //
        // 2. Add policy routing table for TPROXY marked packets.
        //

        run_command(
            "ip",
            &[
                "rule",
                "add",
                "fwmark",
                &TPROXY_MARK.to_string(),
                "lookup",
                &TPROXY_TABLE.to_string(),
            ],
        )
        .context("Failed to add ip rule")?;
        self.policy_rule_added = true;
        Self::check_start_cancelled(cancel)?;

        run_command(
            "ip",
            &[
                "route",
                "add",
                "local",
                "0.0.0.0/0",
                "dev",
                "lo",
                "table",
                &TPROXY_TABLE.to_string(),
            ],
        )
        .context("Failed to add ip route")?;
        self.policy_route_added = true;
        Self::check_start_cancelled(cancel)?;

        //
        // 3. Add bypass rule so proxy's outgoing connections aren't intercepted.
        //    Packets with TPROXY_BYPASS_MARK skip the interception rules.
        //

        run_command(
            "iptables",
            &[
                "-t",
                "mangle",
                "-A",
                "OUTPUT",
                "-m",
                "mark",
                "--mark",
                &TPROXY_BYPASS_MARK.to_string(),
                "-m",
                "comment",
                "--comment",
                "PRAXIS-INTERCEPT",
                "-j",
                "RETURN",
            ],
        )
        .context("Failed to add bypass rule")?;
        self.bypass_rule_added = true;
        Self::check_start_cancelled(cancel)?;

        //
        // 4. Add iptables TPROXY rules for each intercept IP.
        //

        self.target_rules_started = true;
        for ip in self.intercept_ips.clone() {
            Self::check_start_cancelled(cancel)?;
            self.add_tproxy_rule(ip)?;
        }

        Ok(())
    }

    /// Stop TPROXY interception and clean up rules.
    pub fn stop(&mut self) -> Result<()> {
        if !self.has_resources() {
            return Ok(());
        }

        common::log_info!("Stopping TPROXY interception");

        //
        // Remove iptables TPROXY rules.
        //

        let mut failures = Vec::new();
        if self.target_rules_started {
            for ip in &self.intercept_ips.clone() {
                if let Err(e) = self.remove_tproxy_rule(*ip) {
                    failures.push(format!("target {}: {}", ip, e));
                }
            }
            if failures.is_empty() {
                self.target_rules_started = false;
            }
        }

        //
        // Remove bypass rule.
        //

        if self.bypass_rule_added {
            let result = if self.rules_tagged {
                run_cleanup_command("iptables", &[
                    "-t", "mangle", "-D", "OUTPUT", "-m", "mark", "--mark",
                    &TPROXY_BYPASS_MARK.to_string(), "-m", "comment", "--comment",
                    "PRAXIS-INTERCEPT", "-j", "RETURN",
                ])
            } else {
                run_cleanup_command("iptables", &[
                    "-t", "mangle", "-D", "OUTPUT", "-m", "mark", "--mark",
                    &TPROXY_BYPASS_MARK.to_string(), "-j", "RETURN",
                ])
            };
            match result {
                Ok(()) => self.bypass_rule_added = false,
                Err(error) => failures.push(format!("bypass rule: {}", error)),
            }
        }

        //
        // Remove policy routing.
        //

        if self.policy_route_added {
            match run_cleanup_command("ip", &[
                "route",
                "del",
                "local",
                "0.0.0.0/0",
                "dev",
                "lo",
                "table",
                &TPROXY_TABLE.to_string(),
            ]) {
                Ok(()) => self.policy_route_added = false,
                Err(e) => failures.push(format!("policy route: {}", e)),
            }
        }

        if self.policy_rule_added {
            match run_cleanup_command("ip", &[
                "rule",
                "del",
                "fwmark",
                &TPROXY_MARK.to_string(),
                "lookup",
                &TPROXY_TABLE.to_string(),
            ]) {
                Ok(()) => self.policy_rule_added = false,
                Err(e) => failures.push(format!("policy rule: {}", e)),
            }
        }

        //
        // Disable route_localnet (restore default).
        //

        if self.route_localnet_changed {
            let original = self.route_localnet_original.as_deref().unwrap_or("0");
            match run_command(
                "sysctl",
                &["-w", &format!("net.ipv4.conf.lo.route_localnet={}", original)],
            ) {
                Ok(()) => {
                    self.route_localnet_changed = false;
                    self.route_localnet_original = None;
                }
                Err(e) => failures.push(format!("route_localnet restore: {}", e)),
            }
        }

        if !failures.is_empty() {
            anyhow::bail!(failures.join("; "));
        }

        self.intercept_ips.clear();
        self.is_active = false;
        common::log_info!("TPROXY interception stopped");
        Ok(())
    }

    fn has_resources(&self) -> bool {
        self.is_active
            || self.route_localnet_changed
            || self.policy_rule_added
            || self.policy_route_added
            || self.bypass_rule_added
            || self.target_rules_started
    }

    pub fn route_localnet_original(&self) -> Option<&str> {
        self.route_localnet_original.as_deref()
    }

    pub fn cleanup_stale(
        proxy_port: u16,
        intercept_ips: Vec<Ipv4Addr>,
        route_localnet_original: Option<String>,
        rules_tagged: bool,
    ) -> Result<()> {
        let mut manager = Self::new();
        manager.proxy_port = proxy_port;
        manager.intercept_ips = intercept_ips;
        manager.route_localnet_original = route_localnet_original;
        manager.route_localnet_changed = manager.route_localnet_original.is_some();
        manager.policy_rule_added = true;
        manager.policy_route_added = true;
        manager.bypass_rule_added = true;
        manager.target_rules_started = true;
        manager.rules_tagged = rules_tagged;
        manager.stop()
    }

    /// Add a TPROXY rule for a specific IP.
    fn add_tproxy_rule(&self, ip: Ipv4Addr) -> Result<()> {
        let ip_str = ip.to_string();
        let port_str = self.proxy_port.to_string();
        let mark_str = format!("{}", TPROXY_MARK);

        //
        // Mark outbound packets to this IP in OUTPUT chain.
        //

        run_command(
            "iptables",
            &[
                "-t",
                "mangle",
                "-A",
                "OUTPUT",
                "-p",
                "tcp",
                "-d",
                &ip_str,
                "--dport",
                "443",
                "-m",
                "comment",
                "--comment",
                "PRAXIS-INTERCEPT",
                "-j",
                "MARK",
                "--set-mark",
                &mark_str,
            ],
        )
        .context("Failed to add OUTPUT mark rule")?;

        //
        // TPROXY rule in PREROUTING to redirect marked packets.
        // Note: We need to use PREROUTING because OUTPUT doesn't support TPROXY.
        // The policy routing sends marked packets back through PREROUTING.
        //

        run_command(
            "iptables",
            &[
                "-t",
                "mangle",
                "-A",
                "PREROUTING",
                "-p",
                "tcp",
                "-d",
                &ip_str,
                "--dport",
                "443",
                "-m",
                "comment",
                "--comment",
                "PRAXIS-INTERCEPT",
                "-j",
                "TPROXY",
                "--on-port",
                &port_str,
                "--tproxy-mark",
                &mark_str,
            ],
        )
        .context("Failed to add TPROXY rule")?;

        common::log_debug!("Added TPROXY rule for {}", ip);
        Ok(())
    }

    /// Remove a TPROXY rule for a specific IP.
    fn remove_tproxy_rule(&self, ip: Ipv4Addr) -> Result<()> {
        let ip_str = ip.to_string();
        let port_str = self.proxy_port.to_string();
        let mark_str = format!("{}", TPROXY_MARK);

        let mut failures = Vec::new();
        let output_cleanup = if self.rules_tagged {
            run_cleanup_command(
                "iptables",
                &[
                    "-t", "mangle", "-D", "OUTPUT", "-p", "tcp", "-d", &ip_str,
                    "--dport", "443", "-m", "comment", "--comment", "PRAXIS-INTERCEPT",
                    "-j", "MARK", "--set-mark", &mark_str,
                ],
            )
        } else {
            run_cleanup_command(
                "iptables",
                &[
                    "-t", "mangle", "-D", "OUTPUT", "-p", "tcp", "-d", &ip_str,
                    "--dport", "443", "-j", "MARK", "--set-mark", &mark_str,
                ],
            )
        };
        if let Err(e) = output_cleanup {
            failures.push(format!("OUTPUT mark: {}", e));
        }

        let prerouting_cleanup = if self.rules_tagged {
            run_cleanup_command(
                "iptables",
                &[
                    "-t", "mangle", "-D", "PREROUTING", "-p", "tcp", "-d", &ip_str,
                    "--dport", "443", "-m", "comment", "--comment", "PRAXIS-INTERCEPT",
                    "-j", "TPROXY", "--on-port", &port_str, "--tproxy-mark", &mark_str,
                ],
            )
        } else {
            run_cleanup_command(
                "iptables",
                &[
                    "-t", "mangle", "-D", "PREROUTING", "-p", "tcp", "-d", &ip_str,
                    "--dport", "443", "-j", "TPROXY", "--on-port", &port_str,
                    "--tproxy-mark", &mark_str,
                ],
            )
        };
        if let Err(e) = prerouting_cleanup {
            failures.push(format!("PREROUTING redirect: {}", e));
        }

        if failures.is_empty() {
            common::log_debug!("Removed TPROXY rule for {}", ip);
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }

    #[allow(dead_code)]
    /// Update the list of intercept IPs.
    pub fn update_intercept_ips(&mut self, ips: &[Ipv4Addr]) -> Result<()> {
        if !self.is_active {
            self.intercept_ips = ips.to_vec();
            return Ok(());
        }

        //
        // Remove rules for IPs no longer in the list.
        //

        let old_ips: std::collections::HashSet<_> = self.intercept_ips.iter().cloned().collect();
        let new_ips: std::collections::HashSet<_> = ips.iter().cloned().collect();

        for ip in old_ips.difference(&new_ips) {
            if let Err(e) = self.remove_tproxy_rule(*ip) {
                common::log_warn!("Failed to remove TPROXY rule for {}: {}", ip, e);
            }
        }

        //
        // Add rules for new IPs.
        //

        for ip in new_ips.difference(&old_ips) {
            self.add_tproxy_rule(*ip)?;
        }

        self.intercept_ips = ips.to_vec();
        Ok(())
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }
}

impl Default for TproxyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TproxyManager {
    fn drop(&mut self) {
        if self.has_resources() {
            if let Err(e) = self.stop() {
                common::log_error!("Failed to stop TPROXY on drop: {}", e);
            }
        }
    }
}

//
// Helper to run shell commands.
//

fn run_command(cmd: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(cmd)
        .args(args)
        .output_bounded()
        .context(format!("Failed to execute {}", cmd))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} failed: {}", cmd, stderr.trim());
    }

    Ok(())
}

fn run_cleanup_command(cmd: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(cmd)
        .args(args)
        .output_bounded()
        .with_context(|| format!("Failed to execute {}", cmd))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if cleanup_error_missing(&output.stderr) {
        return Ok(());
    }

    anyhow::bail!("{} {:?} failed: {}", cmd, args, stderr.trim())
}

fn cleanup_error_missing(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    stderr.contains("no such file or directory")
        || stderr.contains("no chain/target/match")
        || stderr.contains("bad rule")
        || stderr.contains("cannot find device")
        || stderr.contains("no such process")
        || stderr.contains("fib table does not exist")
}

//
// Create a transparent TCP listener that can accept connections for any IP.
//

use socket2::{Domain, Protocol, Socket, Type};
use std::net::TcpListener as StdTcpListener;

/// Create a TCP listener with IP_TRANSPARENT socket option.
///
/// This allows the socket to accept connections destined for any IP address,
/// which is required for TPROXY to work.
pub fn create_transparent_listener(addr: &str) -> Result<StdTcpListener> {
    let addr: std::net::SocketAddr = addr.parse().context("Invalid address")?;
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))
        .context("Failed to create socket")?;

    //
    // Set IP_TRANSPARENT to allow binding/accepting for any IP.
    //

    socket
        .set_ip_transparent_v4(true)
        .context("Failed to set IP_TRANSPARENT")?;

    //
    // Allow address reuse.
    //

    socket.set_reuse_address(true).ok();

    //
    // Bind and listen.
    //

    socket
        .bind(&addr.into())
        .context("Failed to bind transparent socket")?;
    socket.listen(128).context("Failed to listen")?;

    //
    // Set non-blocking for tokio.
    //

    socket
        .set_nonblocking(true)
        .context("Failed to set non-blocking")?;

    Ok(socket.into())
}
