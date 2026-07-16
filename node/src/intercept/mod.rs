pub mod certificate;
pub mod dns_resolver;
pub mod env_vars;
pub mod hosts;
pub mod lifecycle;
pub mod listen_policy;
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
pub use proxy::{AgentCaptureRule, DomainCaptureConfig, InterceptProxy, ProxyConfig};
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
use lifecycle::{
    after_rollback, begin_enable, enable_short_circuit, finish_clean, finish_enable,
    may_run_sync_vpn_or_stale_cleanup, needs_cleanup, status_cleanup_required,
    status_shows_cleanup_required,
    should_abort_enable, InterceptLifecycle,
};
use routing::{Ipv6Manager, RouteManager, VpnBypassManager};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio::task::JoinHandle;
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
    /// Explicit enable lifecycle (Enabling requires cleanup even if not Enabled).
    lifecycle: InterceptLifecycle,
    /// Cancellation for the in-flight enable operation (reset/shutdown).
    operation_cancel: Option<CancellationToken>,
    /// Current interception method
    method: Option<InterceptMethod>,
    /// Certificate Authority for generating TLS certificates
    ca: Option<Arc<RwLock<CertificateAuthority>>>,
    /// The running proxy server
    proxy: Option<InterceptProxy>,
    /// Domains being intercepted
    domains: HashSet<String>,
    /// Per-domain agent candidates and URL filters.
    domain_capture_configs: HashMap<String, DomainCaptureConfig>,
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
    /// Shutdown token for VPN mode packet engine (not enable cancel).
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
            lifecycle: InterceptLifecycle::Disabled,
            operation_cancel: None,
            method: None,
            ca: None,
            proxy: None,
            domains: HashSet::new(),
            domain_capture_configs: HashMap::new(),
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
        //
        // Lifecycle is authoritative over is_enabled (CleanupRequired may
        // coexist with a stale is_enabled after post-bind cleanup failure).
        //
        match enable_short_circuit(self.lifecycle, self.is_enabled) {
            Ok(true) => {
                common::log_info!("Interception already enabled");
                return Ok(self.method.unwrap_or(InterceptMethod::Proxy));
            }
            Ok(false) => {}
            Err(e) => anyhow::bail!("{}", e),
        }
        //
        // Orphaned resources/recovery on Disabled also block re-enable.
        //
        if matches!(self.lifecycle, InterceptLifecycle::Disabled)
            && (self.has_vpn_resources()
                || self.ca.is_some()
                || state::load_state().ok().flatten().is_some())
        {
            anyhow::bail!(
                "intercept cleanup required before re-enable (stale resources or recovery state)"
            );
        }

        common::log_info!(
            "Enabling node-level traffic interception with method: {:?}",
            method
        );

        //
        // PREFLIGHT before lifecycle → Enabling: validate targets and construct
        // CA so invalid URL patterns / CA construct never leave Enabling stuck.
        //
        let mut pending_domains = HashSet::new();
        let mut pending_capture: HashMap<String, DomainCaptureConfig> = HashMap::new();

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

            let url_pattern = match target.url_pattern.as_deref() {
                Some(pattern) => {
                    let re = fancy_regex::Regex::new(pattern).with_context(|| {
                        format!(
                            "Invalid URL pattern '{}' for intercept target '{}'",
                            pattern, target.name
                        )
                    })?;
                    common::log_info!("  URL filter pattern: {}", pattern);
                    Some(re)
                }
                None => None,
            };

            for domain in &target.domains {
                let domain = domain
                    .trim()
                    .trim_start_matches("*.")
                    .trim_end_matches('.')
                    .to_ascii_lowercase();
                if domain.is_empty() {
                    continue;
                }
                pending_domains.insert(domain.clone());
                let capture = pending_capture.entry(domain).or_default();
                if !capture
                    .agent_rules
                    .iter()
                    .any(|rule| rule.agent_short_name == target.agent_short_name)
                {
                    capture.agent_rules.push(AgentCaptureRule {
                        agent_short_name: target.agent_short_name.clone(),
                        url_pattern: url_pattern.clone(),
                    });
                }
            }
        }

        if pending_domains.is_empty() {
            return Err(anyhow::anyhow!(
                "No intercept targets configured — add one in Settings → Intercept"
            ));
        }

        let mut ca =
            CertificateAuthority::new().context("Failed to create Certificate Authority")?;
        ca.prepare_root_cert_install()
            .context("Failed to prepare root CA recovery metadata")?;

        //
        // Commit Enabling only after preflight succeeds.
        //
        self.lifecycle = begin_enable(self.lifecycle)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        self.domains = pending_domains;
        self.domain_capture_configs = pending_capture;

        let mut intercept_state = state::InterceptState::new(method);

        common::log_info!(
            "Intercepting {} domain(s): {:?}",
            self.domains.len(),
            self.domains
        );

        #[cfg(target_os = "windows")]
        {
            intercept_state.cert_thumbprint = ca.thumbprint().map(str::to_string);
            intercept_state.cert_installed = intercept_state.cert_thumbprint.is_some();
            intercept_state.firewall_rule_added = false;
        }
        #[cfg(target_os = "linux")]
        {
            intercept_state.cert_path = ca.cert_path();
            intercept_state.linux_distro = ca.linux_distro_name().map(str::to_string);
            intercept_state.cert_installed = intercept_state.cert_path.is_some();
        }
        if let Err(error) = state::save_state(&intercept_state)
            .context("Failed to persist recovery state before root CA installation")
        {
            let cleanup = state::remove_state()
                .context("Failed to remove incomplete intercept recovery state");
            self.lifecycle = after_rollback(cleanup.is_ok());
            return Err(Self::combine_enable_failure(error, cleanup));
        }

        //
        // Install root CA in system certificate store.
        //

        if let Err(e) = ca.install_root_cert() {
            let rollback = Self::rollback_cert_install(&ca);
            self.lifecycle = after_rollback(rollback.is_ok());
            return Err(Self::combine_enable_failure(
                e.context("Failed to install root CA certificate"),
                rollback,
            ));
        }

        //
        // Save state after certificate is installed so we can clean up on crash.
        //

        if let Err(e) = state::save_state(&intercept_state) {
            let cause = e.context("Failed to persist intercept recovery state after CA install");
            let rollback = Self::rollback_cert_install(&ca);
            self.lifecycle = after_rollback(rollback.is_ok());
            return Err(Self::combine_enable_failure(cause, rollback));
        }

        //
        // Everything after the CA is installed must roll the CA back on
        // failure — otherwise a partial enable leaves a trusted root and
        // a state file until the next process start.
        //
        for domain in &self.domains {
            if let Err(e) = ca.generate_leaf_cert(domain) {
                let cause = e.context(format!("Failed to generate certificate for {}", domain));
                let rollback = Self::rollback_cert_install(&ca);
                self.lifecycle = after_rollback(rollback.is_ok());
                return Err(Self::combine_enable_failure(cause, rollback));
            }
        }

        let ca = Arc::new(RwLock::new(ca));
        //
        // Attach CA early so cancel/force_cleanup can uninstall if enable is
        // aborted before is_enabled is set. Do not clear self.ca after a
        // failed rollback (CleanupRequired keeps the handle).
        //
        self.ca = Some(Arc::clone(&ca));
        if let Err(e) = self.check_enable_cancelled() {
            return Err(self.fail_enable_with_rollback(&ca, e).await);
        }

        let shared_intercept_domains = Arc::new(RwLock::new(self.domains.clone()));

        //
        // For Hosts mode, resolve real IPs BEFORE modifying hosts file.
        // This prevents the proxy from connecting back to itself.
        //
        let domain_to_real_ip = if method == InterceptMethod::Hosts {
            let dns_resolver = match self
                .race_cancel(DomainResolver::new())
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let cause = e.context("Failed to create DNS resolver for hosts mode");
                    return Err(self.fail_enable_with_rollback(&ca, cause).await);
                }
            };

            let resolved = match self
                .race_cancel(dns_resolver.resolve_domains_best_effort(&self.domains))
                .await
            {
                Ok(resolved) => resolved,
                Err(e) => {
                    let cause = e.context("Failed to pre-resolve any Hosts intercept target");
                    return Err(self.fail_enable_with_rollback(&ca, cause).await);
                }
            };

            let mut ip_map = HashMap::with_capacity(resolved.len());
            for (domain, ips) in resolved {
                let Some(ip) = ips
                    .iter()
                    .find(|ip| ip.is_ipv4())
                    .or_else(|| ips.iter().next())
                    .copied()
                else {
                    //
                    // Empty address set for a required domain: fail the enable
                    // with rollback rather than panicking the node mid-enable
                    // while holding the manager lock.
                    //
                    let cause = anyhow::anyhow!(
                        "required DNS resolution for {} returned an empty address set",
                        domain
                    );
                    return Err(self.fail_enable_with_rollback(&ca, cause).await);
                };
                common::log_info!("Pre-resolved {} -> {} for hosts mode", domain, ip);
                ip_map.insert(domain, ip);
            }
            ip_map
        } else {
            HashMap::new()
        };

        let config = ProxyConfig {
            intercept_domains: shared_intercept_domains,
            domain_capture_configs: self.domain_capture_configs.clone(),
            node_id: self.node_id.clone(),
            intercept_method: method,
            domain_to_real_ip,
        };

        //
        // Phase order comes from listen_policy::plan_for(method). VPN must
        // bring the TUN up and assign TUN_IP before binding the proxy to
        // that address; other methods bind first then apply method routing.
        //
        let listen_plan = listen_policy::plan_for(method);
        if let Err(e) = self.check_enable_cancelled() {
            return Err(self.fail_enable_with_rollback(&ca, e).await);
        }
        if listen_policy::requires_tun_before_bind(&listen_plan) {
            //
            // prepare_vpn_tun checks cancel between privileged steps; also
            // gate before/after the whole phase.
            //
            if let Err(e) = self.check_enable_cancelled() {
                return Err(self.fail_enable_with_rollback(&ca, e).await);
            }
            if let Err(e) = self.prepare_vpn_tun().await {
                let cause = e.context("Failed to prepare VPN TUN before proxy bind");
                return Err(self.fail_enable_with_rollback(&ca, cause).await);
            }
            if let Err(e) = self.check_enable_cancelled() {
                return Err(self.fail_enable_with_rollback(&ca, e).await);
            }
        }

        if let Err(e) = self.check_enable_cancelled() {
            return Err(self.fail_enable_with_rollback(&ca, e).await);
        }

        let traffic_sink = proxy::TrafficSink::new(self.traffic_tx.clone());
        let proxy = match self
            .race_cancel(InterceptProxy::start(Arc::clone(&ca), config, traffic_sink))
            .await
        {
            Ok(p) => p,
            Err(e) => {
                let cause = e.context("Failed to start intercept proxy");
                return Err(self.fail_enable_with_rollback(&ca, cause).await);
            }
        };

        let proxy_port = proxy.port();
        self.proxy_port = Some(proxy_port);
        self.proxy = Some(proxy);
        self.method = Some(method);
        self.is_enabled = true;
        self.lifecycle = finish_enable(self.lifecycle).unwrap_or(InterceptLifecycle::Enabled);

        //
        // Post-bind setup remains cancellable (reset/shutdown).
        //
        if let Err(e) = self.check_enable_cancelled() {
            let cleanup = self.disable_after_partial_enable().await;
            return Err(Self::combine_enable_failure(e, cleanup));
        }

        //
        // Configure interception based on method.
        //

        match method {
            InterceptMethod::Proxy => {
                //
                // Configure system proxy. Sync OS calls cannot be interrupted
                // mid-command; check cancel between each phase and retain
                // recovery ownership if later cleanup is incomplete.
                //

                if let Err(e) = self.check_enable_cancelled() {
                    let cleanup = self.disable_after_partial_enable().await;
                    return Err(Self::combine_enable_failure(e, cleanup));
                }
                let proxy_addr = format!("127.0.0.1:{}", proxy_port);
                #[cfg(target_os = "windows")]
                {
                    let saved = match system_proxy::get_proxy_settings()
                        .context("Failed to read system proxy settings for recovery")
                    {
                        Ok(saved) => saved,
                        Err(e) => {
                            let cleanup = self.disable_after_partial_enable().await;
                            return Err(Self::combine_enable_failure(e, cleanup));
                        }
                    };
                    if let Err(e) = self.check_enable_cancelled() {
                        let cleanup = self.disable_after_partial_enable().await;
                        return Err(Self::combine_enable_failure(e, cleanup));
                    }
                    intercept_state.proxy_modified = true;
                    intercept_state.saved_proxy_enable = Some(saved.proxy_enable);
                    intercept_state.saved_proxy_server = saved.proxy_server;
                    if let Err(e) = state::save_state(&intercept_state) {
                        let cleanup = self.disable_after_partial_enable().await;
                        return Err(Self::combine_enable_failure(
                            e.context(
                                "Failed to persist recovery state before system proxy change",
                            ),
                            cleanup,
                        ));
                    }
                }
                if let Err(e) = self.check_enable_cancelled() {
                    let cleanup = self.disable_after_partial_enable().await;
                    return Err(Self::combine_enable_failure(e, cleanup));
                }
                let saved = match enable_system_proxy(&proxy_addr)
                    .context("Failed to configure system proxy")
                {
                    Ok(saved) => saved,
                    Err(e) => {
                        let cleanup = self.disable_after_partial_enable().await;
                        return Err(Self::combine_enable_failure(e, cleanup));
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
                // TUN was prepared before bind; finish routes + packet engine.
                //

                if let Err(e) = self.check_enable_cancelled() {
                    let cleanup = self.disable_after_partial_enable().await;
                    return Err(Self::combine_enable_failure(e, cleanup));
                }
                if let Err(e) = self.finish_vpn_mode(proxy_port).await {
                    let cleanup = self.disable_after_partial_enable().await;
                    return Err(Self::combine_enable_failure(e, cleanup));
                }
                if let Err(e) = self.check_enable_cancelled() {
                    let cleanup = self.disable_after_partial_enable().await;
                    return Err(Self::combine_enable_failure(e, cleanup));
                }
                #[cfg(target_os = "linux")]
                if let Some(ref ipv6) = self.ipv6_manager {
                    intercept_state.ipv6_original_value =
                        ipv6.original_value().map(str::to_string);
                }
                if let Some(ref routes) = self.route_manager {
                    intercept_state.vpn_interface = Some(routes.interface_name().to_string());
                    intercept_state.vpn_routes = routes
                        .added_routes()
                        .iter()
                        .map(ToString::to_string)
                        .collect();
                }
                intercept_state.vpn_bypass_enabled = self
                    .vpn_bypass_manager
                    .as_ref()
                    .is_some_and(VpnBypassManager::is_active);
                //
                // Windows: install a port-scoped inbound rule only for VPN
                // (listener is on the TUN address and needs firewall for
                // the synthetic client path). Persist ownership write-ahead
                // before netsh so crash recovery can remove the rule.
                //
                #[cfg(windows)]
                {
                    use crate::utils::FirewallOwnershipState;

                    let rule_name = crate::utils::intercept_firewall_rule_name(proxy_port);
                    let ownership = FirewallOwnershipState::write_ahead(rule_name.clone(), proxy_port);
                    intercept_state.firewall_rule_added = ownership.added;
                    intercept_state.firewall_rule_name = ownership.name.clone();
                    intercept_state.firewall_rule_port = ownership.port;
                    if let Err(e) = state::save_state(&intercept_state) {
                        //
                        // No netsh mutation yet — clear in-memory ownership only.
                        //
                        intercept_state.firewall_rule_added = false;
                        intercept_state.firewall_rule_name = None;
                        intercept_state.firewall_rule_port = None;
                        let cleanup = self.disable_after_partial_enable().await;
                        return Err(Self::combine_enable_failure(
                            e.context(
                                "Failed to persist firewall ownership before rule creation",
                            ),
                            cleanup,
                        ));
                    }
                    if !crate::utils::ensure_firewall_rule_for_port(proxy_port) {
                        let remove_ok = crate::utils::remove_firewall_rule_named(&rule_name);
                        let next = ownership.after_create_failed(remove_ok);
                        intercept_state.firewall_rule_added = next.added;
                        intercept_state.firewall_rule_name = next.name.clone();
                        intercept_state.firewall_rule_port = next.port;
                        //
                        // Persist cleared ownership only after confirmed remove.
                        // If that save fails, reconstruct write-ahead so recovery
                        // can retry. If remove failed, leave write-ahead on disk.
                        //
                        if remove_ok {
                            if let Err(save_err) = state::save_state(&intercept_state) {
                                intercept_state.firewall_rule_added = true;
                                intercept_state.firewall_rule_name = Some(rule_name.clone());
                                intercept_state.firewall_rule_port = Some(proxy_port);
                                let _ = state::save_state(&intercept_state);
                                let cleanup = self.disable_after_partial_enable().await;
                                return Err(Self::combine_enable_failure(
                                    anyhow::anyhow!(
                                        "Failed to install the Praxis VPN firewall rule for port {}; \
                                         rule was removed but clearing ownership failed ({})",
                                        proxy_port,
                                        save_err
                                    ),
                                    cleanup,
                                ));
                            }
                        }
                        let cleanup = self.disable_after_partial_enable().await;
                        let cause = if remove_ok {
                            anyhow::anyhow!(
                                "Failed to install the Praxis VPN firewall rule for port {}",
                                proxy_port
                            )
                        } else {
                            anyhow::anyhow!(
                                "Failed to install the Praxis VPN firewall rule for port {}; \
                                 owned rule removal also failed and ownership was retained for recovery",
                                proxy_port
                            )
                        };
                        return Err(Self::combine_enable_failure(cause, cleanup));
                    }
                }
                common::log_info!(
                    "Traffic interception enabled via VPN mode on port {}",
                    proxy_port
                );
            }
            InterceptMethod::Hosts => {
                //
                // Add hosts file entries for all intercept domains. The proxy
                // already binds 127.0.0.1:443, so no iptables REDIRECT is needed.
                //

                intercept_state.hosts_modified = true;
                intercept_state.hosts_proxy_port = proxy_port;
                //
                // New enables never install the legacy iptables REDIRECT.
                //
                intercept_state.hosts_redirect_added = Some(false);
                if let Err(e) = state::save_state(&intercept_state) {
                    let cleanup = self.disable_after_partial_enable().await;
                    return Err(Self::combine_enable_failure(
                        e.context("Failed to persist recovery state before Hosts modification"),
                        cleanup,
                    ));
                }

                let domains: Vec<String> = self.domains.iter().cloned().collect();
                for domain in &domains {
                    if let Err(e) = self.check_enable_cancelled() {
                        let cleanup = self.disable_after_partial_enable().await;
                        return Err(Self::combine_enable_failure(e, cleanup));
                    }
                    if let Err(e) = hosts::add_hosts_entry(domain) {
                        let cleanup = self.disable_after_partial_enable().await;
                        return Err(Self::combine_enable_failure(
                            e.context(format!("Failed to add hosts entry for {}", domain)),
                            cleanup,
                        ));
                    }
                }

                //
                // Flush DNS cache so hosts file changes take effect immediately.
                //

                hosts::flush_dns_cache();

                common::log_info!(
                    "Traffic interception enabled via hosts file on port {}",
                    proxy_port
                );
            }
            InterceptMethod::Tproxy => {
                //
                // Start TPROXY-based interception (Linux only).
                //

                if let Err(e) = self.check_enable_cancelled() {
                    let cleanup = self.disable_after_partial_enable().await;
                    return Err(Self::combine_enable_failure(e, cleanup));
                }
                if let Err(e) = self.enable_tproxy_mode(proxy_port).await {
                    let cleanup = self.disable_after_partial_enable().await;
                    return Err(Self::combine_enable_failure(e, cleanup));
                }
                if let Err(e) = self.check_enable_cancelled() {
                    let cleanup = self.disable_after_partial_enable().await;
                    return Err(Self::combine_enable_failure(e, cleanup));
                }

                //
                // Save TPROXY state for crash recovery.
                //

                intercept_state.tproxy_enabled = true;
                intercept_state.tproxy_port = proxy_port;
                intercept_state.tproxy_rules_tagged = true;

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
                #[cfg(target_os = "linux")]
                if let Some(ref ipv6) = self.ipv6_manager {
                    intercept_state.ipv6_original_value =
                        ipv6.original_value().map(str::to_string);
                }
                #[cfg(target_os = "linux")]
                if let Some(ref tproxy) = self.tproxy_manager {
                    intercept_state.route_localnet_original_value =
                        tproxy.route_localnet_original().map(str::to_string);
                }

                common::log_info!(
                    "Traffic interception enabled via TPROXY on port {}",
                    proxy_port
                );
            }
        }

        if let Err(e) = state::save_state(&intercept_state) {
            let cleanup = self.disable_after_partial_enable().await;
            return Err(Self::combine_enable_failure(
                e.context("Failed to persist intercept recovery state after system setup"),
                cleanup,
            ));
        }

        //
        // Set system-wide environment variables for interception
        // Export the CA cert and set NODE_EXTRA_CA_CERTS
        // For Proxy mode, also set HTTP_PROXY and HTTPS_PROXY.
        //

        if let Err(e) = self.check_enable_cancelled() {
            let cleanup = self.disable_after_partial_enable().await;
            return Err(Self::combine_enable_failure(e, cleanup));
        }
        {
            let ca_guard = self.ca.as_ref().unwrap().read().await;
            let cert_pem = ca_guard.root_cert_pem();
            let cert_path = match env_vars::export_ca_cert(cert_pem) {
                Ok(path) => path,
                Err(e) => {
                    drop(ca_guard);
                    let cleanup = self.disable_after_partial_enable().await;
                    return Err(Self::combine_enable_failure(
                        e.context("Failed to export intercept CA certificate"),
                        cleanup,
                    ));
                }
            };
            if let Err(e) = self.check_enable_cancelled() {
                drop(ca_guard);
                let cleanup = self.disable_after_partial_enable().await;
                return Err(Self::combine_enable_failure(e, cleanup));
            }
            let proxy_addr = if method == InterceptMethod::Proxy {
                Some(format!("127.0.0.1:{}", proxy_port))
            } else {
                None
            };
            if let Err(e) = env_vars::set_intercept_env_vars(&cert_path, proxy_addr.as_deref()) {
                drop(ca_guard);
                let cleanup = self.disable_after_partial_enable().await;
                return Err(Self::combine_enable_failure(
                    e.context("Failed to set intercept environment variables"),
                    cleanup,
                ));
            }
            intercept_state.env_vars_set = true;
        }

        //
        // Save final state to disk.
        //

        if let Err(e) = self.check_enable_cancelled() {
            let cleanup = self.disable_after_partial_enable().await;
            return Err(Self::combine_enable_failure(e, cleanup));
        }
        if let Err(e) = state::save_state(&intercept_state) {
            let cleanup = self.disable_after_partial_enable().await;
            return Err(Self::combine_enable_failure(
                e.context("Failed to persist final intercept recovery state"),
                cleanup,
            ));
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

        let mut failures = Vec::new();
        let method_cleanup = match method {
            InterceptMethod::Proxy => self.cleanup_proxy_sync(),
            InterceptMethod::Vpn => self.disable_vpn_mode().await,
            InterceptMethod::Hosts => self.cleanup_hosts_sync(),
            InterceptMethod::Tproxy => self.disable_tproxy_mode().await,
        };
        if let Err(e) = method_cleanup {
            failures.push(format!("{:?} cleanup: {}", method, e));
        }

        //
        // Stop proxy.
        //
        let proxy_stopped = if let Some(proxy) = self.proxy.as_mut() {
            if let Err(e) = proxy.stop().await {
                failures.push(format!("proxy shutdown: {}", e));
                false
            } else {
                true
            }
        } else {
            false
        };
        if proxy_stopped {
            self.proxy = None;
        }

        //
        // Uninstall root CA.
        //
        let mut ca_uninstall_failed = false;
        if let Some(ca) = &self.ca {
            let ca_guard = ca.read().await;
            if let Err(e) = ca_guard.uninstall_root_cert() {
                failures.push(format!("root CA uninstall: {}", e));
                ca_uninstall_failed = true;
            }
        }
        //
        // Retain the CA handle for retry only if uninstall actually failed;
        // keyed on a typed flag, not the failure message text.
        //
        if !ca_uninstall_failed {
            self.ca = None;
        }

        //
        // Remove intercept environment variables.
        //
        if let Err(e) = env_vars::remove_intercept_env_vars() {
            failures.push(format!("environment cleanup: {}", e));
        }

        //
        // Remove Windows firewall rule owned by this session (or recovery
        // state if enable failed before self.intercept_state was assigned).
        //

        #[cfg(windows)]
        {
            let fw_state = self
                .intercept_state
                .clone()
                .or_else(|| state::load_state().ok().flatten());
            if let Some(ref fw) = fw_state {
                if fw.firewall_rule_added {
                    let removed = if let Some(ref name) = fw.firewall_rule_name {
                        crate::utils::remove_firewall_rule_named(name)
                    } else {
                        crate::utils::remove_legacy_firewall_rule()
                    };
                    if !removed {
                        failures.push("Windows firewall cleanup failed".to_string());
                    }
                }
            }
        }

        //
        // Remove state file since cleanup is complete.
        //

        if !failures.is_empty() {
            //
            // Incomplete cleanup is always CleanupRequired so re-enable is
            // blocked (not only when called via disable_after_partial_enable).
            //
            self.lifecycle = InterceptLifecycle::CleanupRequired;
            anyhow::bail!(
                "Intercept cleanup incomplete; recovery state retained: {}",
                failures.join("; ")
            );
        }

        if let Err(e) = state::cleanup_stale_state()
            .context("Failed to verify intercept cleanup from recovery state")
        {
            self.lifecycle = InterceptLifecycle::CleanupRequired;
            return Err(e);
        }

        self.is_enabled = false;
        self.lifecycle = finish_clean(self.lifecycle);
        self.method = None;
        self.proxy_port = None;
        self.domains.clear();
        self.domain_capture_configs.clear();
        self.intercept_state = None;
        self.ca = None;

        common::log_info!("Traffic interception disabled");

        Ok(())
    }

    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    pub fn lifecycle(&self) -> InterceptLifecycle {
        self.lifecycle
    }

    /// Attach cancel token for the current enable/disable operation.
    pub fn set_operation_cancel(&mut self, token: CancellationToken) {
        self.operation_cancel = Some(token);
    }

    pub fn clear_operation_cancel(&mut self) {
        self.operation_cancel = None;
    }

    /// Signal cancel of the in-flight enable (reset/shutdown). Enable awaits
    /// rollback on the same task rather than relying on future drop alone.
    pub fn request_cancel(&self) {
        if let Some(ref token) = self.operation_cancel {
            token.cancel();
        }
    }

    pub fn needs_cleanup(&self) -> bool {
        needs_cleanup(
            self.lifecycle,
            self.has_vpn_resources() || self.proxy.is_some() || self.ca.is_some(),
            self.intercept_state.is_some() || state::load_state().ok().flatten().is_some(),
        )
    }

    /// Disable after a partial enable that already set is_enabled; map cleanup
    /// failure to CleanupRequired so a blind re-enable is blocked.
    async fn disable_after_partial_enable(&mut self) -> Result<()> {
        match self.disable().await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.lifecycle = InterceptLifecycle::CleanupRequired;
                Err(e)
            }
        }
    }

    /// Failed enable path with CA attached: rollback and set Disabled only if
    /// cleanup fully succeeds; otherwise CleanupRequired and keep handles.
    async fn fail_enable_with_rollback(
        &mut self,
        ca: &Arc<RwLock<CertificateAuthority>>,
        cause: anyhow::Error,
    ) -> anyhow::Error {
        let rollback = self.rollback_partial_enable(ca).await;
        let ok = rollback.is_ok();
        self.lifecycle = after_rollback(ok);
        self.is_enabled = false;
        if ok {
            self.ca = None;
            self.proxy = None;
            self.method = None;
            self.proxy_port = None;
            self.intercept_state = None;
        }
        Self::combine_enable_failure(cause, rollback)
    }

    /// Reset/shutdown cleanup: cancel enable if running, disable/rollback, stale recovery.
    pub async fn force_cleanup(&mut self) -> Result<()> {
        self.request_cancel();
        let mut incomplete = false;
        if self.is_enabled {
            if let Err(e) = self.disable().await {
                common::log_error!("force_cleanup disable: {}", e);
                self.lifecycle = InterceptLifecycle::CleanupRequired;
                incomplete = true;
            }
        } else if matches!(
            self.lifecycle,
            InterceptLifecycle::Enabling | InterceptLifecycle::CleanupRequired
        ) {
            let mut ok = true;
            if let Some(ca) = self.ca.clone() {
                if let Err(e) = self.rollback_partial_enable(&ca).await {
                    common::log_error!("force_cleanup rollback: {}", e);
                    ok = false;
                }
            } else if let Err(e) = Self::rollback_cert_install_from_disk() {
                common::log_error!("force_cleanup cert recovery: {}", e);
                ok = false;
            }
            if self.has_vpn_resources() {
                if let Err(e) = self.disable_vpn_mode().await {
                    common::log_error!("force_cleanup VPN: {}", e);
                    ok = false;
                }
            }
            self.lifecycle = after_rollback(ok);
            if ok {
                self.ca = None;
                self.proxy = None;
                self.method = None;
                self.proxy_port = None;
                self.intercept_state = None;
            } else {
                incomplete = true;
            }
        }
        //
        // Never run disk stale cleanup while the packet engine task is still
        // owned (unconfirmed stop). That would remove recovery under a live task.
        //
        let engine_owned = self.packet_engine_task_owned();
        if !may_run_sync_vpn_or_stale_cleanup(engine_owned) {
            self.lifecycle = InterceptLifecycle::CleanupRequired;
            anyhow::bail!(
                "force_cleanup incomplete: packet engine task still owned; recovery retained"
            );
        }
        if incomplete {
            //
            // In-memory teardown failed earlier — keep recovery file.
            //
            self.lifecycle = InterceptLifecycle::CleanupRequired;
            anyhow::bail!("force_cleanup incomplete; recovery state retained");
        }
        if let Err(e) = state::cleanup_stale_state() {
            common::log_warn!("force_cleanup stale state: {}", e);
            self.lifecycle = InterceptLifecycle::CleanupRequired;
            return Err(e);
        }
        self.lifecycle = finish_clean(self.lifecycle);
        Ok(())
    }

    /// Whether Drop/sync VPN teardown is safe (no owned packet-engine task).
    pub fn packet_engine_task_owned(&self) -> bool {
        self.packet_engine_task.is_some()
    }

    pub(super) fn check_enable_cancelled(&self) -> Result<()> {
        let cancelled = self
            .operation_cancel
            .as_ref()
            .is_some_and(|t| t.is_cancelled());
        if should_abort_enable(cancelled) {
            anyhow::bail!("Intercept enable cancelled");
        }
        Ok(())
    }

    pub(super) async fn race_cancel<T, E, F>(&self, fut: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T, E>>,
        E: Into<anyhow::Error>,
    {
        let cancel = self.operation_cancel.clone();
        if let Some(token) = cancel {
            tokio::select! {
                biased;
                _ = token.cancelled() => {
                    anyhow::bail!("Intercept enable cancelled");
                }
                result = fut => result.map_err(Into::into),
            }
        } else {
            fut.await.map_err(Into::into)
        }
    }

    fn rollback_cert_install_from_disk() -> Result<()> {
        //
        // Best-effort CA uninstall via recovery metadata when no Arc CA is held.
        //
        match state::load_state() {
            Ok(Some(s)) if s.cert_installed => {
                //
                // Full uninstall paths live in cleanup_stale_state.
                //
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn method(&self) -> Option<InterceptMethod> {
        self.method
    }

    pub fn intercepted_domains(&self) -> Vec<String> {
        self.domains.iter().cloned().collect()
    }

    pub fn status(&self) -> common::InterceptStatus {
        let has_recovery = self.intercept_state.is_some()
            || state::load_state().ok().flatten().is_some();
        common::InterceptStatus {
            node_id: self.node_id.clone(),
            //
            // May be true while cleanup_required is also true (partial
            // teardown). UI prioritizes cleanup_required over the enabled pill.
            //
            enabled: self.is_enabled,
            method: self.method,
            proxy_port: self.proxy_port,
            intercepted_domains: self.intercepted_domains(),
            cleanup_required: status_shows_cleanup_required(self.lifecycle, has_recovery)
                || status_cleanup_required(self.lifecycle),
        }
    }

    //
    // Undo a partial enable that installed a CA but never reached is_enabled.
    // Does not touch VPN managers — use rollback_partial_enable when VPN may
    // have been prepared.
    //

    fn rollback_cert_install(ca: &CertificateAuthority) -> Result<()> {
        let mut failures = Vec::new();
        if let Err(e) = ca.uninstall_root_cert() {
            failures.push(format!("root CA uninstall: {}", e));
        }
        if let Err(e) = Self::rollback_firewall_from_recovery() {
            failures.push(e.to_string());
        }

        if !failures.is_empty() {
            anyhow::bail!(failures.join("; "));
        }

        state::remove_state().context("Failed to remove intercept recovery state")
    }

    #[allow(dead_code)]
    async fn rollback_cert_install_arc(ca: &Arc<RwLock<CertificateAuthority>>) -> Result<()> {
        let ca_guard = ca.read().await;
        Self::rollback_cert_install(&ca_guard)
    }

    //
    // Unified partial-enable rollback: VPN resources + CA + firewall, then
    // remove recovery state only if every privileged step succeeds.
    //

    async fn rollback_partial_enable(
        &mut self,
        ca: &Arc<RwLock<CertificateAuthority>>,
    ) -> Result<()> {
        let mut failures = Vec::new();

        if self.has_vpn_resources() {
            if let Err(e) = self.disable_vpn_mode().await {
                failures.push(format!("VPN cleanup: {}", e));
            }
        }

        {
            let ca_guard = ca.read().await;
            if let Err(e) = ca_guard.uninstall_root_cert() {
                failures.push(format!("root CA uninstall: {}", e));
            }
        }

        if let Err(e) = Self::rollback_firewall_from_recovery() {
            failures.push(e.to_string());
        }

        if !failures.is_empty() {
            anyhow::bail!(failures.join("; "));
        }

        state::remove_state().context("Failed to remove intercept recovery state")
    }

    fn rollback_firewall_from_recovery() -> Result<()> {
        #[cfg(windows)]
        {
            match state::load_state() {
                Ok(Some(intercept_state)) if intercept_state.firewall_rule_added => {
                    let removed = if let Some(ref name) = intercept_state.firewall_rule_name {
                        crate::utils::remove_firewall_rule_named(name)
                    } else {
                        crate::utils::remove_legacy_firewall_rule()
                    };
                    if !removed {
                        anyhow::bail!("Windows firewall rule removal failed");
                    }
                }
                Ok(_) => {}
                Err(error) => anyhow::bail!("firewall recovery state: {}", error),
            }
        }
        Ok(())
    }

    /// Live VPN managers/resources present (including pre-is_enabled TunUp).
    fn has_vpn_resources(&self) -> bool {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            if self.route_manager.is_some()
                || self.vpn_bypass_manager.is_some()
                || self.ipv6_manager.is_some()
                || self.dns_resolver.is_some()
                || self.tun_device.is_some()
                || self.packet_engine_task.is_some()
                || self.shutdown_token.is_some()
            {
                return true;
            }
        }
        #[cfg(target_os = "windows")]
        {
            if self.wintun_manager.is_some() {
                return true;
            }
        }
        #[cfg(target_os = "linux")]
        {
            if self.tun_manager.is_some() {
                return true;
            }
        }
        false
    }

    fn combine_enable_failure(cause: anyhow::Error, rollback: Result<()>) -> anyhow::Error {
        match rollback {
            Ok(()) => cause,
            Err(rollback_error) => anyhow::anyhow!(
                "{}; rollback also failed and recovery state was retained: {}",
                cause,
                rollback_error
            ),
        }
    }
}

impl NodeInterceptManager {
    //
    // Synchronous cleanup for Proxy and Hosts methods.
    // Called by both disable() and Drop.
    //

    fn cleanup_proxy_sync(&mut self) -> Result<()> {
        //
        // Prefer in-memory saved settings. Fall back to write-ahead recovery
        // (in-memory or disk) so partial enable after proxy_modified=true still
        // restores the user's original proxy instead of forcing ProxyEnable=0.
        //
        let recovered = self.saved_proxy_settings.take().or_else(|| {
            let st = self
                .intercept_state
                .as_ref()
                .cloned()
                .or_else(|| state::load_state().ok().flatten())?;
            if !st.proxy_modified {
                return None;
            }
            Some(SavedProxySettings {
                proxy_enable: st.saved_proxy_enable.unwrap_or(0),
                proxy_server: st.saved_proxy_server,
            })
        });
        disable_system_proxy(recovered.as_ref())
            .context("Failed to restore system proxy settings")?;
        Ok(())
    }

    fn cleanup_hosts_sync(&mut self) -> Result<()> {
        let mut failures = Vec::new();
        if let Err(e) = hosts::remove_all_hosts_entries() {
            failures.push(format!("hosts entries: {}", e));
        }
        //
        // Only tear down the Linux iptables REDIRECT when ownership says one
        // was installed (or legacy state without the flag may still need it).
        //
        let cleanup_redirect = self
            .intercept_state
            .as_ref()
            .map(state::should_cleanup_hosts_redirect)
            .unwrap_or(false);
        if cleanup_redirect {
            if let Err(e) = hosts::disable_hosts_redirect(self.proxy_port) {
                failures.push(format!("hosts redirect: {}", e));
            }
        }
        hosts::flush_dns_cache();

        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }
}

impl Drop for NodeInterceptManager {
    fn drop(&mut self) {
        //
        // Never tear down adapter/TUN under an unconfirmed packet-engine task.
        // Abort the task handle without joining (Drop is sync) and leave host
        // resources for process exit / next recovery — do not detach then
        // destroy the device the task may still hold.
        //
        let engine_owned = self.packet_engine_task.is_some();
        if engine_owned {
            if let Some(token) = self.shutdown_token.take() {
                token.cancel();
            }
            if let Some(task) = self.packet_engine_task.take() {
                task.abort();
            }
            common::log_error!(
                "Drop: skipped VPN sync cleanup; packet engine was still owned (is_enabled={})",
                self.is_enabled
            );
            return;
        }

        if self.has_vpn_resources() {
            if let Err(e) = self.cleanup_vpn_sync() {
                common::log_error!(
                    "VPN cleanup during drop was incomplete (is_enabled={}): {}",
                    self.is_enabled,
                    e
                );
            }
        }

        if !self.is_enabled {
            return;
        }

        let method = self.method.unwrap_or(InterceptMethod::Proxy);
        let result = match method {
            InterceptMethod::Proxy => self.cleanup_proxy_sync(),
            InterceptMethod::Vpn => Ok(()),
            InterceptMethod::Hosts => self.cleanup_hosts_sync(),
            InterceptMethod::Tproxy => self.cleanup_tproxy_sync(),
        };
        if let Err(e) = result {
            common::log_error!("Intercept cleanup during drop was incomplete: {}", e);
        }
    }
}
