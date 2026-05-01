use crate::intercept::dns_resolver::DomainResolver;
use crate::intercept::routing::{TUN_IP, TUN_IP6};
use crate::intercept::tun_device::SharedTunDevice;
use dashmap::DashMap;
use etherparse::{IpNumber, Ipv4Header, Ipv6Header, TcpHeader};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// NAT table key for connection tracking (supports both IPv4 and IPv6).
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct NatKey {
    /// Source IP (client)
    src_ip: IpAddr,
    /// Source port (client ephemeral port)
    src_port: u16,
    /// Original destination IP (server)
    dst_ip: IpAddr,
    /// Original destination port (usually 443)
    dst_port: u16,
}

/// NAT table entry.
#[derive(Clone, Debug)]
struct NatEntry {
    /// Original destination IP before NAT
    original_dst_ip: IpAddr,
    /// Original destination port before NAT
    original_dst_port: u16,
    /// When this entry was created
    created_at: Instant,
    /// Last activity time (for cleanup)
    last_activity: Instant,
}

/// TUN interface IPv4 address as Ipv4Addr (where proxy listens).
fn tun_ipv4() -> Ipv4Addr {
    TUN_IP.parse().unwrap()
}

/// TUN interface IPv6 address as Ipv6Addr (where proxy listens).
fn tun_ipv6() -> Ipv6Addr {
    TUN_IP6.parse().unwrap()
}

/// Virtual client IPv4 for NAT'd connections.
///
/// Using a different IP in the TUN subnet so proxy responses come back through
/// TUN.
fn virtual_client_ipv4() -> Ipv4Addr {
    Ipv4Addr::new(10, 255, 0, 100)
}

/// Virtual client IPv6 for NAT'd connections.
fn virtual_client_ipv6() -> Ipv6Addr {
    "fd00:255:0::100".parse().unwrap()
}

/// Packet engine for VPN interception.
///
/// This is now cross-platform - it works with any TunDevice implementation.
/// Supports both IPv4 and IPv6 packet processing.
pub struct PacketEngine {
    /// TUN device for packet I/O
    device: SharedTunDevice,
    /// NAT table for connection tracking (both IPv4 and IPv6)
    nat_table: DashMap<NatKey, NatEntry>,
    /// Reverse NAT table (for response packets)
    /// Key: (proxy_src_port) -> NatKey
    reverse_nat: DashMap<u16, NatKey>,
    /// Port of the local MITM proxy
    proxy_port: u16,
    /// DNS resolver for checking intercept IPs
    dns_resolver: Arc<DomainResolver>,
    /// Intercept IPs cache (refreshed periodically)
    intercept_ips: Arc<RwLock<std::collections::HashSet<IpAddr>>>,
}

impl PacketEngine {
    /// Create a new packet engine.
    pub fn new(
        device: SharedTunDevice,
        proxy_port: u16,
        dns_resolver: Arc<DomainResolver>,
    ) -> Self {
        Self {
            device,
            nat_table: DashMap::new(),
            reverse_nat: DashMap::new(),
            proxy_port,
            dns_resolver,
            intercept_ips: Arc::new(RwLock::new(std::collections::HashSet::new())),
        }
    }

    /// Refresh the intercept IPs from the DNS resolver.
    pub async fn refresh_intercept_ips(&self) {
        let ips = self.dns_resolver.get_all_intercept_ips();
        let mut guard = self.intercept_ips.write().await;
        *guard = ips;
    }

