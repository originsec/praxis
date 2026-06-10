//
// VPN-mode interception: TUN adapter + packet engine + routing. Split out
// of mod.rs; these are inherent methods on NodeInterceptManager.
//

#[cfg(any(target_os = "windows", target_os = "linux"))]
use anyhow::Context;
use anyhow::Result;

#[cfg(any(target_os = "windows", target_os = "linux"))]
use std::sync::Arc;

#[cfg(any(target_os = "windows", target_os = "linux"))]
use tokio_util::sync::CancellationToken;

#[cfg(any(target_os = "windows", target_os = "linux"))]
use super::dns_resolver::DomainResolver;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use super::packet_engine::PacketEngine;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use super::routing::RouteManager;
#[cfg(target_os = "linux")]
use super::routing::{Ipv6Manager, VpnBypassManager};
#[cfg(target_os = "windows")]
use super::tun_device::SharedTunDevice;
#[cfg(target_os = "linux")]
use super::tun_linux::LinuxTunManager;
#[cfg(target_os = "windows")]
use super::wintun::WintunManager;

use super::NodeInterceptManager;

impl NodeInterceptManager {
    /// Enable VPN mode with packet-level routing (Windows).
    ///
    /// This sets up:
    /// 1. Wintun adapter with packet session
    /// 2. DNS resolution for intercept domains
    /// 3. Routes for resolved IPs through the TUN adapter
    /// 4. Packet engine for NAT and forwarding
    #[cfg(target_os = "windows")]
    pub(super) async fn enable_vpn_mode(&mut self, proxy_port: u16) -> Result<()> {
        use super::routing::TUN_INTERFACE_NAME;
        use super::tun_device::WintunDevice;

        common::log_info!("Setting up VPN mode with packet routing (Windows)");

        //
        // 1. Start wintun adapter with session.
        //
        let mut wintun_manager = WintunManager::new();
        wintun_manager
            .start()
            .context("Failed to start wintun VPN adapter")?;

        let session = wintun_manager
            .session()
            .ok_or_else(|| anyhow::anyhow!("Wintun session not available"))?;

        //
        // Wrap the wintun session in our TunDevice abstraction.
        //
        let tun_device: SharedTunDevice = Arc::new(WintunDevice::new(session));

        //
        // 2. Configure TUN interface IP.
        //
        let mut route_manager = RouteManager::new(TUN_INTERFACE_NAME);
        route_manager
            .configure_interface()
            .context("Failed to configure TUN interface")?;

        //
        // 3. Resolve domain IPs.
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
        // 4. Add routes for all resolved IPs.
        //
        let intercept_ips = dns_resolver.get_all_intercept_ips();
        common::log_info!("Adding routes for {} IPs", intercept_ips.len());

        for ip in &intercept_ips {
            if let Err(e) = route_manager.add_route(*ip) {
                common::log_warn!("Failed to add route for {}: {}", ip, e);
            }
        }

        //
        // 5. Start packet engine.
        //
        let packet_engine = Arc::new(PacketEngine::new(
            tun_device.clone(),
            proxy_port,
            dns_resolver.clone(),
        ));

        //
        // Refresh intercept IPs in the packet engine.
        //
        packet_engine.refresh_intercept_ips().await;

        let shutdown_token = CancellationToken::new();
        let engine_shutdown = shutdown_token.clone();
        let engine = packet_engine.clone();

        let task = tokio::spawn(async move {
            engine.run(engine_shutdown).await;
        });

        //
        // Store all components.
        //
        self.wintun_manager = Some(wintun_manager);
        self.tun_device = Some(tun_device);
        self.dns_resolver = Some(dns_resolver);
        self.route_manager = Some(route_manager);
        self.packet_engine_task = Some(task);
        self.shutdown_token = Some(shutdown_token);

        Ok(())
    }

