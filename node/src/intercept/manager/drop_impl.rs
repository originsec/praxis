impl Drop for NodeInterceptManager {
    fn drop(&mut self) {
        if !self.is_enabled {
            return;
        }

        let method = self.method.unwrap_or(InterceptMethod::Proxy);
        match method {
            InterceptMethod::Proxy => self.cleanup_proxy_sync(),
            InterceptMethod::Vpn => self.cleanup_vpn_sync(),
            InterceptMethod::Hosts => self.cleanup_hosts_sync(),
            InterceptMethod::Tproxy => self.cleanup_tproxy_sync(),
        }
    }
}
