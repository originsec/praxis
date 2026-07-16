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
    //
    // Phase TunUp: create the TUN adapter and assign TUN_IP so the proxy can
    // bind 10.255.0.1 before MethodRouting (routes + packet engine).
    //

    /// Bring up the VPN TUN interface and assign its address (before proxy bind).
    #[cfg(target_os = "windows")]
    pub(super) async fn prepare_vpn_tun(&mut self) -> Result<()> {
        use super::routing::TUN_INTERFACE_NAME;
        use super::tun_device::WintunDevice;

        common::log_info!("Preparing VPN TUN (Windows) before proxy bind");

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
            .context("Failed to resolve any VPN intercept target")?;
        self.check_enable_cancelled()?;
        let intercept_ips: std::collections::HashSet<std::net::IpAddr> = resolved
            .into_values()
            .flatten()
            .filter(std::net::IpAddr::is_ipv4)
            .collect();
        if intercept_ips.is_empty() {
            anyhow::bail!("VPN interception requires at least one resolved IPv4 target");
        }
        RouteManager::ensure_routes_available(&intercept_ips)
            .context("One or more VPN target routes are already in use")?;

        //
        // Attach each manager to self after privileged mutation — including on
        // start Err when it still owns host resources — so rollback_partial_enable
        // can run stop() and gate remove_state on the result (not Drop).
        //
        self.check_enable_cancelled()?;
        let mut wintun_manager = WintunManager::new();
        if let Err(e) = wintun_manager.start() {
            if wintun_manager.is_active() {
                self.wintun_manager = Some(wintun_manager);
            }
            return Err(e).context("Failed to start wintun VPN adapter");
        }
        self.wintun_manager = Some(wintun_manager);

        let session = self
            .wintun_manager
            .as_ref()
            .and_then(|m| m.session())
            .ok_or_else(|| anyhow::anyhow!("Wintun session not available"))?;
        let tun_device: SharedTunDevice = Arc::new(WintunDevice::new(session));
        self.tun_device = Some(tun_device);
        self.dns_resolver = Some(dns_resolver);

        let mut route_manager = RouteManager::new(TUN_INTERFACE_NAME);
        if let Err(e) = route_manager.configure_interface() {
            //
            // configure_interface only sets interface_configured on success;
            // keep route_manager on self only if routes were tracked (none yet).
            //
            let _ = route_manager;
            return Err(e).context("Failed to configure TUN interface");
        }
        self.route_manager = Some(route_manager);

        super::state::update_state(|state| {
            state.vpn_interface = Some(TUN_INTERFACE_NAME.to_string());
            state.vpn_routes = intercept_ips.iter().map(ToString::to_string).collect();
        })
        .context("Failed to persist recovery state after TUN configure")?;

        Ok(())
    }

    /// Finish VPN after the proxy is bound: install routes and start the packet engine.
    #[cfg(target_os = "windows")]
    pub(super) async fn finish_vpn_mode(&mut self, proxy_port: u16) -> Result<()> {
        common::log_info!("Finishing VPN mode (Windows) with proxy port {}", proxy_port);

        self.check_enable_cancelled()?;
        //
        // Clone cancel token before mut-borrowing route_manager so the route
        // loop can check cancellation without E0502.
        //
        let op_cancel = self.operation_cancel.clone();
        let dns_resolver = self
            .dns_resolver
            .clone()
            .ok_or_else(|| anyhow::anyhow!("VPN DNS resolver missing after TunUp"))?;
        let tun_device = self
            .tun_device
            .clone()
            .ok_or_else(|| anyhow::anyhow!("VPN TUN device missing after TunUp"))?;
        let route_manager = self
            .route_manager
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("VPN route manager missing after TunUp"))?;

        let intercept_ips = dns_resolver.get_all_intercept_ips();
        let intercept_ips: std::collections::HashSet<std::net::IpAddr> = intercept_ips
            .into_iter()
            .filter(std::net::IpAddr::is_ipv4)
            .collect();

        common::log_info!("Adding routes for {} IPs", intercept_ips.len());
        let mut routes_added = 0usize;
        let mut route_errors = Vec::new();
        for ip in &intercept_ips {
            if op_cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
                anyhow::bail!("Intercept enable cancelled");
            }
            match route_manager.add_route(*ip) {
                Ok(()) => routes_added += 1,
                Err(e) => route_errors.push(format!("{}: {}", ip, e)),
            }
        }
        if routes_added != intercept_ips.len() {
            anyhow::bail!(
                "Failed to install every VPN target route ({}/{} installed): {}",
                routes_added,
                intercept_ips.len(),
                route_errors.join("; ")
            );
        }

        self.check_enable_cancelled()?;
        let packet_engine = Arc::new(PacketEngine::new(
            tun_device.clone(),
            proxy_port,
            dns_resolver.clone(),
        ));
        self.race_cancel(async {
            packet_engine.refresh_intercept_ips().await;
            Ok::<(), anyhow::Error>(())
        })
        .await?;
        self.check_enable_cancelled()?;

        let shutdown_token = CancellationToken::new();
        let engine_shutdown = shutdown_token.clone();
        let engine = packet_engine.clone();
        let task = tokio::spawn(async move {
            engine.run(engine_shutdown).await;
        });

        self.packet_engine_task = Some(task);
        self.shutdown_token = Some(shutdown_token);
        Ok(())
    }

    /// Bring up the VPN TUN interface and assign its address (before proxy bind).
    #[cfg(target_os = "linux")]
    pub(super) async fn prepare_vpn_tun(&mut self) -> Result<()> {
        use super::tun_linux::ADAPTER_NAME;

        common::log_info!("Preparing VPN TUN (Linux) before proxy bind");

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
            .context("Failed to resolve any VPN intercept target")?;
        self.check_enable_cancelled()?;
        let intercept_ips: std::collections::HashSet<std::net::IpAddr> = resolved
            .into_values()
            .flatten()
            .filter(std::net::IpAddr::is_ipv4)
            .collect();
        if intercept_ips.is_empty() {
            anyhow::bail!("VPN interception requires at least one resolved IPv4 target");
        }
        RouteManager::ensure_routes_available(&intercept_ips)
            .context("One or more VPN target routes are already in use")?;
        VpnBypassManager::ensure_resources_available()
            .context("VPN bypass policy-routing resources are already in use")?;

        //
        // Attach each manager to self after privileged mutation — including on
        // start/disable Err when it still owns host resources — so
        // rollback_partial_enable runs stop/restore and gates remove_state
        // (do not rely on Drop discarding cleanup errors).
        //
        self.check_enable_cancelled()?;
        let mut ipv6_manager = Ipv6Manager::new();
        let ipv6_original =
            Ipv6Manager::current_value().context("Failed to read IPv6 state for VPN recovery")?;
        super::state::update_state(|state| {
            state.ipv6_original_value = Some(ipv6_original);
        })
        .context("Failed to persist recovery state before VPN changes")?;
        if let Err(e) = ipv6_manager.disable() {
            //
            // disable() only sets is_disabled after a successful sysctl write.
            //
            return Err(e).context("Failed to disable IPv6");
        }
        self.ipv6_manager = Some(ipv6_manager);

        let mut vpn_bypass_manager = VpnBypassManager::new();
        super::state::update_state(|state| {
            state.vpn_bypass_enabled = true;
        })
        .context("Failed to persist VPN bypass recovery intent")?;
        if let Err(e) = vpn_bypass_manager.start() {
            //
            // start() may leave rule_added/route_added after a failed stop()
            // during its own rollback. Attach so disable_vpn_mode can stop()
            // and gate remove_state on that result.
            //
            if vpn_bypass_manager.owns_host_resources() {
                self.vpn_bypass_manager = Some(vpn_bypass_manager);
            }
            return Err(e).context("Failed to set up VPN bypass routing");
        }
        self.vpn_bypass_manager = Some(vpn_bypass_manager);

        let mut tun_manager = LinuxTunManager::new();
        if let Err(e) = tun_manager.start() {
            if tun_manager.is_active() {
                self.tun_manager = Some(tun_manager);
            }
            return Err(e).context(
                "Failed to start Linux TUN device. Ensure you have CAP_NET_ADMIN or are running as root.",
            );
        }
        let tun_device = tun_manager
            .device()
            .ok_or_else(|| anyhow::anyhow!("TUN device not available"))?;
        self.tun_manager = Some(tun_manager);
        self.tun_device = Some(tun_device);
        self.dns_resolver = Some(dns_resolver);

        let mut route_manager = RouteManager::new(ADAPTER_NAME);
        if let Err(e) = route_manager.configure_interface() {
            let _ = route_manager;
            return Err(e).context("Failed to configure TUN interface");
        }
        self.route_manager = Some(route_manager);

        super::state::update_state(|state| {
            state.vpn_interface = Some(ADAPTER_NAME.to_string());
            state.vpn_routes = intercept_ips.iter().map(ToString::to_string).collect();
        })
        .context("Failed to persist recovery state after TUN configure")?;

        Ok(())
    }

    /// Finish VPN after the proxy is bound: install routes and start the packet engine.
    #[cfg(target_os = "linux")]
    pub(super) async fn finish_vpn_mode(&mut self, proxy_port: u16) -> Result<()> {
        common::log_info!("Finishing VPN mode (Linux) with proxy port {}", proxy_port);

        self.check_enable_cancelled()?;
        //
        // Clone cancel token before mut-borrowing route_manager so the route
        // loop can check cancellation without E0502.
        //
        let op_cancel = self.operation_cancel.clone();
        let dns_resolver = self
            .dns_resolver
            .clone()
            .ok_or_else(|| anyhow::anyhow!("VPN DNS resolver missing after TunUp"))?;
        let tun_device = self
            .tun_device
            .clone()
            .ok_or_else(|| anyhow::anyhow!("VPN TUN device missing after TunUp"))?;
        let route_manager = self
            .route_manager
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("VPN route manager missing after TunUp"))?;

        let intercept_ips = dns_resolver.get_all_intercept_ips();
        let intercept_ips: std::collections::HashSet<std::net::IpAddr> = intercept_ips
            .into_iter()
            .filter(std::net::IpAddr::is_ipv4)
            .collect();

        common::log_info!("Adding routes for {} IPs", intercept_ips.len());
        let mut routes_added = 0usize;
        let mut route_errors = Vec::new();
        for ip in &intercept_ips {
            if op_cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
                anyhow::bail!("Intercept enable cancelled");
            }
            match route_manager.add_route(*ip) {
                Ok(()) => routes_added += 1,
                Err(e) => route_errors.push(format!("{}: {}", ip, e)),
            }
        }
        if routes_added != intercept_ips.len() {
            anyhow::bail!(
                "Failed to install every VPN target route ({}/{} installed): {}",
                routes_added,
                intercept_ips.len(),
                route_errors.join("; ")
            );
        }

        self.check_enable_cancelled()?;
        let packet_engine = Arc::new(PacketEngine::new(
            tun_device.clone(),
            proxy_port,
            dns_resolver.clone(),
        ));
        self.race_cancel(async {
            packet_engine.refresh_intercept_ips().await;
            Ok::<(), anyhow::Error>(())
        })
        .await?;
        self.check_enable_cancelled()?;

        let shutdown_token = CancellationToken::new();
        let engine_shutdown = shutdown_token.clone();
        let engine = packet_engine.clone();
        let task = tokio::spawn(async move {
            engine.run(engine_shutdown).await;
        });

        self.packet_engine_task = Some(task);
        self.shutdown_token = Some(shutdown_token);
        Ok(())
    }

    /// Non-Windows/non-Linux stub for VPN TUN prepare.
    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    pub(super) async fn prepare_vpn_tun(&mut self) -> Result<()> {
        Err(anyhow::anyhow!(
            "VPN mode is only supported on Windows and Linux"
        ))
    }

    /// Non-Windows/non-Linux stub for VPN finish.
    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    pub(super) async fn finish_vpn_mode(&mut self, _proxy_port: u16) -> Result<()> {
        Err(anyhow::anyhow!(
            "VPN mode is only supported on Windows and Linux"
        ))
    }

    /// Disable VPN mode and clean up components (Windows/Linux).
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub(super) async fn disable_vpn_mode(&mut self) -> Result<()> {
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

        let mut failures = Vec::new();
        let mut engine_confirmed_stopped = true;
        if let Some(mut task) = self.packet_engine_task.take() {
            common::log_debug!("Waiting for packet engine to stop (bounded)");
            //
            // Poll JoinHandle by mut ref so timeout does not drop/detach it.
            // On timeout: abort, short re-await; if still live put the handle
            // back and skip adapter/TUN teardown so a later Disable can retry.
            //
            match tokio::time::timeout(std::time::Duration::from_secs(5), &mut task).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if !e.is_cancelled() {
                        failures.push(format!("Packet engine task failed: {}", e));
                    }
                }
                Err(_) => {
                    task.abort();
                    match tokio::time::timeout(std::time::Duration::from_secs(2), &mut task).await
                    {
                        Ok(Ok(())) | Ok(Err(_)) => {
                            failures.push(
                                "Packet engine join timed out; aborted and confirmed stopped"
                                    .into(),
                            );
                        }
                        Err(_) => {
                            engine_confirmed_stopped = false;
                            self.packet_engine_task = Some(task);
                            failures.push(
                                "Packet engine join timed out; task handle retained for retry"
                                    .into(),
                            );
                        }
                    }
                }
            }
        }

        //
        // Never tear down adapter/device while the engine task may still run.
        //
        if super::lifecycle::may_teardown_vpn_after_engine_join(engine_confirmed_stopped) {
            if let Err(e) = self.cleanup_vpn_sync() {
                failures.push(e.to_string());
            }
        } else {
            failures.push(
                "VPN adapter/route cleanup skipped while packet engine task still owned".into(),
            );
        }
        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }

    /// Disable VPN mode (non-Windows/non-Linux stub).
    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    pub(super) async fn disable_vpn_mode(&mut self) -> Result<()> {
        if let Some(mut route_manager) = self.route_manager.take() {
            route_manager.remove_all_routes()?;
        }
        Ok(())
    }

    //
    // Synchronous VPN cleanup (signal shutdown, remove routes, stop adapters).
    // The async parts (waiting for tasks) are only in disable_vpn_mode().
    //

    pub(super) fn cleanup_vpn_sync(&mut self) -> Result<()> {
        let mut failures = Vec::new();
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        if let Some(token) = self.shutdown_token.take() {
            token.cancel();
        }

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        if let Some(ref device) = self.tun_device {
            device.shutdown();
        }

        let routes_cleaned = if let Some(route_manager) = self.route_manager.as_mut() {
            if let Err(e) = route_manager.remove_all_routes() {
                failures.push(format!("routes: {}", e));
                false
            } else {
                true
            }
        } else {
            false
        };
        if routes_cleaned {
            self.route_manager = None;
        }

        //
        // Clean up VPN bypass routing (policy routing rules).
        //
        let bypass_cleaned = if let Some(vpn_bypass_manager) = self.vpn_bypass_manager.as_mut() {
            if let Err(e) = vpn_bypass_manager.stop() {
                failures.push(format!("VPN bypass: {}", e));
                false
            } else {
                true
            }
        } else {
            false
        };
        if bypass_cleaned {
            self.vpn_bypass_manager = None;
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

        #[cfg(target_os = "windows")]
        let adapter_cleaned = if let Some(wintun_manager) = self.wintun_manager.as_mut() {
            if let Err(e) = wintun_manager.stop() {
                failures.push(format!("Wintun: {}", e));
                false
            } else {
                true
            }
        } else {
            false
        };
        #[cfg(target_os = "windows")]
        if adapter_cleaned {
            self.wintun_manager = None;
        }

        #[cfg(target_os = "linux")]
        let adapter_cleaned = if let Some(tun_manager) = self.tun_manager.as_mut() {
            if let Err(e) = tun_manager.stop() {
                failures.push(format!("TUN: {}", e));
                false
            } else {
                true
            }
        } else {
            false
        };
        #[cfg(target_os = "linux")]
        if adapter_cleaned {
            self.tun_manager = None;
        }

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        if failures.is_empty() {
            self.tun_device = None;
            self.dns_resolver = None;
        }

        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }
}