    /// Enable VPN mode with packet-level routing (Linux).
    ///
    /// This sets up:
    /// 1. VPN bypass routing (policy routing for proxy's outbound connections)
    /// 2. TUN device via the tun crate
    /// 3. DNS resolution for intercept domains
    /// 4. Routes for resolved IPs through the TUN device
    /// 5. Packet engine for NAT and forwarding
    #[cfg(target_os = "linux")]
    pub(super) async fn enable_vpn_mode(&mut self, proxy_port: u16) -> Result<()> {
        use super::tun_linux::ADAPTER_NAME;

        common::log_info!("Setting up VPN mode with packet routing (Linux)");

        //
        // 0. Disable IPv6 to avoid routing issues with TUN device.
        //    IPv6 traffic doesn't go through our packet engine properly.
        //
        let mut ipv6_manager = Ipv6Manager::new();
        ipv6_manager.disable().context("Failed to disable IPv6")?;

        //
        // 1. Set up VPN bypass routing FIRST (before adding TUN routes).
        //    This discovers the default gateway and sets up policy routing
        //    so that proxy's outbound connections (with SO_MARK) bypass TUN.
        //
        let mut vpn_bypass_manager = VpnBypassManager::new();
        vpn_bypass_manager
            .start()
            .context("Failed to set up VPN bypass routing")?;

        //
        // 2. Start Linux TUN device.
        //
        let mut tun_manager = LinuxTunManager::new();
        tun_manager
            .start()
            .context("Failed to start Linux TUN device. Ensure you have CAP_NET_ADMIN or are running as root.")?;

        let tun_device = tun_manager
            .device()
            .ok_or_else(|| anyhow::anyhow!("TUN device not available"))?;

        //
        // 3. Configure TUN interface IP.
        //
        let mut route_manager = RouteManager::new(ADAPTER_NAME);
        route_manager
            .configure_interface()
            .context("Failed to configure TUN interface")?;

        //
        // 4. Resolve domain IPs.
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
        // 5. Add routes for all resolved IPs.
        //
        let intercept_ips = dns_resolver.get_all_intercept_ips();
        common::log_info!("Adding routes for {} IPs", intercept_ips.len());

        for ip in &intercept_ips {
            if let Err(e) = route_manager.add_route(*ip) {
                common::log_warn!("Failed to add route for {}: {}", ip, e);
            }
        }

        //
        // 6. Start packet engine.
        //
        let packet_engine = Arc::new(PacketEngine::new(
            tun_device.clone(),
            proxy_port,
            dns_resolver.clone(),
        ));

        //
        // Refresh intercept IPs in the packet engine.
        //
        packet_engine.refresh_intercept_ips().await;

        let shutdown_token = CancellationToken::new();
        let engine_shutdown = shutdown_token.clone();
        let engine = packet_engine.clone();

        let task = tokio::spawn(async move {
            engine.run(engine_shutdown).await;
        });

        //
        // Store all components.
        //
        self.tun_manager = Some(tun_manager);
        self.tun_device = Some(tun_device);
        self.dns_resolver = Some(dns_resolver);
        self.route_manager = Some(route_manager);
        self.vpn_bypass_manager = Some(vpn_bypass_manager);
        self.ipv6_manager = Some(ipv6_manager);
        self.packet_engine_task = Some(task);
        self.shutdown_token = Some(shutdown_token);

        Ok(())
    }

    /// Non-Windows/non-Linux stub for VPN mode.
    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    pub(super) async fn enable_vpn_mode(&mut self, _proxy_port: u16) -> Result<()> {
        Err(anyhow::anyhow!(
            "VPN mode is only supported on Windows and Linux"
        ))
    }

    /// Disable VPN mode and clean up components (Windows/Linux).
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub(super) async fn disable_vpn_mode(&mut self) {
        common::log_info!("Disabling VPN mode");

        //
        // Signal shutdown and wait for packet engine task (async part).
        //

        if let Some(token) = self.shutdown_token.take() {
            common::log_debug!("Signaling packet engine shutdown");
            token.cancel();
        }

        if let Some(ref device) = self.tun_device {
            common::log_debug!("Shutting down TUN device");
            device.shutdown();
        }

        if let Some(task) = self.packet_engine_task.take() {
            common::log_debug!("Waiting for packet engine to stop");
            let _ = task.await;
        }

        //
        // Do the rest of the sync cleanup.
        //

        self.cleanup_vpn_sync();
    }

    /// Disable VPN mode (non-Windows/non-Linux stub).
    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    pub(super) async fn disable_vpn_mode(&mut self) {
        if let Some(mut route_manager) = self.route_manager.take() {
            let _ = route_manager.remove_all_routes();
        }
    }

    //
    // Synchronous VPN cleanup (signal shutdown, remove routes, stop adapters).
    // The async parts (waiting for tasks) are only in disable_vpn_mode().
    //

    pub(super) fn cleanup_vpn_sync(&mut self) {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        if let Some(token) = self.shutdown_token.take() {
            token.cancel();
        }

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        if let Some(ref device) = self.tun_device {
            device.shutdown();
        }

        if let Some(mut route_manager) = self.route_manager.take() {
            if let Err(e) = route_manager.remove_all_routes() {
                common::log_error!("Failed to remove routes: {}", e);
            }
        }

        //
        // Clean up VPN bypass routing (policy routing rules).
        //
        if let Some(mut vpn_bypass_manager) = self.vpn_bypass_manager.take() {
            if let Err(e) = vpn_bypass_manager.stop() {
                common::log_error!("Failed to stop VPN bypass routing: {}", e);
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

        #[cfg(target_os = "windows")]
        if let Some(mut wintun_manager) = self.wintun_manager.take() {
            if let Err(e) = wintun_manager.stop() {
                common::log_error!("Failed to stop wintun adapter: {}", e);
            }
        }

        #[cfg(target_os = "linux")]
        if let Some(mut tun_manager) = self.tun_manager.take() {
            if let Err(e) = tun_manager.stop() {
                common::log_error!("Failed to stop TUN manager: {}", e);
            }
        }

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            self.tun_device = None;
            self.dns_resolver = None;
        }
    }
}
