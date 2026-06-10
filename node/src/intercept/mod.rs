pub mod certificate;
pub mod dns_resolver;
pub mod env_vars;
pub mod hosts;
pub mod packet_engine;
pub mod proxy;
pub mod routing;
pub mod state;
pub mod system_proxy;
#[cfg(target_os = "linux")]
pub mod tproxy;
mod tproxy_mode;
pub mod tun_device;
#[cfg(target_os = "linux")]
pub mod tun_linux;
mod vpn_mode;
pub mod wintun;

pub use certificate::CertificateAuthority;
pub use proxy::{InterceptProxy, ProxyConfig};
pub use state::cleanup_stale_state;
pub use system_proxy::{SavedProxySettings, disable_system_proxy, enable_system_proxy};
#[cfg(target_os = "linux")]
pub use tproxy::TproxyManager;
#[cfg(target_os = "linux")]
pub use tun_linux::LinuxTunManager;
#[cfg(target_os = "windows")]
pub use wintun::WintunManager;

use anyhow::{Context, Result};
use common::{InterceptMethod, InterceptTargetConfig, InterceptedTrafficEntry};
use dns_resolver::DomainResolver;
use routing::{Ipv6Manager, RouteManager, VpnBypassManager};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use tokio::task::JoinHandle;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use tokio_util::sync::CancellationToken;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use tun_device::SharedTunDevice;

/// Node-level intercept manager
///
/// Manages traffic interception at the node level, collecting domains
/// from all agents that support interception. Supports four methods:
/// - Proxy: Uses system proxy settings (Windows registry / Linux env vars)
/// - VPN: Uses TUN adapter with packet-level routing (Windows/Linux)
/// - Hosts: Uses hosts file redirection only
/// - Tproxy: Uses iptables TPROXY for transparent proxying (Linux only)
pub struct NodeInterceptManager {
    /// Whether interception is currently enabled
    is_enabled: bool,
    /// Current interception method
    method: Option<InterceptMethod>,
    /// Certificate Authority for generating TLS certificates
    ca: Option<Arc<RwLock<CertificateAuthority>>>,
    /// The running proxy server
    proxy: Option<InterceptProxy>,
    /// Domains being intercepted
    domains: HashSet<String>,
    /// Mapping of domain to agent short name
    domain_to_agent: HashMap<String, String>,
    /// Saved proxy settings for restoration (Proxy method)
    saved_proxy_settings: Option<SavedProxySettings>,
    /// Wintun manager (VPN method, Windows)
    #[cfg(target_os = "windows")]
    wintun_manager: Option<WintunManager>,
    /// Linux TUN manager (VPN method, Linux)
    #[cfg(target_os = "linux")]
    tun_manager: Option<LinuxTunManager>,
    /// Channel to send intercepted traffic to main for forwarding to service
    traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
    /// Node ID for traffic entries
    node_id: String,
    /// Proxy port (when enabled)
    proxy_port: Option<u16>,
    /// Persistent state for crash recovery
    intercept_state: Option<state::InterceptState>,

    //
    // VPN mode components (Windows and Linux).
    //
    /// DNS resolver for VPN mode
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    dns_resolver: Option<Arc<DomainResolver>>,
    /// Route manager for VPN mode
    route_manager: Option<RouteManager>,
    /// VPN bypass manager for policy routing (Linux)
    vpn_bypass_manager: Option<VpnBypassManager>,
    /// IPv6 manager to disable/restore IPv6 for VPN and TPROXY modes (Linux)
    ipv6_manager: Option<Ipv6Manager>,
    /// Packet engine task for VPN mode
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    packet_engine_task: Option<JoinHandle<()>>,
    /// Shutdown token for VPN mode
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    shutdown_token: Option<CancellationToken>,
    /// TUN device for VPN mode (shared between manager and packet engine)
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    tun_device: Option<SharedTunDevice>,

    //
    // TPROXY mode components (Linux only).
    //
    /// TPROXY manager for iptables-based transparent proxying
    #[cfg(target_os = "linux")]
    tproxy_manager: Option<TproxyManager>,
    /// DNS resolver for TPROXY mode
    #[cfg(target_os = "linux")]
    tproxy_dns_resolver: Option<Arc<DomainResolver>>,
}

