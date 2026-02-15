impl NodeInterceptManager {
    //
    // Synchronous cleanup for Proxy and Hosts methods.
    // Called by both disable() and Drop.
    //

    fn cleanup_proxy_sync(&mut self) {
        if let Err(e) = disable_system_proxy(self.saved_proxy_settings.as_ref()) {
            common::log_error!("Failed to restore system proxy settings: {}", e);
        }
        self.saved_proxy_settings = None;
    }

    fn cleanup_hosts_sync(&mut self) {
        if let Err(e) = hosts::remove_all_hosts_entries() {
            common::log_error!("Failed to remove hosts file entries: {}", e);
        }
        hosts::disable_hosts_redirect();
        hosts::flush_dns_cache();
    }

    //
    // Synchronous VPN cleanup (signal shutdown, remove routes, stop adapters).
    // The async parts (waiting for tasks) are only in disable_vpn_mode().
    //

    fn cleanup_vpn_sync(&mut self) {
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

    //
    // Synchronous TPROXY cleanup.
    //

    #[cfg(target_os = "linux")]
    fn cleanup_tproxy_sync(&mut self) {
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
    fn cleanup_tproxy_sync(&mut self) {
        // No-op on non-Linux
    }
}

