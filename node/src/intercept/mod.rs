pub mod agent_discovery;
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
pub mod tun_device;
#[cfg(target_os = "linux")]
pub mod tun_linux;
pub mod wintun;

pub use agent_discovery::AgentDiscoveryManager;
pub use certificate::CertificateAuthority;
pub use proxy::{InterceptProxy, ObservedConnection, ProxyConfig};
pub use state::cleanup_stale_state;
pub use system_proxy::{disable_system_proxy, enable_system_proxy, SavedProxySettings};
#[cfg(target_os = "linux")]
pub use tproxy::TproxyManager;
#[cfg(target_os = "linux")]
pub use tun_linux::LinuxTunManager;
#[cfg(target_os = "windows")]
pub use wintun::WintunManager;

use anyhow::{Context, Result};
use common::{DiscoveredLlmEndpoint, InterceptMethod, InterceptedTrafficEntry};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use tokio::task::JoinHandle;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use tokio_util::sync::CancellationToken;
#[cfg(any(target_os = "windows", target_os = "linux"))]

use crate::agent_connectors::Agent;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use dns_resolver::DomainResolver;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use packet_engine::PacketEngine;
use routing::{Ipv6Manager, RouteManager, VpnBypassManager};
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
    traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
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

    //
    // Agent discovery.
    //
    /// Agent discovery manager for detecting LLM endpoints
    agent_discovery: Arc<RwLock<AgentDiscoveryManager>>,
    /// Discovery observer task handle
    discovery_observer_task: Option<tokio::task::JoinHandle<()>>,
    /// Dynamic intercept task handle (for adding domains at runtime)
    dynamic_intercept_task: Option<tokio::task::JoinHandle<()>>,
    /// Shared intercept domains (for dynamic updates)
    shared_intercept_domains: Option<Arc<RwLock<HashSet<String>>>>,
    /// Channel for requesting domain interception (stored for enable_agent_discovery)
    intercept_domain_tx: Option<mpsc::UnboundedSender<String>>,
}