impl NodeInterceptManager {
    /// Create a new node intercept manager
    pub fn new(node_id: String, traffic_tx: mpsc::Sender<InterceptedTrafficEntry>) -> Self {
        Self {
            is_enabled: false,
            method: None,
            ca: None,
            proxy: None,
            domains: HashSet::new(),
            domain_to_agent: HashMap::new(),
            saved_proxy_settings: None,
            #[cfg(target_os = "windows")]
            wintun_manager: None,
            #[cfg(target_os = "linux")]
            tun_manager: None,
            traffic_tx,
            node_id: node_id.clone(),
            proxy_port: None,
            intercept_state: None,

            //
            // VPN mode components (Windows and Linux).
            //
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            dns_resolver: None,
            route_manager: None,
            vpn_bypass_manager: None,
            ipv6_manager: None,
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            packet_engine_task: None,
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            shutdown_token: None,
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            tun_device: None,

            //
            // TPROXY mode components (Linux only).
            //
            #[cfg(target_os = "linux")]
            tproxy_manager: None,
            #[cfg(target_os = "linux")]
            tproxy_dns_resolver: None,
        }
    }

    /// Enable interception using the supplied target list
    ///
    /// This will:
    /// 1. Collect intercept domains from each target's domains list
    /// 2. Create a root CA certificate
    /// 3. Install the root CA in the system certificate store
    /// 4. Generate leaf certificates for each domain
    /// 5. Start the HTTP proxy
    /// 6. Configure interception based on method:
    ///    - Proxy: Configure Windows system proxy
    ///    - VPN: Start wintun adapter with packet routing
    ///    - Hosts: Configure hosts file entries
    pub async fn enable(
        &mut self,
        targets: &[InterceptTargetConfig],
        method: InterceptMethod,
    ) -> Result<InterceptMethod> {
        if self.is_enabled {
            common::log_info!("Interception already enabled");
            return Ok(self.method.unwrap_or(InterceptMethod::Proxy));
        }

        common::log_info!(
            "Enabling node-level traffic interception with method: {:?}",
            method
        );

        //
        // Initialize intercept state for crash recovery.
        //

        let mut intercept_state = state::InterceptState::new(method);

        //
        // On Windows, ensure firewall rule exists to allow inbound connections.
        //

        #[cfg(windows)]
        {
            crate::utils::ensure_firewall_rule();
            intercept_state.firewall_rule_added = true;
        }

        //
        // Collect domains and URL patterns from the configured target list.
        // Targets are pushed by the service via the registration ack and
        // refreshed via NodeBroadcastMessage::InterceptTargetsUpdate.
        //
        self.domains.clear();
        self.domain_to_agent.clear();
        let mut domain_to_url_pattern = HashMap::new();

        for target in targets {
            if target.domains.is_empty() {
                continue;
            }

            common::log_info!(
                "Adding intercept domains from target '{}' ({}): {:?}",
                target.name,
                target.agent_short_name,
                target.domains
            );

            //
            // Compile the URL pattern once per target. Uses fancy-regex
            // so patterns with negative lookahead (e.g. ^(?!.*pacman).*$)
            // continue to work.
            //
            let url_pattern = target.url_pattern.as_deref().and_then(|pattern| {
                match fancy_regex::Regex::new(pattern) {
                    Ok(re) => {
                        common::log_info!("  URL filter pattern: {}", pattern);
                        Some(re)
                    }
                    Err(e) => {
                        common::log_warn!(
                            "Invalid URL pattern '{}' for target '{}': {}",
                            pattern,
                            target.name,
                            e
                        );
                        None
                    }
                }
            });

            for domain in &target.domains {
                self.domains.insert(domain.clone());
                self.domain_to_agent
                    .insert(domain.clone(), target.agent_short_name.clone());

                if let Some(ref re) = url_pattern {
                    domain_to_url_pattern.insert(domain.clone(), re.clone());
                }
            }
        }

        if self.domains.is_empty() {
            return Err(anyhow::anyhow!(
                "No intercept targets configured — add one in Settings → Intercept"
            ));
        }

        common::log_info!(
            "Intercepting {} domain(s): {:?}",
            self.domains.len(),
            self.domains
        );

        //
        // Create Certificate Authority.
        //

        let mut ca =
            CertificateAuthority::new().context("Failed to create Certificate Authority")?;

        //
        // Install root CA in system certificate store.
        //

        ca.install_root_cert()
            .context("Failed to install root CA certificate")?;

        intercept_state.cert_installed = true;

        //
        // Capture certificate details for state.
        //

        #[cfg(target_os = "windows")]
        {
            intercept_state.cert_thumbprint = ca.thumbprint().map(|s| s.to_string());
        }

        #[cfg(target_os = "linux")]
        {
            intercept_state.cert_path = ca.cert_path();
            intercept_state.linux_distro = ca.linux_distro_name().map(|s| s.to_string());
        }

        //
        // Save state after certificate is installed so we can clean up on crash.
        //

        if let Err(e) = state::save_state(&intercept_state) {
            common::log_warn!("Failed to save intercept state: {}", e);
        }

        //
        // Generate leaf certificates for all domains.
        //
        for domain in &self.domains {
            ca.generate_leaf_cert(domain)
                .context(format!("Failed to generate certificate for {}", domain))?;
        }

        let ca = Arc::new(RwLock::new(ca));

        let shared_intercept_domains = Arc::new(RwLock::new(self.domains.clone()));

        //
        // For Hosts mode, resolve real IPs BEFORE modifying hosts file.
        // This prevents the proxy from connecting back to itself.
        //
        let domain_to_real_ip = if method == InterceptMethod::Hosts {
            let dns_resolver = DomainResolver::new()
                .await
                .context("Failed to create DNS resolver for hosts mode")?;

            let mut ip_map = HashMap::new();
            for domain in &self.domains {
                match dns_resolver.resolve_domain(domain).await {
                    Ok(ips) => {
                        if let Some(ip) = ips.iter().next() {
                            common::log_info!("Pre-resolved {} -> {} for hosts mode", domain, ip);
                            ip_map.insert(domain.clone(), *ip);
                        }
                    }
                    Err(e) => {
                        common::log_warn!("Failed to pre-resolve {} for hosts mode: {}", domain, e);
                    }
                }
            }
            ip_map
        } else {
            HashMap::new()
        };

        //
        // Create proxy configuration.
        //
        let config = ProxyConfig {
            intercept_domains: shared_intercept_domains,
            domain_to_agent: self.domain_to_agent.clone(),
            domain_to_url_pattern,
            node_id: self.node_id.clone(),
            intercept_method: method,
            domain_to_real_ip,
        };

        //
        // Start the proxy.
        //
        let proxy = InterceptProxy::start(Arc::clone(&ca), config, self.traffic_tx.clone())
            .await
            .context("Failed to start intercept proxy")?;

        let proxy_port = proxy.port();
        self.proxy_port = Some(proxy_port);
        self.ca = Some(Arc::clone(&ca));
        self.proxy = Some(proxy);
        self.method = Some(method);
        self.is_enabled = true;

        //
        // Configure interception based on method.
        //

        match method {
            InterceptMethod::Proxy => {
                //
                // Configure system proxy.
                //

                let proxy_addr = format!("127.0.0.1:{}", proxy_port);
                let saved = match enable_system_proxy(&proxy_addr)
                    .context("Failed to configure system proxy")
                {
                    Ok(saved) => saved,
                    Err(e) => {
                        let _ = self.disable().await;
                        return Err(e);
                    }
                };

                intercept_state.proxy_modified = true;
                intercept_state.saved_proxy_enable = Some(saved.proxy_enable);
                intercept_state.saved_proxy_server = saved.proxy_server.clone();

                self.saved_proxy_settings = Some(saved);
                common::log_info!(
                    "Traffic interception enabled via system proxy on port {}",
                    proxy_port
                );
            }
            InterceptMethod::Vpn => {
                //
                // Start wintun adapter with packet-level routing.
                //

                if let Err(e) = self.enable_vpn_mode(proxy_port).await {
                    let _ = self.disable().await;
                    return Err(e);
                }
                common::log_info!(
                    "Traffic interception enabled via VPN mode on port {}",
                    proxy_port
                );
            }
            InterceptMethod::Hosts => {
                //
                // Add hosts file entries for all intercept domains (no VPN
                // adapter).
                //

                for domain in &self.domains {
                    if let Err(e) = hosts::add_hosts_entry(domain) {
                        common::log_error!("Failed to add hosts entry for {}: {}", domain, e);
                    }
                }

                //
                // On Linux, add iptables REDIRECT to forward 127.0.0.1:443 to proxy port.
                //

                if let Err(e) = hosts::enable_hosts_redirect(proxy_port) {
                    common::log_error!("Failed to enable hosts redirect: {}", e);
                }

                //
                // Flush DNS cache so hosts file changes take effect immediately.
                //

                hosts::flush_dns_cache();

                intercept_state.hosts_modified = true;
                common::log_info!(
                    "Traffic interception enabled via hosts file on port {}",
                    proxy_port
                );
            }
            InterceptMethod::Tproxy => {
                //
                // Start TPROXY-based interception (Linux only).
                //

                if let Err(e) = self.enable_tproxy_mode(proxy_port).await {
                    let _ = self.disable().await;
                    return Err(e);
                }

                //
                // Save TPROXY state for crash recovery.
                //

                intercept_state.tproxy_enabled = true;
                intercept_state.tproxy_port = proxy_port;

                #[cfg(target_os = "linux")]
                if let Some(ref resolver) = self.tproxy_dns_resolver {
                    let ips = resolver.get_all_intercept_ips();
                    intercept_state.tproxy_ips = ips
                        .iter()
                        .filter_map(|ip| match ip {
                            std::net::IpAddr::V4(v4) => Some(v4.to_string()),
                            std::net::IpAddr::V6(_) => None,
                        })
                        .collect();
                }

                common::log_info!(
                    "Traffic interception enabled via TPROXY on port {}",
                    proxy_port
                );
            }
        }

        //
        // Set system-wide environment variables for interception
        // Export the CA cert and set NODE_EXTRA_CA_CERTS
        // For Proxy mode, also set HTTP_PROXY and HTTPS_PROXY.
        //

        {
            let ca_guard = self.ca.as_ref().unwrap().read().await;
            let cert_pem = ca_guard.root_cert_pem();
            match env_vars::export_ca_cert(cert_pem) {
                Ok(cert_path) => {
                    let proxy_addr = if method == InterceptMethod::Proxy {
                        Some(format!("127.0.0.1:{}", proxy_port))
                    } else {
                        None
                    };
                    if let Err(e) =
                        env_vars::set_intercept_env_vars(&cert_path, proxy_addr.as_deref())
                    {
                        common::log_warn!("Failed to set intercept environment variables: {}", e);
                    } else {
                        intercept_state.env_vars_set = true;
                    }
                }
                Err(e) => {
                    common::log_warn!(
                        "Failed to export CA certificate for NODE_EXTRA_CA_CERTS: {}",
                        e
                    );
                }
            }
        }

        //
        // Save final state to disk.
        //

        if let Err(e) = state::save_state(&intercept_state) {
            common::log_warn!("Failed to save intercept state: {}", e);
        }
        self.intercept_state = Some(intercept_state);

        Ok(method)
    }

