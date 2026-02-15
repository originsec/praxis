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
use crate::agent_connectors::Agent;
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


include!("manager/runtime_impl.rs");
include!("manager/cleanup_impl.rs");
include!("manager/drop_impl.rs");
