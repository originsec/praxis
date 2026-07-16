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
    /// 3. Accepted socket local_addr used by proxy as real destination
    #[cfg(target_os = "linux")]
    pub(super) async fn enable_tproxy_mode(&mut self, proxy_port: u16) -> Result<()> {
        common::log_info!("Setting up TPROXY intercept mode (Linux)");

        //
        // 1. Resolve domain IPs before making any system changes.
        //

        self.check_enable_cancelled()?;
        let dns_resolver = Arc::new(
            self.race_cancel(DomainResolver::new())
                .await
                .context("Failed to create DNS resolver")?,
        );

        self.check_enable_cancelled()?;
        let resolved = self
            .race_cancel(dns_resolver.resolve_domains_best_effort(&self.domains))
            .await
            .context("Failed to resolve any TPROXY intercept target")?;

        //
        // 2. Get IPv4 addresses for TPROXY rules.
        //

        let ipv4_ips: Vec<std::net::Ipv4Addr> = resolved
            .into_values()
            .flatten()
            .filter_map(|ip| match ip {
                std::net::IpAddr::V4(v4) => Some(v4),
                std::net::IpAddr::V6(_) => None,
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if ipv4_ips.is_empty() {
            anyhow::bail!("TPROXY interception requires at least one resolved IPv4 target");
        }

        common::log_info!("Setting up TPROXY for {} IPv4 addresses", ipv4_ips.len());

        self.check_enable_cancelled()?;
        let ipv6_original = Ipv6Manager::current_value()
            .context("Failed to read IPv6 state for TPROXY recovery")?;
        let route_localnet_original = TproxyManager::current_route_localnet()
            .context("Failed to read route_localnet for TPROXY recovery")?;
        TproxyManager::ensure_resources_available()
            .context("TPROXY policy-routing resources are already in use")?;
        super::state::update_state(|state| {
            state.tproxy_enabled = true;
            state.tproxy_port = proxy_port;
            state.tproxy_ips = ipv4_ips.iter().map(ToString::to_string).collect();
            state.ipv6_original_value = Some(ipv6_original);
            state.route_localnet_original_value = Some(route_localnet_original);
            state.tproxy_rules_tagged = true;
        })
        .context("Failed to persist recovery state before TPROXY changes")?;

        //
        // 3. Start TPROXY manager (sets up iptables rules + policy routing).
        //

        self.check_enable_cancelled()?;
        let mut tproxy_manager = TproxyManager::new();
        let op_cancel = self.operation_cancel.clone();
        if let Err(error) = tproxy_manager
            .start(proxy_port, &ipv4_ips, op_cancel.as_ref())
            .context("Failed to start TPROXY manager")
        {
            self.tproxy_manager = Some(tproxy_manager);
            return Err(error);
        }
        self.tproxy_manager = Some(tproxy_manager);

        //
        // TPROXY is IPv4-only. Disable IPv6 only after all target resolution
        // and rule setup succeeded; dropping the manager rolls the rules back
        // if this final system change fails.
        //

        self.check_enable_cancelled()?;
        let mut ipv6_manager = Ipv6Manager::new();
        if let Err(error) = ipv6_manager.disable().context("Failed to disable IPv6") {
            self.ipv6_manager = Some(ipv6_manager);
            return Err(error);
        }

        //
        // Store components.
        //

        self.tproxy_dns_resolver = Some(dns_resolver);
        self.ipv6_manager = Some(ipv6_manager);

        Ok(())
    }

    /// Non-Linux stub for TPROXY mode.
    #[cfg(not(target_os = "linux"))]
    pub(super) async fn enable_tproxy_mode(&mut self, _proxy_port: u16) -> Result<()> {
        Err(anyhow::anyhow!("TPROXY mode is only supported on Linux"))
    }

    /// Disable TPROXY mode and clean up components (Linux).
    #[cfg(target_os = "linux")]
    pub(super) async fn disable_tproxy_mode(&mut self) -> Result<()> {
        //
        // Same cleanup as Drop path: remove iptables rules and restore
        // IPv6. Must restore IPv6 here — after disable() sets
        // is_enabled=false, Drop skips method cleanup entirely.
        //
        self.cleanup_tproxy_sync()
    }

    /// Non-Linux stub for TPROXY mode cleanup.
    #[cfg(not(target_os = "linux"))]
    pub(super) async fn disable_tproxy_mode(&mut self) -> Result<()> {
        Ok(())
    }

    //
    // Synchronous TPROXY cleanup.
    //

    #[cfg(target_os = "linux")]
    pub(super) fn cleanup_tproxy_sync(&mut self) -> Result<()> {
        let mut failures = Vec::new();
        //
        // Stop TPROXY manager (removes iptables rules + policy routing).
        //

        let tproxy_cleaned = if let Some(tproxy_manager) = self.tproxy_manager.as_mut() {
            if let Err(e) = tproxy_manager.stop() {
                failures.push(format!("TPROXY rules: {}", e));
                false
            } else {
                true
            }
        } else {
            false
        };
        if tproxy_cleaned {
            self.tproxy_manager = None;
        }

        //
        // Restore IPv6.
        //

        let ipv6_cleaned = if let Some(ipv6_manager) = self.ipv6_manager.as_mut() {
            if let Err(e) = ipv6_manager.restore() {
                failures.push(format!("IPv6 restore: {}", e));
                false
            } else {
                true
            }
        } else {
            false
        };
        if ipv6_cleaned {
            self.ipv6_manager = None;
        }

        //
        // Clear DNS resolver.
        //

        if failures.is_empty() {
            self.tproxy_dns_resolver = None;
        }

        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn cleanup_tproxy_sync(&mut self) -> Result<()> {
        Ok(())
    }
}
