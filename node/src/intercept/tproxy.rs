//
// TPROXY-based transparent proxy support for Linux.
//
// Uses iptables TPROXY + policy routing to intercept traffic without NAT.
// This avoids the kernel limitation where loopback-to-loopback traffic
// bypasses TC eBPF hooks.
//

use anyhow::{Context, Result};
use std::net::Ipv4Addr;
use std::process::Command;

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
}

impl TproxyManager {
    pub fn new() -> Self {
        Self {
            proxy_port: 0,
            intercept_ips: Vec::new(),
            is_active: false,
        }
    }

    /// Start TPROXY interception.
    ///
    /// Sets up:
    /// 1. Policy routing table and rule for marked packets
    /// 2. route_localnet sysctl to allow local routing of external IPs
    /// 3. iptables TPROXY rules in mangle table
    pub fn start(&mut self, proxy_port: u16, intercept_ips: &[Ipv4Addr]) -> Result<()> {
        if self.is_active {
            common::log_info!("TPROXY already active");
            return Ok(());
        }

        common::log_info!("Starting TPROXY interception on port {}", proxy_port);
        self.proxy_port = proxy_port;
        self.intercept_ips = intercept_ips.to_vec();

        //
        // 1. Enable route_localnet to allow routing external IPs through loopback.
        //

        run_command("sysctl", &["-w", "net.ipv4.conf.lo.route_localnet=1"])
            .context("Failed to enable route_localnet")?;

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
                "-j",
                "RETURN",
            ],
        )
        .context("Failed to add bypass rule")?;

        //
        // 4. Add iptables TPROXY rules for each intercept IP.
        //

        for ip in &self.intercept_ips {
            self.add_tproxy_rule(*ip)?;
        }

        self.is_active = true;
        common::log_info!(
            "TPROXY interception started for {} IPs",
            self.intercept_ips.len()
        );

        Ok(())
    }

    /// Stop TPROXY interception and clean up rules.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_active {
            return Ok(());
        }

        common::log_info!("Stopping TPROXY interception");

        //
        // Remove iptables TPROXY rules.
        //

        for ip in &self.intercept_ips.clone() {
            if let Err(e) = self.remove_tproxy_rule(*ip) {
                common::log_warn!("Failed to remove TPROXY rule for {}: {}", ip, e);
            }
        }

        //
        // Remove bypass rule.
        //

        let _ = run_command(
            "iptables",
            &[
                "-t",
                "mangle",
                "-D",
                "OUTPUT",
                "-m",
                "mark",
                "--mark",
                &TPROXY_BYPASS_MARK.to_string(),
                "-j",
                "RETURN",
            ],
        );

        //
        // Remove policy routing.
        //

        let _ = run_command(
            "ip",
            &[
                "route",
                "del",
                "local",
                "0.0.0.0/0",
                "dev",
                "lo",
                "table",
                &TPROXY_TABLE.to_string(),
            ],
        );

        let _ = run_command(
            "ip",
            &[
                "rule",
                "del",
                "fwmark",
                &TPROXY_MARK.to_string(),
                "lookup",
                &TPROXY_TABLE.to_string(),
            ],
        );

        //
        // Disable route_localnet (restore default).
        //

        let _ = run_command("sysctl", &["-w", "net.ipv4.conf.lo.route_localnet=0"]);

        self.intercept_ips.clear();
        self.is_active = false;

        common::log_info!("TPROXY interception stopped");
        Ok(())
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

        let _ = run_command(
            "iptables",
            &[
                "-t",
                "mangle",
                "-D",
                "OUTPUT",
                "-p",
                "tcp",
                "-d",
                &ip_str,
                "--dport",
                "443",
                "-j",
                "MARK",
                "--set-mark",
                &mark_str,
            ],
        );

        let _ = run_command(
            "iptables",
            &[
                "-t",
                "mangle",
                "-D",
                "PREROUTING",
                "-p",
                "tcp",
                "-d",
                &ip_str,
                "--dport",
                "443",
                "-j",
                "TPROXY",
                "--on-port",
                &port_str,
                "--tproxy-mark",
                &mark_str,
            ],
        );

        common::log_debug!("Removed TPROXY rule for {}", ip);
        Ok(())
    }

    /// Update the list of intercept IPs.
    #[allow(dead_code)]
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
        if self.is_active {
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
        .output()
        .context(format!("Failed to execute {}", cmd))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} failed: {}", cmd, stderr.trim());
    }

    Ok(())
}

//
// SO_ORIGINAL_DST support for getting the real destination from TPROXY.
//

use std::net::SocketAddr;
use std::os::fd::AsRawFd;

/// Get the original destination address from a socket using SO_ORIGINAL_DST.
///
/// This is used with TPROXY to determine where the connection was originally
/// going before it was redirected to the proxy.
pub fn get_original_dst(socket: &tokio::net::TcpStream) -> Result<SocketAddr> {
    use std::mem;

    let fd = socket.as_raw_fd();

    //
    // SO_ORIGINAL_DST = 80 (defined in linux/netfilter_ipv4.h)
    // SOL_IP = 0
    //

    const SOL_IP: libc::c_int = 0;
    const SO_ORIGINAL_DST: libc::c_int = 80;

    let mut addr: libc::sockaddr_in = unsafe { mem::zeroed() };
    let mut len: libc::socklen_t = mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;

    let ret = unsafe {
        libc::getsockopt(
            fd,
            SOL_IP,
            SO_ORIGINAL_DST,
            &mut addr as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };

    if ret < 0 {
        return Err(std::io::Error::last_os_error()).context("getsockopt SO_ORIGINAL_DST failed");
    }

    //
    // Convert to SocketAddr.
    //

    let ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
    let port = u16::from_be(addr.sin_port);

    Ok(SocketAddr::new(std::net::IpAddr::V4(ip), port))
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
