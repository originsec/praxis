//
// Pure per-method listen / enable-phase policy. No I/O — unit-tested so VPN
// bind ordering cannot silently regress relative to TUN bring-up, and so
// Proxy/Hosts stay loopback-only while TPROXY uses a wildcard bind.
//

use common::InterceptMethod;

/// Ordered enable steps. For VPN, `TunUp` must precede `BindListener`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnablePhase {
    /// Create/configure TUN (and any pre-bind system prep).
    TunUp,
    /// Bind the intercept proxy listener.
    BindListener,
    /// Method-specific routing after the listener port is known.
    MethodRouting,
    /// Optional Windows port-scoped firewall (VPN only).
    Firewall,
}

/// Where the proxy listener binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindSpec {
    /// `127.0.0.1:0` — Proxy mode.
    LoopbackEphemeral,
    /// `127.0.0.1:443` (+ optional :80) — Hosts mode.
    LoopbackFixedHttps,
    /// `TUN_IP:0` (`10.255.0.1`) — VPN mode; requires TunUp first.
    TunIpEphemeral,
    /// `0.0.0.0:0` transparent — TPROXY mode.
    WildcardTransparent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenPlan {
    pub method: InterceptMethod,
    pub phases: &'static [EnablePhase],
    pub bind: BindSpec,
}

pub fn plan_for(method: InterceptMethod) -> ListenPlan {
    match method {
        InterceptMethod::Proxy => ListenPlan {
            method,
            phases: &[EnablePhase::BindListener, EnablePhase::MethodRouting],
            bind: BindSpec::LoopbackEphemeral,
        },
        InterceptMethod::Hosts => ListenPlan {
            method,
            phases: &[EnablePhase::BindListener, EnablePhase::MethodRouting],
            bind: BindSpec::LoopbackFixedHttps,
        },
        InterceptMethod::Vpn => ListenPlan {
            method,
            phases: &[
                EnablePhase::TunUp,
                EnablePhase::BindListener,
                EnablePhase::MethodRouting,
                EnablePhase::Firewall,
            ],
            bind: BindSpec::TunIpEphemeral,
        },
        InterceptMethod::Tproxy => ListenPlan {
            method,
            phases: &[EnablePhase::BindListener, EnablePhase::MethodRouting],
            bind: BindSpec::WildcardTransparent,
        },
    }
}

/// True when enable must bring the TUN up before binding the proxy.
pub fn requires_tun_before_bind(plan: &ListenPlan) -> bool {
    matches!(plan.phases.first(), Some(EnablePhase::TunUp))
}

/// Primary listener bind address string used by production `InterceptProxy::start`.
/// Hosts secondary HTTP (`127.0.0.1:80`) is best-effort and not part of BindSpec.
pub fn primary_bind_addr(spec: BindSpec) -> String {
    match spec {
        BindSpec::LoopbackEphemeral => "127.0.0.1:0".to_string(),
        BindSpec::LoopbackFixedHttps => "127.0.0.1:443".to_string(),
        BindSpec::TunIpEphemeral => format!("{}:0", super::routing::TUN_IP),
        BindSpec::WildcardTransparent => "0.0.0.0:0".to_string(),
    }
}

#[allow(dead_code)] // unit tests + policy documentation API
pub fn phase_index(plan: &ListenPlan, phase: EnablePhase) -> Option<usize> {
    plan.phases.iter().position(|p| *p == phase)
}

/// Whether peer admission for this method is loopback-only (bind is not public).
#[allow(dead_code)] // unit tests + policy documentation API
pub fn is_loopback_only(plan: &ListenPlan) -> bool {
    matches!(
        plan.bind,
        BindSpec::LoopbackEphemeral | BindSpec::LoopbackFixedHttps
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intercept::proxy::{admit_client_peer, admit_tproxy_redirect};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn vpn_bind_after_tun_up() {
        let plan = plan_for(InterceptMethod::Vpn);
        let tun = phase_index(&plan, EnablePhase::TunUp).expect("VPN has TunUp");
        let bind = phase_index(&plan, EnablePhase::BindListener).expect("VPN has BindListener");
        assert!(
            bind > tun,
            "VPN BindListener (index {bind}) must come after TunUp (index {tun})"
        );
        assert_eq!(plan.bind, BindSpec::TunIpEphemeral);
        assert!(requires_tun_before_bind(&plan));
        assert!(phase_index(&plan, EnablePhase::Firewall).is_some());
    }

    #[test]
    fn proxy_and_hosts_are_loopback_only() {
        let proxy = plan_for(InterceptMethod::Proxy);
        let hosts = plan_for(InterceptMethod::Hosts);
        assert_eq!(proxy.bind, BindSpec::LoopbackEphemeral);
        assert_eq!(hosts.bind, BindSpec::LoopbackFixedHttps);
        assert!(is_loopback_only(&proxy));
        assert!(is_loopback_only(&hosts));
        assert!(!requires_tun_before_bind(&proxy));
        assert!(!requires_tun_before_bind(&hosts));
        assert!(phase_index(&proxy, EnablePhase::TunUp).is_none());
        assert!(phase_index(&hosts, EnablePhase::TunUp).is_none());
        assert!(admit_client_peer(InterceptMethod::Proxy, IpAddr::V4(Ipv4Addr::LOCALHOST)).is_ok());
        assert!(admit_client_peer(
            InterceptMethod::Hosts,
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))
        )
        .is_err());
    }

    #[test]
    fn tproxy_is_wildcard_with_port_admission() {
        let plan = plan_for(InterceptMethod::Tproxy);
        assert_eq!(plan.bind, BindSpec::WildcardTransparent);
        assert!(phase_index(&plan, EnablePhase::TunUp).is_none());
        let proxy_listen_port = 45678u16;
        let remote: SocketAddr = "93.184.216.34:443".parse().unwrap();
        assert!(
            admit_tproxy_redirect(remote, proxy_listen_port).is_ok(),
            "redirected :443 must pass even if getsockname equals original_dst"
        );
        let direct: SocketAddr = format!("10.0.0.5:{proxy_listen_port}").parse().unwrap();
        assert!(admit_tproxy_redirect(direct, proxy_listen_port).is_err());
    }

    #[test]
    fn vpn_admission_requires_tun_subnet() {
        assert!(admit_client_peer(
            InterceptMethod::Vpn,
            IpAddr::V4(Ipv4Addr::new(10, 255, 0, 100))
        )
        .is_ok());
        assert!(admit_client_peer(
            InterceptMethod::Vpn,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2))
        )
        .is_err());
    }

    #[test]
    fn primary_bind_addr_matches_production_policy() {
        use crate::intercept::routing::TUN_IP;

        assert_eq!(
            primary_bind_addr(plan_for(InterceptMethod::Proxy).bind),
            "127.0.0.1:0"
        );
        assert_eq!(
            primary_bind_addr(plan_for(InterceptMethod::Hosts).bind),
            "127.0.0.1:443"
        );
        assert_eq!(
            primary_bind_addr(plan_for(InterceptMethod::Vpn).bind),
            format!("{TUN_IP}:0")
        );
        assert_eq!(
            primary_bind_addr(plan_for(InterceptMethod::Tproxy).bind),
            "0.0.0.0:0"
        );
        //
        // Same source of truth production proxy start must call.
        //
        assert_eq!(
            primary_bind_addr(BindSpec::TunIpEphemeral),
            format!("{}:0", TUN_IP)
        );
    }
}