    /// Disable interception and clean up
    ///
    /// This will:
    /// 1. Clean up based on method:
    ///    - Proxy: Restore original system proxy settings
    ///    - VPN: Stop packet engine, remove routes, stop wintun adapter
    ///    - Hosts: Remove hosts file entries
    /// 2. Stop the proxy server
    /// 3. Uninstall the root CA certificate
    pub async fn disable(&mut self) -> Result<()> {
        if !self.is_enabled {
            common::log_info!("Interception not enabled");
            return Ok(());
        }

        let method = self.method.unwrap_or(InterceptMethod::Proxy);
        common::log_info!("Disabling traffic interception (method: {:?})", method);

        //
        // Clean up based on method.
        //

        match method {
            InterceptMethod::Proxy => self.cleanup_proxy_sync(),
            InterceptMethod::Vpn => self.disable_vpn_mode().await,
            InterceptMethod::Hosts => self.cleanup_hosts_sync(),
            InterceptMethod::Tproxy => self.disable_tproxy_mode().await,
        }

        //
        // Stop proxy.
        //
        if let Some(mut proxy) = self.proxy.take() {
            proxy.stop().await;
        }
        self.proxy_port = None;

        //
        // Uninstall root CA.
        //
        if let Some(ca) = &self.ca {
            let ca_guard = ca.read().await;
            if let Err(e) = ca_guard.uninstall_root_cert() {
                common::log_warn!("Failed to uninstall root CA certificate: {}", e);
            }
        }
        self.ca = None;

        self.is_enabled = false;
        self.method = None;
        self.domains.clear();
        self.domain_to_agent.clear();

        //
        // Remove intercept environment variables.
        //
        if let Err(e) = env_vars::remove_intercept_env_vars() {
            common::log_warn!("Failed to remove intercept environment variables: {}", e);
        }

        //
        // Remove Windows firewall rule.
        //

        #[cfg(windows)]
        crate::utils::remove_firewall_rule();

        //
        // Remove state file since cleanup is complete.
        //

        if let Err(e) = state::remove_state() {
            common::log_warn!("Failed to remove intercept state file: {}", e);
        }
        self.intercept_state = None;

        common::log_info!("Traffic interception disabled");

        Ok(())
    }

    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    pub fn method(&self) -> Option<InterceptMethod> {
        self.method
    }

    pub fn intercepted_domains(&self) -> Vec<String> {
        self.domains.iter().cloned().collect()
    }
}

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
}

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
