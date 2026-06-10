//
// TPROXY-mode interception (Linux): iptables TPROXY rules + policy routing.
// Split out of mod.rs; these are inherent methods on NodeInterceptManager.
//

#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;

#[cfg(target_os = "linux")]
use std::sync::Arc;

#[cfg(target_os = "linux")]
use super::dns_resolver::DomainResolver;
#[cfg(target_os = "linux")]
use super::routing::Ipv6Manager;
#[cfg(target_os = "linux")]
use super::tproxy::TproxyManager;

use super::NodeInterceptManager;

impl NodeInterceptManager {
    /// Enable TPROXY mode with iptables-based packet interception (Linux).
    ///
    /// This sets up:
    /// 1. iptables TPROXY rules to redirect traffic to proxy
    /// 2. Policy routing for marked packets
    /// 3. SO_ORIGINAL_DST used by proxy to get real destination
    #[cfg(target_os = "linux")]
    pub(super) async fn enable_tproxy_mode(&mut self, proxy_port: u16) -> Result<()> {
        common::log_info!("Setting up TPROXY intercept mode (Linux)");

        //
        // 0. Disable IPv6 to avoid routing issues.
        //    TPROXY rules only handle IPv4 currently.
        //
        let mut ipv6_manager = Ipv6Manager::new();
        ipv6_manager.disable().context("Failed to disable IPv6")?;
        self.ipv6_manager = Some(ipv6_manager);

        //
        // 1. Resolve domain IPs.
        //

        let dns_resolver = Arc::new(
            DomainResolver::new()
                .await
                .context("Failed to create DNS resolver")?,
        );

        for domain in &self.domains {
            match dns_resolver.resolve_domain(domain).await {
                Ok(ips) => {
                    common::log_debug!("Resolved {} to {:?}", domain, ips);
                }
                Err(e) => {
                    common::log_warn!("Failed to resolve {}: {}", domain, e);
                }
            }
        }

        //
        // 2. Get IPv4 addresses for TPROXY rules.
        //

        let intercept_ips = dns_resolver.get_all_intercept_ips();
        let ipv4_ips: Vec<std::net::Ipv4Addr> = intercept_ips
            .iter()
            .filter_map(|ip| match ip {
                std::net::IpAddr::V4(v4) => Some(*v4),
                std::net::IpAddr::V6(_) => None,
            })
            .collect();

        common::log_info!("Setting up TPROXY for {} IPv4 addresses", ipv4_ips.len());

        //
        // 3. Start TPROXY manager (sets up iptables rules + policy routing).
        //

        let mut tproxy_manager = TproxyManager::new();
        tproxy_manager
            .start(proxy_port, &ipv4_ips)
            .context("Failed to start TPROXY manager")?;

        //
        // Store components.
        //

        self.tproxy_manager = Some(tproxy_manager);
        self.tproxy_dns_resolver = Some(dns_resolver);

        Ok(())
    }

    /// Non-Linux stub for TPROXY mode.
    #[cfg(not(target_os = "linux"))]
    pub(super) async fn enable_tproxy_mode(&mut self, _proxy_port: u16) -> Result<()> {
        Err(anyhow::anyhow!("TPROXY mode is only supported on Linux"))
    }

    /// Disable TPROXY mode and clean up components (Linux).
    #[cfg(target_os = "linux")]
    pub(super) async fn disable_tproxy_mode(&mut self) {
        common::log_info!("Disabling TPROXY mode");

        //
        // Stop TPROXY manager (removes iptables rules + policy routing).
        //

        if let Some(mut tproxy_manager) = self.tproxy_manager.take() {
            if let Err(e) = tproxy_manager.stop() {
                common::log_error!("Failed to stop TPROXY manager: {}", e);
            }
        }

        //
        // Clear DNS resolver.
        //

        self.tproxy_dns_resolver = None;
    }

    /// Non-Linux stub for TPROXY mode cleanup.
    #[cfg(not(target_os = "linux"))]
    pub(super) async fn disable_tproxy_mode(&mut self) {
        // No-op on non-Linux
    }

    //
    // Synchronous TPROXY cleanup.
    //

    #[cfg(target_os = "linux")]
    pub(super) fn cleanup_tproxy_sync(&mut self) {
        //
        // Stop TPROXY manager (removes iptables rules + policy routing).
        //

        if let Some(mut tproxy_manager) = self.tproxy_manager.take() {
            if let Err(e) = tproxy_manager.stop() {
                common::log_error!("Failed to stop TPROXY manager: {}", e);
            }
        }

        //
        // Restore IPv6.
        //

        if let Some(mut ipv6_manager) = self.ipv6_manager.take() {
            if let Err(e) = ipv6_manager.restore() {
                common::log_error!("Failed to restore IPv6: {}", e);
            }
        }

        //
        // Clear DNS resolver.
        //

        self.tproxy_dns_resolver = None;
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn cleanup_tproxy_sync(&mut self) {
        // No-op on non-Linux
    }
}