impl NodeInterceptManager {
    /// Create a new node intercept manager
    pub fn new(
        node_id: String,
        traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
        discovery_tx: mpsc::UnboundedSender<DiscoveredLlmEndpoint>,
    ) -> Self {
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

            //
            // Agent discovery.
            //
            agent_discovery: Arc::new(RwLock::new(AgentDiscoveryManager::new(node_id, discovery_tx))),
            discovery_observer_task: None,
            dynamic_intercept_task: None,
            shared_intercept_domains: None,
            intercept_domain_tx: None,
        }
    }

    /// Enable interception for all agents that support it
    ///
    /// This will:
    /// 1. Collect intercept domains from all agents
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
        agents: &[Arc<dyn Agent>],
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
        // Collect domains and URL patterns from all agents that support
        // interception.
        //
        self.domains.clear();
        self.domain_to_agent.clear();
        let mut domain_to_url_pattern = HashMap::new();

        for agent in agents {
            //
            // Check if agent supports interception via the AgentIntercept
            // trait.
            //
            if let Some(intercept) = agent.as_intercept() {
                let domains = intercept.intercept_domains();
                if !domains.is_empty() {
                    common::log_info!(
                        "Adding intercept domains from {}: {:?}",
                        agent.short_name(),
                        domains
                    );

                    //
                    // Compile URL pattern once for all domains of this agent
                    // Uses fancy-regex to support negative lookahead, e.g.,
                    // ^(?!.*pacman).*$.
                    //
                    let url_pattern = intercept.intercept_url_pattern().and_then(|pattern| {
                        match fancy_regex::Regex::new(pattern) {
                            Ok(re) => {
                                common::log_info!("  URL filter pattern: {}", pattern);
                                Some(re)
                            }
                            Err(e) => {
                                common::log_warn!(
                                    "Invalid URL pattern '{}' for {}: {}",
                                    pattern,
                                    agent.short_name(),
                                    e
                                );
                                None
                            }
                        }
                    });

                    for domain in domains {
                        self.domains.insert(domain.to_string());
                        self.domain_to_agent
                            .insert(domain.to_string(), agent.short_name().to_string());

                        if let Some(ref re) = url_pattern {
                            domain_to_url_pattern.insert(domain.to_string(), re.clone());
                        }
                    }
                }
            }
        }

        if self.domains.is_empty() {
            return Err(anyhow::anyhow!(
                "No agents have intercept domains configured"
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

        //
        // Create shared intercept domains (for dynamic updates).
        //
        let shared_intercept_domains = Arc::new(RwLock::new(self.domains.clone()));
        self.shared_intercept_domains = Some(Arc::clone(&shared_intercept_domains));

        //
        // Create channel for observed connections (for agent discovery).
        //
        let (connection_observer_tx, mut connection_observer_rx) =
            mpsc::unbounded_channel::<ObservedConnection>();

        //
        // Create channel for dynamic domain interception requests.
        //
        let (intercept_domain_tx, mut intercept_domain_rx) =
            mpsc::unbounded_channel::<String>();
        self.intercept_domain_tx = Some(intercept_domain_tx.clone());

        //
        // Spawn dynamic intercept task to handle domain additions at runtime.
        //
        let shared_domains_for_task = Arc::clone(&shared_intercept_domains);
        let ca_for_task = Arc::clone(&ca);
        let dynamic_task = tokio::spawn(async move {
            while let Some(domain) = intercept_domain_rx.recv().await {
                common::log_info!("Adding domain {} to intercept list dynamically", domain);

                //
                // Generate certificate for the new domain.
                //
                {
                    let mut ca_guard = ca_for_task.write().await;
                    if let Err(e) = ca_guard.generate_leaf_cert(&domain) {
                        common::log_error!("Failed to generate certificate for {}: {}", domain, e);
                        continue;
                    }
                }

                //
                // Add domain to intercept list.
                //
                {
                    let mut domains = shared_domains_for_task.write().await;
                    domains.insert(domain.clone());
                    common::log_info!("Domain {} added to intercept list (total: {})", domain, domains.len());
                }
            }
        });
        self.dynamic_intercept_task = Some(dynamic_task);

        //
        // Spawn discovery observer task.
        //
        let discovery_manager = Arc::clone(&self.agent_discovery);
        let discovery_task = tokio::spawn(async move {
            while let Some(conn) = connection_observer_rx.recv().await {
                let manager = discovery_manager.read().await;
                if manager.is_enabled() {
                    common::log_debug!(
                        "Discovery observer received: domain={:?}, has_api_key={}",
                        conn.domain,
                        conn.api_key.is_some()
                    );

                    //
                    // If we have an API key, record it for this domain.
                    //
                    if let (Some(domain), Some(api_key)) = (&conn.domain, &conn.api_key) {
                        common::log_debug!(
                            "Recording API key for domain {} (key length: {})",
                            domain,
                            api_key.len()
                        );
                        manager.record_api_key(domain, api_key.clone()).await;
                    }

                    manager
                        .probe_endpoint(conn.ip, conn.port, conn.domain, conn.is_https)
                        .await;
                }
            }
        });
        self.discovery_observer_task = Some(discovery_task);

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
            connection_observer_tx: Some(connection_observer_tx),
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

        //
        // Configure interception based on method.
        //

        match method {
            InterceptMethod::Proxy => {
                //
                // Configure system proxy.
                //

                let proxy_addr = format!("127.0.0.1:{}", proxy_port);
                let saved =
                    enable_system_proxy(&proxy_addr).context("Failed to configure system proxy")?;

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

                self.enable_vpn_mode(proxy_port).await?;
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

                self.enable_tproxy_mode(proxy_port).await?;

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

        self.ca = Some(ca);
        self.proxy = Some(proxy);
        self.method = Some(method);
        self.is_enabled = true;

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
                    if let Err(e) = env_vars::set_intercept_env_vars(&cert_path, proxy_addr.as_deref()) {
                        common::log_warn!("Failed to set intercept environment variables: {}", e);
                    } else {
                        intercept_state.env_vars_set = true;
                    }
                }
                Err(e) => {
                    common::log_warn!("Failed to export CA certificate for NODE_EXTRA_CA_CERTS: {}", e);
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

    /// Enable VPN mode with packet-level routing (Windows).
    ///
    /// This sets up:
    /// 1. Wintun adapter with packet session
    /// 2. DNS resolution for intercept domains
    /// 3. Routes for resolved IPs through the TUN adapter
    /// 4. Packet engine for NAT and forwarding
    #[cfg(target_os = "windows")]
    async fn enable_vpn_mode(&mut self, proxy_port: u16) -> Result<()> {
        use routing::TUN_INTERFACE_NAME;
        use tun_device::WintunDevice;

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
    async fn enable_vpn_mode(&mut self, proxy_port: u16) -> Result<()> {
        use tun_linux::ADAPTER_NAME;

        common::log_info!("Setting up VPN mode with packet routing (Linux)");

        //
        // 0. Disable IPv6 to avoid routing issues with TUN device.
        //    IPv6 traffic doesn't go through our packet engine properly.
        //
        let mut ipv6_manager = Ipv6Manager::new();
        ipv6_manager
            .disable()
            .context("Failed to disable IPv6")?;

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
    async fn enable_vpn_mode(&mut self, _proxy_port: u16) -> Result<()> {
        Err(anyhow::anyhow!("VPN mode is only supported on Windows and Linux"))
    }

    /// Enable TPROXY mode with iptables-based packet interception (Linux).
    ///
    /// This sets up:
    /// 1. iptables TPROXY rules to redirect traffic to proxy
    /// 2. Policy routing for marked packets
    /// 3. SO_ORIGINAL_DST used by proxy to get real destination
    #[cfg(target_os = "linux")]
    async fn enable_tproxy_mode(&mut self, proxy_port: u16) -> Result<()> {
        common::log_info!("Setting up TPROXY intercept mode (Linux)");

        //
        // 0. Disable IPv6 to avoid routing issues.
        //    TPROXY rules only handle IPv4 currently.
        //
        let mut ipv6_manager = Ipv6Manager::new();
        ipv6_manager
            .disable()
            .context("Failed to disable IPv6")?;
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
    async fn enable_tproxy_mode(&mut self, _proxy_port: u16) -> Result<()> {
        Err(anyhow::anyhow!("TPROXY mode is only supported on Linux"))
    }

    /// Disable TPROXY mode and clean up components (Linux).
    #[cfg(target_os = "linux")]
    async fn disable_tproxy_mode(&mut self) {
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
    async fn disable_tproxy_mode(&mut self) {
        // No-op on non-Linux
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

    /// Disable VPN mode and clean up components (Windows/Linux).
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    async fn disable_vpn_mode(&mut self) {
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
    async fn disable_vpn_mode(&mut self) {
        if let Some(mut route_manager) = self.route_manager.take() {
            let _ = route_manager.remove_all_routes();
        }
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

    #[allow(dead_code)]
    pub fn proxy_port(&self) -> Option<u16> {
        self.proxy_port
    }

    //
    // Agent Discovery methods.
    //

    /// Enable agent discovery (requires intercept to be enabled first)
    pub async fn enable_agent_discovery(&mut self) -> Result<()> {
        if !self.is_enabled {
            return Err(anyhow::anyhow!(
                "Intercept must be enabled before agent discovery"
            ));
        }

        let ca = self.ca.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Certificate Authority not available")
        })?;

        let intercept_tx = self.intercept_domain_tx.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Intercept domain channel not available")
        })?;

        self.agent_discovery
            .write()
            .await
            .enable(Arc::clone(ca), intercept_tx.clone());

        Ok(())
    }

    /// Disable agent discovery
    pub async fn disable_agent_discovery(&mut self) {
        self.agent_discovery.write().await.disable();
    }

    /// Check if agent discovery is enabled
    pub async fn is_agent_discovery_enabled(&self) -> bool {
        self.agent_discovery.read().await.is_enabled()
    }

    /// Get the count of discovered endpoints
    pub async fn discovered_endpoints_count(&self) -> usize {
        self.agent_discovery.read().await.discovered_count().await
    }

    /// Get a reference to the agent discovery manager (Arc-wrapped)
    #[allow(dead_code)]
    pub fn agent_discovery(&self) -> &Arc<RwLock<AgentDiscoveryManager>> {
        &self.agent_discovery
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