    /// Run the packet processing loop.
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken) {
        common::log_info!("Packet engine starting (IPv4 + IPv6 support)");

        //
        // Initial refresh of intercept IPs.
        //
        self.refresh_intercept_ips().await;

        //
        // Spawn a task for periodic NAT table cleanup.
        //
        let cleanup_engine = self.clone();
        let cleanup_shutdown = shutdown.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = cleanup_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        cleanup_engine.cleanup_stale_entries();
                    }
                }
            }
        });

        //
        // Main packet processing loop.
        //
        loop {
            if shutdown.is_cancelled() {
                common::log_info!("Packet engine shutdown requested");
                break;
            }

            //
            // Try to receive a packet (blocking). The shutdown() call on the
            // device will unblock this.
            //
            match self.device.receive_blocking() {
                Ok(packet_bytes) => {
                    common::log_trace!("Received packet: {} bytes", packet_bytes.len());

                    if let Some(response) = self.process_packet(&packet_bytes).await {
                        //
                        // Send the modified packet.
                        //
                        match self.device.send(&response) {
                            Ok(()) => {
                                common::log_trace!(
                                    "Sent rewritten packet ({} bytes)",
                                    response.len()
                                );
                            }
                            Err(e) => {
                                common::log_error!("Failed to send packet: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    //
                    // Check if this is a shutdown error.
                    //
                    if shutdown.is_cancelled() {
                        break;
                    }
                    common::log_warn!("Error receiving packet: {}", e);
                }
            }
        }

        common::log_info!("Packet engine stopped");
    }

    /// Process an incoming packet and return the modified packet (if any).
    async fn process_packet(&self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.is_empty() {
            return None;
        }

        //
        // Check IP version from first nibble.
        //
        let version = packet[0] >> 4;

        match version {
            4 => self.process_ipv4_packet(packet).await,
            6 => self.process_ipv6_packet(packet).await,
            _ => {
                common::log_trace!("Unknown IP version {}, ignoring", version);
                None
            }
        }
    }

    /// Process an IPv4 packet.
    async fn process_ipv4_packet(&self, packet: &[u8]) -> Option<Vec<u8>> {
        //
        // Parse the IP header.
        //
        let ip_header = match Ipv4Header::from_slice(packet) {
            Ok((header, _)) => header,
            Err(_) => {
                common::log_trace!("Failed to parse IPv4 header");
                return None;
            }
        };

        //
        // Only handle TCP for now.
        //
        if ip_header.protocol != IpNumber::TCP {
            common::log_trace!(
                "Non-TCP packet (protocol: {:?}), passing through",
                ip_header.protocol
            );
            return Some(packet.to_vec());
        }

        let ip_header_len = ip_header.header_len() as usize;
        let tcp_start = ip_header_len;

        //
        // Parse TCP header.
        //
        let tcp_header = match TcpHeader::from_slice(&packet[tcp_start..]) {
            Ok((header, _)) => header,
            Err(_) => {
                common::log_trace!("Failed to parse TCP header");
                return Some(packet.to_vec());
            }
        };

        let src_ip = Ipv4Addr::from(ip_header.source);
        let dst_ip = Ipv4Addr::from(ip_header.destination);
        let src_port = tcp_header.source_port;
        let dst_port = tcp_header.destination_port;

        common::log_trace!(
            "IPv4 TCP packet: {}:{} -> {}:{}",
            src_ip,
            src_port,
            dst_ip,
            dst_port
        );

        //
        // Check if this is an outbound packet to an intercept IP.
        //
        // IMPORTANT: Only NAT packets that originate from the TUN interface
        // (source IP = TUN IP). Packets from other IPs (like the real host IP)
        // are from the proxy's bypass connection and should NOT be NAT'd -
        // they need to pass through to the real server.
        //
        let intercept_ips = self.intercept_ips.read().await;
        let is_intercept_dst = intercept_ips.contains(&IpAddr::V4(dst_ip));
        drop(intercept_ips);

        let is_from_tun = src_ip == tun_ipv4();

        if is_intercept_dst && dst_port == 443 && is_from_tun {
            //
            // Outbound to intercepted server FROM TUN - NAT to local proxy.
            //
            return self.nat_outbound_v4(packet, &ip_header, &tcp_header);
        }

        if is_intercept_dst && !is_from_tun {
            //
            // This is the proxy's bypass connection to the real server.
            // Pass through without modification.
            //
            common::log_debug!(
                "Passing through proxy bypass (IPv4): {}:{} -> {}:{}",
                src_ip,
                src_port,
                dst_ip,
                dst_port
            );
            return Some(packet.to_vec());
        }

        //
        // Check if this is a response from the proxy (destined for virtual
        // client).
        //
        if dst_ip == virtual_client_ipv4() && src_ip == tun_ipv4() && src_port == self.proxy_port {
            //
            // Response from proxy - reverse NAT.
            //
            return self.nat_inbound_v4(packet, &ip_header, &tcp_header);
        }

        //
        // Not an intercept packet, pass through.
        //
        Some(packet.to_vec())
    }

    /// Process an IPv6 packet.
    async fn process_ipv6_packet(&self, packet: &[u8]) -> Option<Vec<u8>> {
        //
        // Parse the IPv6 header.
        //
        let (ip_header, payload) = match Ipv6Header::from_slice(packet) {
            Ok(result) => result,
            Err(_) => {
                common::log_trace!("Failed to parse IPv6 header");
                return None;
            }
        };

        //
        // Only handle TCP for now. Note: IPv6 may have extension headers,
        // but we only handle the simple case where TCP follows immediately.
        //
        if ip_header.next_header != IpNumber::TCP {
            common::log_trace!(
                "Non-TCP IPv6 packet (next_header: {:?}), passing through",
                ip_header.next_header
            );
            return Some(packet.to_vec());
        }

        //
        // Parse TCP header from payload.
        //
        let tcp_header = match TcpHeader::from_slice(payload) {
            Ok((header, _)) => header,
            Err(_) => {
                common::log_trace!("Failed to parse TCP header in IPv6 packet");
                return Some(packet.to_vec());
            }
        };

        let src_ip = Ipv6Addr::from(ip_header.source);
        let dst_ip = Ipv6Addr::from(ip_header.destination);
        let src_port = tcp_header.source_port;
        let dst_port = tcp_header.destination_port;

        common::log_trace!(
            "IPv6 TCP packet: [{}]:{} -> [{}]:{}",
            src_ip,
            src_port,
            dst_ip,
            dst_port
        );

        //
        // Check if this is an outbound packet to an intercept IP.
        //
        // IMPORTANT: Only NAT packets that originate from the TUN interface
        // (source IP = TUN IP). Packets from other IPs are from the proxy's
        // bypass connection and should NOT be NAT'd.
        //
        let intercept_ips = self.intercept_ips.read().await;
        let is_intercept_dst = intercept_ips.contains(&IpAddr::V6(dst_ip));
        drop(intercept_ips);

        let is_from_tun = src_ip == tun_ipv6();

        if is_intercept_dst && dst_port == 443 && is_from_tun {
            //
            // Outbound to intercepted server FROM TUN - NAT to local proxy.
            //
            return self.nat_outbound_v6(packet, &ip_header, &tcp_header);
        }

        if is_intercept_dst && !is_from_tun {
            //
            // This is the proxy's bypass connection to the real server.
            // Pass through without modification.
            //
            common::log_debug!(
                "Passing through proxy bypass (IPv6): [{}]:{} -> [{}]:{}",
                src_ip,
                src_port,
                dst_ip,
                dst_port
            );
            return Some(packet.to_vec());
        }

        //
        // Check if this is a response from the proxy (destined for virtual
        // client).
        //
        if dst_ip == virtual_client_ipv6() && src_ip == tun_ipv6() && src_port == self.proxy_port {
            //
            // Response from proxy - reverse NAT.
            //
            return self.nat_inbound_v6(packet, &ip_header, &tcp_header);
        }

        //
        // Not an intercept packet, pass through.
        //
        Some(packet.to_vec())
    }

    /// NAT an outbound IPv4 packet to the local proxy.
    fn nat_outbound_v4(
        &self,
        packet: &[u8],
        ip_header: &Ipv4Header,
        tcp_header: &TcpHeader,
    ) -> Option<Vec<u8>> {
        let src_ip = Ipv4Addr::from(ip_header.source);
        let dst_ip = Ipv4Addr::from(ip_header.destination);
        let src_port = tcp_header.source_port;
        let dst_port = tcp_header.destination_port;

        common::log_debug!(
            "NAT outbound (IPv4): {}:{} -> {}:{} => proxy",
            src_ip,
            src_port,
            dst_ip,
            dst_port
        );

        //
        // Create NAT entry.
        //
        let nat_key = NatKey {
            src_ip: IpAddr::V4(src_ip),
            src_port,
            dst_ip: IpAddr::V4(dst_ip),
            dst_port,
        };

        let nat_entry = NatEntry {
            original_dst_ip: IpAddr::V4(dst_ip),
            original_dst_port: dst_port,
            created_at: Instant::now(),
            last_activity: Instant::now(),
        };

        //
        // Store in NAT table.
        //
        self.nat_table.insert(nat_key.clone(), nat_entry);

        //
        // Also store reverse mapping for response packets.
        //
        self.reverse_nat.insert(src_port, nat_key);

        //
        // Rewrite the packet.
        //
        self.rewrite_packet_to_proxy_v4(packet, ip_header, tcp_header)
    }

    /// NAT an outbound IPv6 packet to the local proxy.
    fn nat_outbound_v6(
        &self,
        packet: &[u8],
        ip_header: &Ipv6Header,
        tcp_header: &TcpHeader,
    ) -> Option<Vec<u8>> {
        let src_ip = Ipv6Addr::from(ip_header.source);
        let dst_ip = Ipv6Addr::from(ip_header.destination);
        let src_port = tcp_header.source_port;
        let dst_port = tcp_header.destination_port;

        common::log_debug!(
            "NAT outbound (IPv6): [{}]:{} -> [{}]:{} => proxy",
            src_ip,
            src_port,
            dst_ip,
            dst_port
        );

        //
        // Create NAT entry.
        //
        let nat_key = NatKey {
            src_ip: IpAddr::V6(src_ip),
            src_port,
            dst_ip: IpAddr::V6(dst_ip),
            dst_port,
        };

        let nat_entry = NatEntry {
            original_dst_ip: IpAddr::V6(dst_ip),
            original_dst_port: dst_port,
            created_at: Instant::now(),
            last_activity: Instant::now(),
        };

        //
        // Store in NAT table.
        //
        self.nat_table.insert(nat_key.clone(), nat_entry);

        //
        // Also store reverse mapping for response packets.
        //
        self.reverse_nat.insert(src_port, nat_key);

        //
        // Rewrite the packet.
        //
        self.rewrite_packet_to_proxy_v6(packet, ip_header, tcp_header)
    }

    /// NAT an inbound IPv4 packet from the proxy back to the original destination.
    fn nat_inbound_v4(
        &self,
        packet: &[u8],
        ip_header: &Ipv4Header,
        tcp_header: &TcpHeader,
    ) -> Option<Vec<u8>> {
        let dst_port = tcp_header.destination_port;

        //
        // Look up the original connection.
        //
        let nat_key = self.reverse_nat.get(&dst_port)?;
        let nat_entry = self.nat_table.get(&*nat_key)?;

        common::log_debug!(
            "NAT inbound (IPv4): proxy -> {}:{}",
            nat_entry.original_dst_ip,
            nat_entry.original_dst_port
        );

        //
        // Update last activity.
        //
        drop(nat_entry);
        if let Some(mut entry) = self.nat_table.get_mut(&*nat_key) {
            entry.last_activity = Instant::now();
        }

        //
        // Rewrite the packet from proxy.
        //
        self.rewrite_packet_from_proxy_v4(packet, ip_header, tcp_header, &*nat_key)
    }

    /// NAT an inbound IPv6 packet from the proxy back to the original destination.
    fn nat_inbound_v6(
        &self,
        packet: &[u8],
        ip_header: &Ipv6Header,
        tcp_header: &TcpHeader,
    ) -> Option<Vec<u8>> {
        let dst_port = tcp_header.destination_port;

        //
        // Look up the original connection.
        //
        let nat_key = self.reverse_nat.get(&dst_port)?;
        let nat_entry = self.nat_table.get(&*nat_key)?;

        common::log_debug!(
            "NAT inbound (IPv6): proxy -> {}:{}",
            nat_entry.original_dst_ip,
            nat_entry.original_dst_port
        );

        //
        // Update last activity.
        //
        drop(nat_entry);
        if let Some(mut entry) = self.nat_table.get_mut(&*nat_key) {
            entry.last_activity = Instant::now();
        }

        //
        // Rewrite the packet from proxy.
        //
        self.rewrite_packet_from_proxy_v6(packet, ip_header, tcp_header, &*nat_key)
    }

    /// Rewrite an IPv4 packet to send to the proxy.
    fn rewrite_packet_to_proxy_v4(
        &self,
        packet: &[u8],
        ip_header: &Ipv4Header,
        tcp_header: &TcpHeader,
    ) -> Option<Vec<u8>> {
        let ip_header_len = ip_header.header_len() as usize;
        let tcp_header_len = tcp_header.header_len() as usize;
        let payload_start = ip_header_len + tcp_header_len;
        let payload = &packet[payload_start..];

        //
        // Create new IP header:
        // - Source: virtual client IP (so proxy responses come back through
        // TUN)
        // - Destination: TUN IP (where proxy listens).
        //
        let mut new_ip = ip_header.clone();
        new_ip.source = virtual_client_ipv4().octets();
        new_ip.destination = tun_ipv4().octets();

        //
        // Create new TCP header with destination port changed to proxy.
        //
        let mut new_tcp = tcp_header.clone();
        new_tcp.destination_port = self.proxy_port;
        //
        // Keep source port as-is (client ephemeral port).
        //

        //
        // Recalculate checksums.
        //
        new_tcp.checksum = new_tcp.calc_checksum_ipv4(&new_ip, payload).ok()?;
        new_ip.header_checksum = new_ip.calc_header_checksum();

        //
        // Build the new packet.
        //
        let mut result = Vec::with_capacity(packet.len());
        result.extend_from_slice(&new_ip.to_bytes());
        result.extend_from_slice(&new_tcp.to_bytes());
        result.extend_from_slice(payload);

        let flags = format_tcp_flags(tcp_header);
        common::log_debug!(
            "Rewritten to proxy (IPv4): {}:{} -> {}:{} [{}] ({} bytes)",
            Ipv4Addr::from(new_ip.source),
            new_tcp.source_port,
            Ipv4Addr::from(new_ip.destination),
            new_tcp.destination_port,
            flags.trim(),
            payload.len()
        );

        Some(result)
    }

    /// Rewrite an IPv6 packet to send to the proxy.
    fn rewrite_packet_to_proxy_v6(
        &self,
        packet: &[u8],
        ip_header: &Ipv6Header,
        tcp_header: &TcpHeader,
    ) -> Option<Vec<u8>> {
        let ip_header_len = Ipv6Header::LEN;
        let tcp_header_len = tcp_header.header_len() as usize;
        let payload_start = ip_header_len + tcp_header_len;
        let payload = &packet[payload_start..];

        //
        // Create new IPv6 header:
        // - Source: virtual client IPv6 (so proxy responses come back through
        // TUN)
        // - Destination: TUN IPv6 (where proxy listens).
        //
        let mut new_ip = ip_header.clone();
        new_ip.source = virtual_client_ipv6().octets();
        new_ip.destination = tun_ipv6().octets();

        //
        // Create new TCP header with destination port changed to proxy.
        //
        let mut new_tcp = tcp_header.clone();
        new_tcp.destination_port = self.proxy_port;

        //
        // Recalculate TCP checksum (IPv6 uses different pseudo-header).
        //
        new_tcp.checksum = new_tcp.calc_checksum_ipv6(&new_ip, payload).ok()?;

        //
        // Build the new packet.
        //
        let mut result = Vec::with_capacity(packet.len());
        result.extend_from_slice(&new_ip.to_bytes());
        result.extend_from_slice(&new_tcp.to_bytes());
        result.extend_from_slice(payload);

        let flags = format_tcp_flags(tcp_header);
        common::log_debug!(
            "Rewritten to proxy (IPv6): [{}]:{} -> [{}]:{} [{}] ({} bytes)",
            Ipv6Addr::from(new_ip.source),
            new_tcp.source_port,
            Ipv6Addr::from(new_ip.destination),
            new_tcp.destination_port,
            flags.trim(),
            payload.len()
        );

        Some(result)
    }

    /// Rewrite an IPv4 packet coming from the proxy.
    fn rewrite_packet_from_proxy_v4(
        &self,
        packet: &[u8],
        ip_header: &Ipv4Header,
        tcp_header: &TcpHeader,
        nat_key: &NatKey,
    ) -> Option<Vec<u8>> {
        let ip_header_len = ip_header.header_len() as usize;
        let tcp_header_len = tcp_header.header_len() as usize;
        let payload_start = ip_header_len + tcp_header_len;
        let payload = &packet[payload_start..];

        //
        // Extract IPv4 addresses from nat_key.
        //
        let (dst_ip, src_ip) = match (&nat_key.dst_ip, &nat_key.src_ip) {
            (IpAddr::V4(dst), IpAddr::V4(src)) => (*dst, *src),
            _ => return None,
        };

        //
        // Create new IP header with source changed to original destination.
        //
        let mut new_ip = ip_header.clone();
        new_ip.source = dst_ip.octets();
        new_ip.destination = src_ip.octets();

        //
        // Create new TCP header with source port changed to original.
        //
        let mut new_tcp = tcp_header.clone();
        new_tcp.source_port = nat_key.dst_port;
        new_tcp.destination_port = nat_key.src_port;

        //
        // Recalculate checksums.
        //
        new_tcp.checksum = new_tcp.calc_checksum_ipv4(&new_ip, payload).ok()?;
        new_ip.header_checksum = new_ip.calc_header_checksum();

        //
        // Build the new packet.
        //
        let mut result = Vec::with_capacity(packet.len());
        result.extend_from_slice(&new_ip.to_bytes());
        result.extend_from_slice(&new_tcp.to_bytes());
        result.extend_from_slice(payload);

        let flags = format_tcp_flags(tcp_header);
        common::log_debug!(
            "Rewritten from proxy (IPv4): {}:{} -> {}:{} [{}] ({} bytes)",
            Ipv4Addr::from(new_ip.source),
            new_tcp.source_port,
            Ipv4Addr::from(new_ip.destination),
            new_tcp.destination_port,
            flags.trim(),
            payload.len()
        );

        Some(result)
    }

    /// Rewrite an IPv6 packet coming from the proxy.
    fn rewrite_packet_from_proxy_v6(
        &self,
        packet: &[u8],
        ip_header: &Ipv6Header,
        tcp_header: &TcpHeader,
        nat_key: &NatKey,
    ) -> Option<Vec<u8>> {
        let ip_header_len = Ipv6Header::LEN;
        let tcp_header_len = tcp_header.header_len() as usize;
        let payload_start = ip_header_len + tcp_header_len;
        let payload = &packet[payload_start..];

        //
        // Extract IPv6 addresses from nat_key.
        //
        let (dst_ip, src_ip) = match (&nat_key.dst_ip, &nat_key.src_ip) {
            (IpAddr::V6(dst), IpAddr::V6(src)) => (*dst, *src),
            _ => return None,
        };

        //
        // Create new IPv6 header with source changed to original destination.
        //
        let mut new_ip = ip_header.clone();
        new_ip.source = dst_ip.octets();
        new_ip.destination = src_ip.octets();

        //
        // Create new TCP header with source port changed to original.
        //
        let mut new_tcp = tcp_header.clone();
        new_tcp.source_port = nat_key.dst_port;
        new_tcp.destination_port = nat_key.src_port;

        //
        // Recalculate TCP checksum.
        //
        new_tcp.checksum = new_tcp.calc_checksum_ipv6(&new_ip, payload).ok()?;

        //
        // Build the new packet.
        //
        let mut result = Vec::with_capacity(packet.len());
        result.extend_from_slice(&new_ip.to_bytes());
        result.extend_from_slice(&new_tcp.to_bytes());
        result.extend_from_slice(payload);

        let flags = format_tcp_flags(tcp_header);
        common::log_debug!(
            "Rewritten from proxy (IPv6): [{}]:{} -> [{}]:{} [{}] ({} bytes)",
            Ipv6Addr::from(new_ip.source),
            new_tcp.source_port,
            Ipv6Addr::from(new_ip.destination),
            new_tcp.destination_port,
            flags.trim(),
            payload.len()
        );

        Some(result)
    }

    /// Clean up stale NAT entries (connections inactive for > 5 minutes).
    fn cleanup_stale_entries(&self) {
        let timeout = std::time::Duration::from_secs(300);
        let now = Instant::now();
        let mut to_remove = Vec::new();

        for entry in self.nat_table.iter() {
            if now.duration_since(entry.value().last_activity) > timeout {
                to_remove.push(entry.key().clone());
            }
        }

        for key in to_remove {
            if let Some((_, entry)) = self.nat_table.remove(&key) {
                common::log_debug!(
                    "Removed stale NAT entry: {:?} (age: {:?})",
                    key,
                    now.duration_since(entry.created_at)
                );
            }
            self.reverse_nat.remove(&key.src_port);
        }
    }
}

/// Format TCP flags for debug logging.
fn format_tcp_flags(tcp_header: &TcpHeader) -> String {
    format!(
        "{}{}{}{}",
        if tcp_header.syn { "SYN " } else { "" },
        if tcp_header.ack { "ACK " } else { "" },
        if tcp_header.fin { "FIN " } else { "" },
        if tcp_header.rst { "RST " } else { "" },
    )
}
