mod body;

use anyhow::{Context, Result};
use bytes::Bytes;
use common::{InterceptMethod, InterceptedTrafficEntry, TrafficDirection};
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, mpsc};
use tokio::time::{Duration, timeout};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

use super::certificate::CertificateAuthority;
use body::{decompress_body, decompress_grpc_payload};

/// Configuration for the intercept proxy
pub struct ProxyConfig {
    /// Domains to intercept (extract and log traffic) - dynamically updatable
    pub intercept_domains: Arc<RwLock<HashSet<String>>>,
    /// Mapping of domain to agent short name
    pub domain_to_agent: HashMap<String, String>,
    /// Mapping of domain to URL regex pattern (if any)
    /// Uses fancy-regex to support lookahead/lookbehind for negation
    pub domain_to_url_pattern: HashMap<String, fancy_regex::Regex>,
    /// Node ID for traffic entries
    pub node_id: String,
    /// Interception method used
    pub intercept_method: InterceptMethod,
    /// Pre-resolved IPs for domains (used in Hosts mode to bypass hosts file redirection)
    pub domain_to_real_ip: HashMap<String, std::net::IpAddr>,
}

/// The intercept proxy server
pub struct InterceptProxy {
    /// Primary port the proxy is listening on (443 for Hosts, random for others)
    port: u16,
    /// Shared cancellation token for all listeners.
    shutdown_token: CancellationToken,
    /// Handle to the proxy task
    task_handle: Option<tokio::task::JoinHandle<()>>,
    /// Additional task handles for extra listeners (e.g., port 80 for Hosts mode)
    extra_task_handles: Vec<tokio::task::JoinHandle<()>>,
}

impl InterceptProxy {
    /// Start the intercept proxy server
    pub async fn start(
        ca: Arc<RwLock<CertificateAuthority>>,
        config: ProxyConfig,
        traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
    ) -> Result<Self> {
        let shutdown_token = CancellationToken::new();
        let config = Arc::new(config);
        let mut extra_task_handles = Vec::new();

        //
        // For Hosts mode, we need to listen on ports 443 (HTTPS) and 80 (HTTP)
        // since the hosts file redirects domains to 127.0.0.1.
        //
        let (listener, port) = if config.intercept_method == InterceptMethod::Hosts {
            //
            // Try to bind to port 443 for HTTPS.
            //
            let https_listener = TcpListener::bind("127.0.0.1:443").await
                .context("Failed to bind to port 443. Hosts-based interception requires running as root/administrator.")?;

            common::log_info!("Intercept proxy (Hosts mode) listening on port 443");

            //
            // Also try to bind to port 80 for HTTP (best effort).
            //
            match TcpListener::bind("127.0.0.1:80").await {
                Ok(http_listener) => {
                    common::log_info!("Intercept proxy (Hosts mode) also listening on port 80");
                    let ca_clone = Arc::clone(&ca);
                    let config_clone = Arc::clone(&config);
                    let traffic_tx_clone = traffic_tx.clone();
                    let http_shutdown = shutdown_token.clone();

                    //
                    // Spawn a separate task for the HTTP listener.
                    //
                    let http_task = tokio::spawn(run_proxy_http(
                        http_listener,
                        ca_clone,
                        config_clone,
                        traffic_tx_clone,
                        http_shutdown,
                    ));
                    extra_task_handles.push(http_task);
                }
                Err(e) => {
                    common::log_warn!(
                        "Could not bind to port 80 (HTTP): {}. Only HTTPS interception will work.",
                        e
                    );
                }
            }

            (https_listener, 443)
        } else if config.intercept_method == InterceptMethod::Proxy {
            //
            // For Proxy mode, bind to localhost only since system proxy routes
            // to localhost. This avoids triggering the Windows Firewall prompt.
            //

            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let port = listener.local_addr()?.port();
            common::log_info!("Intercept proxy (Proxy mode) starting on port {}", port);
            (listener, port)
        } else if config.intercept_method == InterceptMethod::Tproxy {
            //
            // For TPROXY mode (Linux), use a transparent socket that can accept
            // connections destined for any IP address. We use SO_ORIGINAL_DST
            // to get the real destination.
            //

            #[cfg(target_os = "linux")]
            {
                let addr = "127.0.0.1:0";
                let std_listener = super::tproxy::create_transparent_listener(addr)
                    .context("Failed to create transparent listener")?;
                let port = std_listener.local_addr()?.port();
                let listener = TcpListener::from_std(std_listener)?;
                common::log_info!("Intercept proxy (TPROXY mode) starting on port {}", port);
                (listener, port)
            }
            #[cfg(not(target_os = "linux"))]
            {
                anyhow::bail!("TPROXY mode is only supported on Linux");
            }
        } else {
            //
            // For VPN/TUN mode, bind to all interfaces since TUN adapter traffic
            // comes from a different interface.
            //

            let listener = TcpListener::bind("0.0.0.0:0").await?;
            let port = listener.local_addr()?.port();
            common::log_info!("Intercept proxy (VPN mode) starting on port {}", port);
            (listener, port)
        };

        let task_handle = tokio::spawn(run_proxy(
            listener,
            ca,
            config,
            traffic_tx,
            shutdown_token.clone(),
        ));

        Ok(Self {
            port,
            shutdown_token,
            task_handle: Some(task_handle),
            extra_task_handles,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn stop(&mut self) {
        self.shutdown_token.cancel();
        if let Some(handle) = self.task_handle.take() {
            let _ = handle.await;
        }
        for handle in self.extra_task_handles.drain(..) {
            handle.abort();
        }
    }
}

impl Drop for InterceptProxy {
    fn drop(&mut self) {
        self.shutdown_token.cancel();
        for handle in &self.extra_task_handles {
            handle.abort();
        }
    }
}

/// Run the proxy server main loop
async fn run_proxy(
    listener: TcpListener,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
    shutdown: CancellationToken,
) {
    {
        let domains = config.intercept_domains.read().await;
        common::log_info!("Proxy server running, intercepting domains: {:?}", *domains);
    }

    accept_loop(listener, ca, config, traffic_tx, shutdown, "Proxy server").await;
}

/// Run the HTTP proxy server (port 80) for Hosts mode.
///
/// This handles plain HTTP connections which are less common for AI APIs
/// but may be needed for some services.
async fn run_proxy_http(
    listener: TcpListener,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
    shutdown: CancellationToken,
) {
    common::log_info!("HTTP proxy server running on port 80 (Hosts mode)");

    accept_loop(
        listener,
        ca,
        config,
        traffic_tx,
        shutdown,
        "HTTP proxy server",
    )
    .await;
}

//
// Shared accept loop for the proxy listeners: accept connections and spawn
// a handler per connection until the shutdown token is cancelled.
//

async fn accept_loop(
    listener: TcpListener,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
    shutdown: CancellationToken,
    label: &str,
) {
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                common::log_info!("{} shutting down", label);
                break;
            }
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        let ca = Arc::clone(&ca);
                        let config = Arc::clone(&config);
                        let traffic_tx = traffic_tx.clone();

                        tokio::spawn(async move {
                            let _ = handle_connection(stream, addr, ca, config, traffic_tx).await;
                        });
                    }
                    Err(e) => {
                        common::log_error!("{}: failed to accept connection: {}", label, e);
                    }
                }
            }
        }
    }
}

/// Handle a single client connection
async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<()> {
    //
    // Peek at first byte to detect TLS vs HTTP.
    //
    let mut peek_buf = [0u8; 1];
    stream
        .peek(&mut peek_buf)
        .await
        .context("Failed to peek connection")?;

    //
    // TLS handshake starts with 0x16 (ContentType.handshake).
    //
    if peek_buf[0] == 0x16 {
        handle_tls_connection(stream, addr, ca, config, traffic_tx).await
    } else {
        let io = TokioIo::new(stream);

        //
        // Serve the connection with HTTP/1.1.
        //
        let service = service_fn(move |req| {
            let ca = Arc::clone(&ca);
            let config = Arc::clone(&config);
            let traffic_tx = traffic_tx.clone();
            async move { handle_request(req, addr, ca, config, traffic_tx).await }
        });

        http1::Builder::new()
            .preserve_header_case(true)
            .title_case_headers(true)
            .serve_connection(io, service)
            .with_upgrades()
            .await
            .context("HTTP connection error")?;

        Ok(())
    }
}

/// Handle a direct TLS connection (VPN/TPROXY mode)
///
/// In VPN/TPROXY mode, clients connect directly with TLS, not via HTTP CONNECT.
/// We need to:
/// 1. Read ClientHello to extract SNI (for certificate selection)
/// 2. For TPROXY mode, use SO_ORIGINAL_DST to get real destination
/// 3. Perform TLS termination with our certificate
/// 4. Forward decrypted traffic to the real server
async fn handle_tls_connection(
    stream: TcpStream,
    _addr: SocketAddr,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<()> {
    #![allow(unused_imports)]
    use tokio::io::AsyncReadExt;

    //
    // For TPROXY mode, get the original destination using SO_ORIGINAL_DST.
    //

    #[cfg(target_os = "linux")]
    let original_dst = if config.intercept_method == InterceptMethod::Tproxy {
        match super::tproxy::get_original_dst(&stream) {
            Ok(addr) => {
                common::log_debug!("TPROXY: Original destination: {}", addr);
                Some(addr)
            }
            Err(e) => {
                common::log_warn!(
                    "Failed to get original destination via SO_ORIGINAL_DST: {}",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    #[cfg(not(target_os = "linux"))]
    let original_dst: Option<SocketAddr> = None;

    //
    // Read enough bytes to parse ClientHello and extract SNI.
    //
    let mut client_hello_buf = vec![0u8; 4096];
    let n = stream
        .peek(&mut client_hello_buf)
        .await
        .context("Failed to peek ClientHello")?;

    //
    // Parse SNI from ClientHello.
    //
    let sni = parse_sni_from_client_hello(&client_hello_buf[..n])
        .context("Failed to parse SNI from ClientHello")?;

    //
    // Determine the actual destination port (from SO_ORIGINAL_DST or default 443).
    //
    let dest_port = original_dst.map(|a| a.port()).unwrap_or(443);

    //
    // Check if this domain should be intercepted.
    //
    let should_intercept = {
        let domains = config.intercept_domains.read().await;
        domains
            .iter()
            .any(|d| sni == *d || sni.ends_with(&format!(".{}", d)))
    };

    if !should_intercept {
        //
        // Non-intercepted domain reached proxy (likely shares IP with intercepted domain).
        // Tunnel through without TLS termination.
        //
        common::log_info!("Passthrough for non-intercepted domain {}", sni);

        let pre_resolved_ip = config.domain_to_real_ip.get(&sni).copied();
        let server = connect_bypass_tun(&sni, dest_port, pre_resolved_ip, config.intercept_method)
            .await
            .context(format!("Failed to connect to {} for passthrough", sni))?;

        //
        // Tunnel bytes bidirectionally. Since we used peek() for ClientHello,
        // it's still in the stream buffer and will be sent to the server.
        //
        let (mut client_read, mut client_write) = tokio::io::split(stream);
        let (mut server_read, mut server_write) = tokio::io::split(server);

        let client_to_server = tokio::io::copy(&mut client_read, &mut server_write);
        let server_to_client = tokio::io::copy(&mut server_read, &mut client_write);

        tokio::select! {
            result = client_to_server => {
                if let Err(e) = result {
                    common::log_debug!("Passthrough {} client->server ended: {}", sni, e);
                }
            }
            result = server_to_client => {
                if let Err(e) = result {
                    common::log_debug!("Passthrough {} server->client ended: {}", sni, e);
                }
            }
        }

        return Ok(());
    }

    //
    // Get or generate certificate for this domain.
    //
    let acceptor = {
        let mut ca_guard = ca.write().await;
        if ca_guard.get_leaf_cert(&sni).is_none() {
            ca_guard
                .generate_leaf_cert(&sni)
                .context("Failed to generate leaf certificate")?;
        }
        create_tls_acceptor(&ca_guard, &sni)?
    };

    //
    // Perform TLS handshake with client.
    //
    let tls_stream = acceptor
        .accept(stream)
        .await
        .context("TLS handshake with client failed")?;

    //
    // Now handle the decrypted traffic similar to CONNECT tunnel.
    //
    handle_intercepted_tunnel_vpn(tls_stream, &sni, dest_port, config, traffic_tx).await
}

/// Parse SNI (Server Name Indication) from a TLS ClientHello message
fn parse_sni_from_client_hello(data: &[u8]) -> Result<String> {
    //
    // TLS record header: ContentType(1) + Version(2) + Length(2).
    //
    if data.len() < 5 {
        anyhow::bail!("Data too short for TLS record header");
    }

    if data[0] != 0x16 {
        anyhow::bail!("Not a TLS handshake record");
    }

    let record_length = u16::from_be_bytes([data[3], data[4]]) as usize;
    if data.len() < 5 + record_length {
        anyhow::bail!("Incomplete TLS record");
    }

    //
    // Handshake header: HandshakeType(1) + Length(3).
    //
    let handshake = &data[5..];
    if handshake.is_empty() || handshake[0] != 0x01 {
        anyhow::bail!("Not a ClientHello message");
    }

    //
    // Skip handshake header (4 bytes) + client version (2) + random (32).
    //
    let mut pos = 4 + 2 + 32;

    if pos >= handshake.len() {
        anyhow::bail!("ClientHello too short");
    }

    //
    // Skip session ID.
    //
    let session_id_len = handshake[pos] as usize;
    pos += 1 + session_id_len;

    if pos + 2 > handshake.len() {
        anyhow::bail!("ClientHello too short for cipher suites");
    }

    //
    // Skip cipher suites.
    //
    let cipher_suites_len = u16::from_be_bytes([handshake[pos], handshake[pos + 1]]) as usize;
    pos += 2 + cipher_suites_len;

    if pos + 1 > handshake.len() {
        anyhow::bail!("ClientHello too short for compression methods");
    }

    //
    // Skip compression methods.
    //
    let compression_len = handshake[pos] as usize;
    pos += 1 + compression_len;

    if pos + 2 > handshake.len() {
        anyhow::bail!("No extensions in ClientHello");
    }

    //
    // Extensions length.
    //
    let extensions_len = u16::from_be_bytes([handshake[pos], handshake[pos + 1]]) as usize;
    pos += 2;

    let extensions_end = pos + extensions_len;

    //
    // Parse extensions looking for SNI (type 0x0000).
    //
    while pos + 4 <= extensions_end && pos + 4 <= handshake.len() {
        let ext_type = u16::from_be_bytes([handshake[pos], handshake[pos + 1]]);
        let ext_len = u16::from_be_bytes([handshake[pos + 2], handshake[pos + 3]]) as usize;
        pos += 4;

        if ext_type == 0x0000 {
            //
            // SNI extension.
            //
            if pos + ext_len > handshake.len() {
                anyhow::bail!("SNI extension truncated");
            }

            //
            // SNI list length (2 bytes).
            //
            if ext_len < 2 {
                anyhow::bail!("SNI extension too short");
            }

            //
            // Skip list length.
            //
            let mut sni_pos = pos + 2;

            //
            // Parse SNI entries.
            //
            while sni_pos + 3 <= pos + ext_len {
                let name_type = handshake[sni_pos];
                let name_len =
                    u16::from_be_bytes([handshake[sni_pos + 1], handshake[sni_pos + 2]]) as usize;
                sni_pos += 3;

                if name_type == 0x00 && sni_pos + name_len <= handshake.len() {
                    //
                    // Host name type.
                    //
                    let sni = std::str::from_utf8(&handshake[sni_pos..sni_pos + name_len])
                        .context("Invalid SNI hostname")?;
                    return Ok(sni.to_string());
                }

                sni_pos += name_len;
            }
        }

        pos += ext_len;
    }

    anyhow::bail!("No SNI extension found in ClientHello")
}

/// Connect to a server bypassing TUN routing, TPROXY iptables, and hosts file
///
/// In VPN mode, we route intercept IPs through the TUN interface. But when the
/// proxy needs to connect to the real server, that traffic must bypass the TUN
/// to avoid a routing loop. We use SO_BINDTODEVICE to force traffic through
/// the real interface.
///
/// In TPROXY mode, iptables rules intercept traffic to certain IPs. The proxy's
/// own outbound connections must be marked with TPROXY_BYPASS_MARK so the
/// iptables bypass rule skips them and prevents a routing loop.
///
/// In Hosts mode, the hosts file redirects domains to 127.0.0.1. We must use
/// pre-resolved IPs to avoid connecting back to ourselves.
async fn connect_bypass_tun(
    host: &str,
    port: u16,
    pre_resolved_ip: Option<std::net::IpAddr>,
    intercept_method: InterceptMethod,
) -> Result<TcpStream> {
    use socket2::{Domain, Protocol, Socket, Type};
    use std::net::ToSocketAddrs;

    //
    // Use pre-resolved IP if available (for Hosts mode), otherwise resolve via DNS.
    //
    let addr = if let Some(ip) = pre_resolved_ip {
        common::log_debug!("Using pre-resolved IP {} for {}", ip, host);
        std::net::SocketAddr::new(ip, port)
    } else {
        let target = format!("{}:{}", host, port);
        target
            .to_socket_addrs()
            .context("Failed to resolve target address")?
            .next()
            .context("No addresses found for target")?
    };

    //
    // Create socket.
    //
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))
        .context("Failed to create socket")?;

    //
    // Apply bypass mechanisms based on intercept mode:
    // - VPN: SO_MARK + SO_BINDTODEVICE (bypass TUN routing)
    // - TPROXY: SO_MARK only (bypass iptables rules)
    // - Hosts: nothing needed (uses pre-resolved IPs)
    //
    #[cfg(target_os = "linux")]
    match intercept_method {
        InterceptMethod::Vpn => {
            //
            // VPN mode: Set SO_MARK for policy routing and SO_BINDTODEVICE
            // to force traffic through the real network interface.
            //
            if let Err(e) = socket.set_mark(super::routing::VPN_BYPASS_MARK) {
                common::log_warn!("Failed to set SO_MARK: {} (may need CAP_NET_ADMIN)", e);
            }
            if let Some(iface) = discover_default_interface() {
                common::log_debug!("VPN bypass: binding to interface {}", iface);
                if let Err(e) = socket.bind_device(Some(iface.as_bytes())) {
                    common::log_warn!(
                        "Failed to bind to device {}: {} (may need CAP_NET_ADMIN)",
                        iface,
                        e
                    );
                }
            }
        }
        InterceptMethod::Tproxy => {
            //
            // TPROXY mode: Only need SO_MARK so the iptables bypass rule
            // (-m mark --mark 0x2 -j RETURN) skips our outbound packets.
            //
            common::log_debug!("TPROXY bypass: setting SO_MARK=0x2");
            if let Err(e) = socket.set_mark(super::tproxy::TPROXY_BYPASS_MARK) {
                common::log_warn!("Failed to set SO_MARK: {} (may need CAP_NET_ADMIN)", e);
            }
        }
        _ => {
            //
            // Hosts/Proxy modes don't need special socket options.
            //
        }
    }

    //
    // Windows VPN bypass: Bind to the main interface's IP so packets have a
    // source IP != TUN IP (10.255.0.1). The packet engine checks is_from_tun
    // and passes through traffic from other source IPs.
    //
    #[cfg(target_os = "windows")]
    if intercept_method == InterceptMethod::Vpn {
        if let Some(bind_ip) = discover_non_tun_ip() {
            common::log_debug!("Windows VPN bypass: binding to {}", bind_ip);
            let bind_addr = std::net::SocketAddr::new(bind_ip, 0);
            if let Err(e) = socket.bind(&bind_addr.into()) {
                common::log_warn!("Failed to bind to {}: {}", bind_ip, e);
            }
        } else {
            common::log_warn!("Could not find non-TUN IP for VPN bypass");
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let _ = intercept_method; // Silence unused variable warning

    socket
        .set_nonblocking(true)
        .context("Failed to set non-blocking")?;

    //
    // Connect (non-blocking) - in-progress errors are expected.
    //
    common::log_debug!("connect_bypass_tun: connecting to {}", addr);
    match socket.connect(&addr.into()) {
        Ok(()) => {
            common::log_debug!("connect_bypass_tun: connect() returned Ok");
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            common::log_debug!("connect_bypass_tun: connect() returned WouldBlock (expected)");
        }
        //
        // WSAEWOULDBLOCK (Windows).
        //
        Err(e) if e.raw_os_error() == Some(10035) => {
            common::log_debug!("connect_bypass_tun: connect() returned WSAEWOULDBLOCK (expected)");
        }
        //
        // EINPROGRESS (Linux).
        //
        Err(e) if e.raw_os_error() == Some(115) => {
            common::log_debug!("connect_bypass_tun: connect() returned EINPROGRESS (expected)");
        }
        //
        // EINPROGRESS (macOS).
        //
        Err(e) if e.raw_os_error() == Some(36) => {
            common::log_debug!(
                "connect_bypass_tun: connect() returned EINPROGRESS macOS (expected)"
            );
        }
        Err(e) => {
            common::log_error!(
                "connect_bypass_tun: connect() failed: {} (os_error={:?})",
                e,
                e.raw_os_error()
            );
            return Err(e).context("Failed to connect");
        }
    }

    //
    // Convert to tokio TcpStream.
    //
    let std_stream: std::net::TcpStream = socket.into();
    let stream = TcpStream::from_std(std_stream).context("Failed to convert to tokio stream")?;

    //
    // Wait for connection to complete.
    //
    common::log_debug!("connect_bypass_tun: waiting for connection to {}", addr);
    stream
        .writable()
        .await
        .context("Failed to wait for connection")?;

    //
    // Check for connection errors.
    //
    if let Some(e) = stream.take_error()? {
        common::log_debug!("connect_bypass_tun: connection to {} failed: {}", addr, e);
        return Err(e).context("Connection failed");
    }

    common::log_debug!("connect_bypass_tun: connected to {}", addr);
    Ok(stream)
}

/// Handle intercepted TLS tunnel for VPN mode
///
/// Takes an already-established TLS connection with the client,
/// connects to the real server with TLS, and proxies traffic.
async fn handle_intercepted_tunnel_vpn(
    client_tls: tokio_rustls::server::TlsStream<TcpStream>,
    host: &str,
    port: u16,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<()> {
    //
    // For Hosts mode, use pre-resolved IP to avoid hosts file loop.
    //
    let pre_resolved_ip = config.domain_to_real_ip.get(host).copied();

    //
    // Connect to real server, bypassing TUN routing and hosts file.
    //
    common::log_debug!(
        "handle_intercepted_tunnel_vpn: connecting to {}:{}",
        host,
        port
    );
    let server_tcp = connect_bypass_tun(host, port, pre_resolved_ip, config.intercept_method)
        .await
        .context(format!("Failed to connect to {}:{}", host, port))?;
    common::log_debug!(
        "handle_intercepted_tunnel_vpn: TCP connected to {}:{}",
        host,
        port
    );

    //
    // Create TLS connector for server.
    //
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let server_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = tokio_rustls::TlsConnector::from(Arc::new(server_config));
    let server_name = rustls_pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| anyhow::anyhow!("Invalid server name"))?;

    common::log_debug!("handle_intercepted_tunnel_vpn: starting TLS to {}", host);
    let server_tls = connector
        .connect(server_name, server_tcp)
        .await
        .context("Failed to establish TLS with server")?;
    common::log_debug!("handle_intercepted_tunnel_vpn: TLS established to {}", host);

    //
    // Now proxy HTTP traffic over the TLS connections.
    //
    proxy_https_traffic(client_tls, server_tls, host, &config, &traffic_tx).await
}

/// Handle an individual HTTP request
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    _addr: SocketAddr,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    if req.method() == Method::CONNECT {
        //
        // Handle HTTPS CONNECT tunnel.
        //
        handle_connect(req, ca, config, traffic_tx).await
    } else {
        //
        // Handle plain HTTP request (forward as-is).
        //
        handle_http_request(req, config, traffic_tx).await
    }
}

/// Handle HTTP CONNECT request for HTTPS tunneling
async fn handle_connect(
    req: Request<hyper::body::Incoming>,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let host = match req.uri().host() {
        Some(h) => h.to_string(),
        None => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from("Invalid host")))
                .unwrap());
        }
    };

    let port = req.uri().port_u16().unwrap_or(443);

    //
    // Check if this domain should be intercepted.
    //
    let should_intercept = {
        let domains = config.intercept_domains.read().await;
        domains
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{}", d)))
    };

    //
    // Establish tunnel to the target server.
    //
    tokio::task::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let _ = tunnel(
                    upgraded,
                    &host,
                    port,
                    should_intercept,
                    ca,
                    &config,
                    &traffic_tx,
                )
                .await;
            }
            Err(e) => {
                common::log_warn!("Upgrade error: {}", e);
            }
        }
    });

    Ok(Response::new(Full::new(Bytes::new())))
}

/// Tunnel traffic between client and server
async fn tunnel(
    upgraded: hyper::upgrade::Upgraded,
    host: &str,
    port: u16,
    should_intercept: bool,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: &ProxyConfig,
    traffic_tx: &mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<()> {
    let target = format!("{}:{}", host, port);

    if should_intercept {
        //
        // Full MITM: decrypt, log, re-encrypt.
        //
        intercept_tls_traffic(upgraded, host, port, ca, config, traffic_tx).await
    } else {
        //
        // Simple passthrough for non-intercepted domains.
        //
        let mut server = TcpStream::connect(&target)
            .await
            .context(format!("Failed to connect to {}", target))?;
        let mut upgraded = TokioIo::new(upgraded);

        let _ = tokio::io::copy_bidirectional(&mut upgraded, &mut server).await;
        Ok(())
    }
}

/// Perform TLS interception (MITM) on the connection
async fn intercept_tls_traffic(
    upgraded: hyper::upgrade::Upgraded,
    host: &str,
    port: u16,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: &ProxyConfig,
    traffic_tx: &mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<()> {
    //
    // Get leaf certificate for this domain.
    //
    let (cert_pem, key_pem) = {
        let mut ca_guard = ca.write().await;
        let cert_data = ca_guard
            .generate_leaf_cert(host)
            .context("Failed to generate leaf certificate")?;
        (cert_data.cert_pem.clone(), cert_data.key_pem.clone())
    };

    //
    // Create TLS acceptor for client connection.
    //
    let tls_acceptor = create_tls_acceptor_from_pem(&cert_pem, &key_pem)
        .context("Failed to create TLS acceptor")?;

    //
    // Accept TLS from client.
    //
    let upgraded_io = TokioIo::new(upgraded);
    let client_tls = match tls_acceptor.accept(upgraded_io).await {
        Ok(stream) => stream,
        Err(e) => {
            common::log_error!("TLS handshake failed with client for {}: {:?}", host, e);
            common::log_error!("  This may indicate the client doesn't trust our root CA");
            return Err(anyhow::anyhow!("Failed to accept TLS from client: {}", e));
        }
    };

    //
    // Connect to real server with TLS.
    //
    let target = format!("{}:{}", host, port);
    let server_tcp = TcpStream::connect(&target)
        .await
        .context(format!("Failed to connect to {}", target))?;

    //
    // Create TLS connector for server.
    //
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let server_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = tokio_rustls::TlsConnector::from(Arc::new(server_config));
    let server_name = rustls_pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| anyhow::anyhow!("Invalid server name"))?;

    let server_tls = connector
        .connect(server_name, server_tcp)
        .await
        .context("Failed to establish TLS with server")?;

    //
    // Now proxy HTTP traffic over the TLS connections.
    //
    proxy_https_traffic(client_tls, server_tls, host, config, traffic_tx).await
}

//
// HTTP/2 connection preface: "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"
// We only need to check the first 4 bytes "PRI " to detect HTTP/2.
//

const HTTP2_PREFACE_PREFIX: &[u8] = b"PRI ";

//
// HTTP/2 frame types (RFC 7540 Section 6).
//

const H2_FRAME_DATA: u8 = 0x0;
const H2_FRAME_HEADERS: u8 = 0x1;
const H2_FRAME_PRIORITY: u8 = 0x2;
const H2_FRAME_RST_STREAM: u8 = 0x3;
const H2_FRAME_SETTINGS: u8 = 0x4;
const H2_FRAME_PUSH_PROMISE: u8 = 0x5;
const H2_FRAME_PING: u8 = 0x6;
const H2_FRAME_GOAWAY: u8 = 0x7;
const H2_FRAME_WINDOW_UPDATE: u8 = 0x8;
const H2_FRAME_CONTINUATION: u8 = 0x9;

//
// Wrapper stream that prepends buffered bytes before the inner stream.
// Used to replay peeked bytes when delegating to h2 or HTTP/1.1 handlers.
//

struct PrefixedStream<S> {
    prefix: Vec<u8>,
    prefix_pos: usize,
    inner: S,
}

impl<S> PrefixedStream<S> {
    fn new(prefix: Vec<u8>, inner: S) -> Self {
        Self {
            prefix,
            prefix_pos: 0,
            inner,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for PrefixedStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        //
        // First return any remaining prefix bytes.
        //

        if self.prefix_pos < self.prefix.len() {
            let remaining = &self.prefix[self.prefix_pos..];
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            self.prefix_pos += to_copy;
            return Poll::Ready(Ok(()));
        }

        //
        // Then delegate to inner stream.
        //

        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for PrefixedStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

//
// HTTP/2 connection preface that must be sent to server.
//

const HTTP2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

//
// Proxy HTTP/2 traffic between client and server with frame-level interception.
// Forwards all frames bidirectionally while logging HEADERS and DATA frames.
//

async fn proxy_h2_traffic<C, S>(
    client_stream: C,
    mut server_stream: S,
    host: &str,
    config: &ProxyConfig,
    traffic_tx: &mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<()>
where
    C: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio::io::AsyncWriteExt;

    //
    // Forward the HTTP/2 preface to the server.
    // The preface was read from the client for detection but not yet sent to server.
    //

    server_stream.write_all(HTTP2_PREFACE).await?;
    server_stream.flush().await?;
    common::log_debug!("Forwarded HTTP/2 preface to server for {}", host);

    let (client_read, client_write) = tokio::io::split(client_stream);
    let (server_read, server_write) = tokio::io::split(server_stream);

    let agent = config
        .domain_to_agent
        .get(host)
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let url_pattern = config.domain_to_url_pattern.get(host);

    common::log_info!("HTTP/2 interception for {} (agent={})", host, agent);

    handle_h2_traffic(
        client_read,
        client_write,
        server_read,
        server_write,
        host,
        &agent,
        &config.node_id,
        config.intercept_method,
        url_pattern,
        traffic_tx,
    )
    .await
}

//
// HTTP/2 frame structure (RFC 7540 Section 4.1):
// +-----------------------------------------------+
// |                 Length (24)                   |
// +---------------+---------------+---------------+
// |   Type (8)    |   Flags (8)   |
// +-+-------------+---------------+-------------------------------+
// |R|                 Stream Identifier (31)                      |
// +=+=============================================================+
// |                   Frame Payload (0...)                      ...
// +---------------------------------------------------------------+
//

#[derive(Debug, Clone)]
struct H2Frame {
    frame_type: u8,
    flags: u8,
    stream_id: u32,
    payload: Vec<u8>,
}

impl H2Frame {
    fn type_name(&self) -> &'static str {
        match self.frame_type {
            H2_FRAME_DATA => "DATA",
            H2_FRAME_HEADERS => "HEADERS",
            H2_FRAME_PRIORITY => "PRIORITY",
            H2_FRAME_RST_STREAM => "RST_STREAM",
            H2_FRAME_SETTINGS => "SETTINGS",
            H2_FRAME_PUSH_PROMISE => "PUSH_PROMISE",
            H2_FRAME_PING => "PING",
            H2_FRAME_GOAWAY => "GOAWAY",
            H2_FRAME_WINDOW_UPDATE => "WINDOW_UPDATE",
            H2_FRAME_CONTINUATION => "CONTINUATION",
            _ => "UNKNOWN",
        }
    }
}

/// Read an HTTP/2 frame from the stream.
async fn read_h2_frame<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> Result<Option<H2Frame>> {
    use tokio::io::AsyncReadExt;

    //
    // Read 9-byte frame header.
    //

    let mut header = [0u8; 9];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }

    //
    // Parse header fields.
    //

    let length = ((header[0] as u32) << 16) | ((header[1] as u32) << 8) | (header[2] as u32);
    let frame_type = header[3];
    let flags = header[4];
    let stream_id = ((header[5] as u32 & 0x7F) << 24)
        | ((header[6] as u32) << 16)
        | ((header[7] as u32) << 8)
        | (header[8] as u32);

    //
    // Read payload.
    //

    let mut payload = vec![0u8; length as usize];
    if length > 0 {
        reader.read_exact(&mut payload).await?;
    }

    Ok(Some(H2Frame {
        frame_type,
        flags,
        stream_id,
        payload,
    }))
}

/// Write an HTTP/2 frame to the stream.
async fn write_h2_frame<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &H2Frame,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let length = frame.payload.len() as u32;

    //
    // Build 9-byte frame header.
    //

    let header = [
        ((length >> 16) & 0xFF) as u8,
        ((length >> 8) & 0xFF) as u8,
        (length & 0xFF) as u8,
        frame.frame_type,
        frame.flags,
        ((frame.stream_id >> 24) & 0x7F) as u8,
        ((frame.stream_id >> 16) & 0xFF) as u8,
        ((frame.stream_id >> 8) & 0xFF) as u8,
        (frame.stream_id & 0xFF) as u8,
    ];

    writer.write_all(&header).await?;
    if !frame.payload.is_empty() {
        writer.write_all(&frame.payload).await?;
    }
    writer.flush().await?;

    Ok(())
}

/// Handle HTTP/2 traffic with frame-level interception.
async fn handle_h2_traffic<CR, CW, SR, SW>(
    mut client_read: CR,
    mut client_write: CW,
    mut server_read: SR,
    mut server_write: SW,
    host: &str,
    agent: &str,
    node_id: &str,
    intercept_method: InterceptMethod,
    url_pattern: Option<&fancy_regex::Regex>,
    traffic_tx: &mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<()>
where
    CR: tokio::io::AsyncRead + Unpin + Send,
    CW: tokio::io::AsyncWrite + Unpin + Send,
    SR: tokio::io::AsyncRead + Unpin + Send,
    SW: tokio::io::AsyncWrite + Unpin + Send,
{
    let host = host.to_string();
    let agent = agent.to_string();
    let node_id = node_id.to_string();

    //
    // Track stream paths for logging context (stream_id -> path).
    //

    let mut stream_paths: std::collections::HashMap<u32, String> = std::collections::HashMap::new();

    //
    // Use tokio::select! to handle bidirectional traffic.
    //

    loop {
        tokio::select! {
            biased;

            //
            // Read frame from server, forward to client.
            //

            result = read_h2_frame(&mut server_read) => {
                match result {
                    Ok(Some(frame)) => {
                        common::log_debug!(
                            "H2 server->client: {} stream={} flags={:#x} len={}",
                            frame.type_name(), frame.stream_id, frame.flags, frame.payload.len()
                        );

                        //
                        // Forward to client.
                        //

                        if write_h2_frame(&mut client_write, &frame).await.is_err() {
                            break;
                        }

                        //
                        // Log DATA frames (response body).
                        //

                        if frame.frame_type == H2_FRAME_DATA && !frame.payload.is_empty() {
                            let path = stream_paths
                                .get(&frame.stream_id)
                                .cloned()
                                .unwrap_or_else(|| format!("/stream/{}", frame.stream_id));
                            let url = format!("https://{}{}", host, path);

                            let should_collect = match url_pattern {
                                Some(pattern) => pattern.is_match(&url).unwrap_or(true),
                                None => true,
                            };

                            if should_collect {
                                //
                                // Decompress gRPC payload for readability.
                                //

                                let decompressed = decompress_grpc_payload(&frame.payload);

                                let entry = InterceptedTrafficEntry {
                                    id: None,
                                    timestamp: chrono::Utc::now(),
                                    node_id: node_id.clone(),
                                    agent_short_name: agent.clone(),
                                    intercept_method,
                                    direction: TrafficDirection::Receive,
                                    method: Some("H2_DATA".to_string()),
                                    url: url.clone(),
                                    host: host.clone(),
                                    request_headers: None,
                                    request_body: None,
                                    response_status: None,
                                    response_headers: None,
                                    response_body: Some(decompressed),
                                };
                                let _ = traffic_tx.try_send(entry);
                            }
                        }

                        //
                        // Log HEADERS frames (response headers).
                        //

                        if frame.frame_type == H2_FRAME_HEADERS && !frame.payload.is_empty() {
                            let path = stream_paths
                                .get(&frame.stream_id)
                                .cloned()
                                .unwrap_or_else(|| format!("/stream/{}", frame.stream_id));
                            let url = format!("https://{}{}", host, path);

                            let should_collect = match url_pattern {
                                Some(pattern) => pattern.is_match(&url).unwrap_or(true),
                                None => true,
                            };

                            if should_collect {
                                let entry = InterceptedTrafficEntry {
                                    id: None,
                                    timestamp: chrono::Utc::now(),
                                    node_id: node_id.clone(),
                                    agent_short_name: agent.clone(),
                                    intercept_method,
                                    direction: TrafficDirection::Receive,
                                    method: Some("H2_HEADERS".to_string()),
                                    url: url.clone(),
                                    host: host.clone(),
                                    request_headers: None,
                                    request_body: None,
                                    response_status: None,
                                    response_headers: None,
                                    response_body: Some(frame.payload.clone()),
                                };
                                let _ = traffic_tx.try_send(entry);
                            }
                        }

                        //
                        // Check for connection close.
                        //

                        if frame.frame_type == H2_FRAME_GOAWAY {
                            common::log_debug!("H2 GOAWAY from server, closing connection");
                            break;
                        }
                    }
                    Ok(None) | Err(_) => {
                        break;
                    }
                }
            }

            //
            // Read frame from client, forward to server.
            //

            result = read_h2_frame(&mut client_read) => {
                match result {
                    Ok(Some(frame)) => {
                        common::log_debug!(
                            "H2 client->server: {} stream={} flags={:#x} len={}",
                            frame.type_name(), frame.stream_id, frame.flags, frame.payload.len()
                        );

                        //
                        // Forward to server.
                        //

                        if write_h2_frame(&mut server_write, &frame).await.is_err() {
                            break;
                        }

                        //
                        // Extract path from HEADERS frames for stream tracking.
                        // HPACK-encoded headers contain the :path pseudo-header.
                        // We do a simple scan for common patterns.
                        //

                        if frame.frame_type == H2_FRAME_HEADERS && !frame.payload.is_empty() {
                            if let Some(path) = extract_path_from_headers(&frame.payload) {
                                stream_paths.insert(frame.stream_id, path.clone());
                            }

                            let path = stream_paths
                                .get(&frame.stream_id)
                                .cloned()
                                .unwrap_or_else(|| format!("/stream/{}", frame.stream_id));
                            let url = format!("https://{}{}", host, path);

                            let should_collect = match url_pattern {
                                Some(pattern) => pattern.is_match(&url).unwrap_or(true),
                                None => true,
                            };

                            if should_collect {
                                let entry = InterceptedTrafficEntry {
                                    id: None,
                                    timestamp: chrono::Utc::now(),
                                    node_id: node_id.clone(),
                                    agent_short_name: agent.clone(),
                                    intercept_method,
                                    direction: TrafficDirection::Send,
                                    method: Some("H2_HEADERS".to_string()),
                                    url: url.clone(),
                                    host: host.clone(),
                                    request_headers: None,
                                    request_body: Some(frame.payload.clone()),
                                    response_status: None,
                                    response_headers: None,
                                    response_body: None,
                                };
                                let _ = traffic_tx.try_send(entry);
                            }
                        }

                        //
                        // Log DATA frames (request body).
                        //

                        if frame.frame_type == H2_FRAME_DATA && !frame.payload.is_empty() {
                            let path = stream_paths
                                .get(&frame.stream_id)
                                .cloned()
                                .unwrap_or_else(|| format!("/stream/{}", frame.stream_id));
                            let url = format!("https://{}{}", host, path);

                            let should_collect = match url_pattern {
                                Some(pattern) => pattern.is_match(&url).unwrap_or(true),
                                None => true,
                            };

                            if should_collect {
                                //
                                // Decompress gRPC payload for readability.
                                //

                                let decompressed = decompress_grpc_payload(&frame.payload);

                                let entry = InterceptedTrafficEntry {
                                    id: None,
                                    timestamp: chrono::Utc::now(),
                                    node_id: node_id.clone(),
                                    agent_short_name: agent.clone(),
                                    intercept_method,
                                    direction: TrafficDirection::Send,
                                    method: Some("H2_DATA".to_string()),
                                    url: url.clone(),
                                    host: host.clone(),
                                    request_headers: None,
                                    request_body: Some(decompressed),
                                    response_status: None,
                                    response_headers: None,
                                    response_body: None,
                                };
                                let _ = traffic_tx.try_send(entry);
                            }
                        }

                        //
                        // Check for connection close.
                        //

                        if frame.frame_type == H2_FRAME_GOAWAY {
                            common::log_debug!("H2 GOAWAY from client, closing connection");
                            break;
                        }
                    }
                    Ok(None) | Err(_) => {
                        break;
                    }
                }
            }
        }
    }

    common::log_info!("HTTP/2 connection closed for {}", host);
    Ok(())
}

//
// Extract :path from HPACK-encoded headers.
// This is a simplified extraction that looks for common patterns.
// Full HPACK decoding would require maintaining a dynamic table.
//

fn extract_path_from_headers(payload: &[u8]) -> Option<String> {
    //
    // Look for :path in the static table index or literal encoding.
    // Static table index 4 = :path /
    // Static table index 5 = :path /index.html
    // Literal header field with name ":path" has the name as bytes.
    //

    //
    // Simple heuristic: scan for ASCII path patterns starting with '/'.
    // This works for gRPC paths like "/service.v1.Service/Method".
    //

    let mut i = 0;
    while i < payload.len() {
        //
        // Look for a sequence that looks like a path: starts with '/' and
        // contains printable ASCII.
        //

        if payload[i] == b'/' {
            let start = i;
            while i < payload.len() && payload[i] >= 0x20 && payload[i] < 0x7F {
                i += 1;
            }
            if i > start + 1 {
                if let Ok(path) = std::str::from_utf8(&payload[start..i]) {
                    //
                    // Validate it looks like a path.
                    //

                    if path.starts_with('/') && !path.contains(' ') {
                        return Some(path.to_string());
                    }
                }
            }
        }
        i += 1;
    }

    None
}

/// Proxy HTTP traffic over TLS connections, logging requests and responses
async fn proxy_https_traffic<C, S>(
    mut client_tls: tokio_rustls::server::TlsStream<C>,
    server_tls: tokio_rustls::client::TlsStream<S>,
    host: &str,
    config: &ProxyConfig,
    traffic_tx: &mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<()>
where
    C: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    //
    // Read first bytes to detect HTTP/2 vs HTTP/1.1.
    // HTTP/2 starts with "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n" (24 bytes).
    // We only need to check the first 4 bytes "PRI ".
    //

    let mut peek_buf = [0u8; 24];
    let n = client_tls.read(&mut peek_buf).await?;
    let peeked = &peek_buf[..n];

    if n >= 4 && &peeked[..4] == HTTP2_PREFACE_PREFIX {
        //
        // HTTP/2 detected - delegate to HTTP/2 proxy.
        // The preface is exactly 24 bytes. Any bytes beyond that are the first
        // frame (client's SETTINGS) and should be passed to the frame relay.
        //

        common::log_info!("HTTP/2 detected for {}, using h2 proxy", host);

        //
        // Only pass bytes AFTER the preface to the frame handler.
        // The preface itself will be forwarded to the server by proxy_h2_traffic.
        //

        let extra_bytes = if n > 24 {
            peeked[24..].to_vec()
        } else {
            Vec::new()
        };
        let client_prefixed = PrefixedStream::new(extra_bytes, client_tls);
        return proxy_h2_traffic(client_prefixed, server_tls, host, config, traffic_tx).await;
    }

    //
    // HTTP/1.1 - continue with existing logic.
    // Prepend the peeked bytes back to the client stream.
    //

    common::log_debug!("proxy_https_traffic: HTTP/1.1 detected for {}", host);
    let client_prefixed = PrefixedStream::new(peeked.to_vec(), client_tls);

    let (client_read, mut client_write) = tokio::io::split(client_prefixed);
    let (server_read, mut server_write) = tokio::io::split(server_tls);

    let mut client_reader = BufReader::new(client_read);
    let mut server_reader = BufReader::new(server_read);

    let host = host.to_string();
    let config_node_id = config.node_id.clone();
    let agent = config
        .domain_to_agent
        .get(&host)
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let url_pattern = config.domain_to_url_pattern.get(&host);

    //
    // Process requests from client.
    //
    common::log_debug!("proxy_https_traffic: starting for {}", host);
    loop {
        //
        // Read HTTP request from client.
        //
        let mut request_line = String::new();
        match client_reader.read_line(&mut request_line).await {
            //
            // Connection closed.
            //
            Ok(0) => {
                common::log_debug!("proxy_https_traffic: client closed (0 bytes)");
                break;
            }
            Ok(n) => {
                common::log_debug!(
                    "proxy_https_traffic: read {} bytes: {:?}",
                    n,
                    request_line.trim()
                );
            }
            Err(e) => {
                common::log_warn!("proxy_https_traffic: read error: {}", e);
                break;
            }
        }

        if request_line.trim().is_empty() {
            continue;
        }

        //
        // Parse request line.
        //
        let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let method = parts[0].to_string();
        let path = parts[1].to_string();
        let url = format!("https://{}{}", host, path);

        //
        // Read headers - preserve original case for forwarding and logging.
        //
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut content_length: usize = 0;
        loop {
            let mut header_line = String::new();
            if client_reader.read_line(&mut header_line).await.is_err() {
                break;
            }
            let line = header_line.trim();
            if line.is_empty() {
                break;
            }
            if let Some((key, value)) = line.split_once(':') {
                let original_key = key.trim().to_string();
                let value = value.trim().to_string();
                if original_key.eq_ignore_ascii_case("content-length") {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.push((original_key, value));
            }
        }
        //
        // Convert to IndexMap for logging (preserves original order and case).
        //
        let headers_map: IndexMap<String, String> = headers.iter().cloned().collect();

        //
        // Read body if present.
        //
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            let _ = client_reader.read_exact(&mut body).await;
        }

        //
        // Forward request to server.
        //
        server_write.write_all(request_line.as_bytes()).await?;
        for (key, value) in &headers {
            server_write
                .write_all(format!("{}: {}\r\n", key, value).as_bytes())
                .await?;
        }
        server_write.write_all(b"\r\n").await?;
        if content_length > 0 {
            server_write.write_all(&body).await?;
        }
        server_write.flush().await?;

        //
        // Read response headers from server with timeout (30 seconds for
        // headers only).
        //
        const HEADER_TIMEOUT_SECS: u64 = 30;
        let headers_result = timeout(
            Duration::from_secs(HEADER_TIMEOUT_SECS),
            read_response_headers(&mut server_reader),
        )
        .await;

        let (response_line, status_code, response_headers, body_type) = match headers_result {
            Ok(Ok((line, status, headers, body_type))) => (line, status, headers, body_type),
            Ok(Err(e)) => {
                //
                // Error reading response headers.
                //
                common::log_warn!(
                    "Intercepted [NO RESPONSE]: {} {} - error: {}",
                    method,
                    url,
                    e
                );

                //
                // Record request without response if pattern matches.
                //
                let should_collect = match url_pattern {
                    Some(pattern) => pattern.is_match(&url).unwrap_or(true),
                    None => true,
                };
                if should_collect {
                    let entry = InterceptedTrafficEntry {
                        id: None,
                        timestamp: chrono::Utc::now(),
                        node_id: config_node_id.clone(),
                        agent_short_name: agent.clone(),
                        intercept_method: config.intercept_method,
                        direction: TrafficDirection::Send,
                        method: Some(method.clone()),
                        url: url.clone(),
                        host: host.clone(),
                        request_headers: Some(headers_map.clone()),
                        request_body: if body.is_empty() {
                            None
                        } else {
                            Some(body.clone())
                        },
                        response_status: None,
                        response_headers: None,
                        response_body: None,
                    };
                    let _ = traffic_tx.try_send(entry);
                }
                continue;
            }
            Err(_) => {
                //
                // Timeout waiting for response headers.
                //
                common::log_warn!(
                    "Intercepted [TIMEOUT]: {} {} - no response headers after {}s",
                    method,
                    url,
                    HEADER_TIMEOUT_SECS
                );

                //
                // Record request without response if pattern matches.
                //
                let should_collect = match url_pattern {
                    Some(pattern) => pattern.is_match(&url).unwrap_or(true),
                    None => true,
                };
                if should_collect {
                    let entry = InterceptedTrafficEntry {
                        id: None,
                        timestamp: chrono::Utc::now(),
                        node_id: config_node_id.clone(),
                        agent_short_name: agent.clone(),
                        intercept_method: config.intercept_method,
                        direction: TrafficDirection::Send,
                        method: Some(method.clone()),
                        url: url.clone(),
                        host: host.clone(),
                        request_headers: Some(headers_map.clone()),
                        request_body: if body.is_empty() {
                            None
                        } else {
                            Some(body.clone())
                        },
                        response_status: None,
                        response_headers: None,
                        response_body: None,
                    };
                    let _ = traffic_tx.try_send(entry);
                }
                continue;
            }
        };

        //
        // Forward response headers to client immediately (enables streaming).
        //
        client_write.write_all(response_line.as_bytes()).await?;
        for (key, value) in &response_headers {
            client_write
                .write_all(format!("{}: {}\r\n", key, value).as_bytes())
                .await?;
        }
        client_write.write_all(b"\r\n").await?;
        client_write.flush().await?;

        //
        // Read and forward body based on type.
        //
        let response_body = match body_type {
            ResponseBodyType::None => Vec::new(),
            ResponseBodyType::Chunked => {
                //
                // Stream chunks to client as they arrive (with per-chunk
                // timeouts).
                //
                match stream_chunked_body(&mut server_reader, &mut client_write).await {
                    Ok(buffered) => buffered,
                    Err(e) => {
                        common::log_warn!("Error streaming chunked body for {}: {}", url, e);
                        Vec::new()
                    }
                }
            }
            ResponseBodyType::ContentLength(len) => {
                //
                // Read fixed-length body with timeout.
                //
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                //
                // 5 minutes for large bodies.
                //
                const BODY_TIMEOUT_SECS: u64 = 300;
                let mut body_buf = vec![0u8; len];
                match timeout(
                    Duration::from_secs(BODY_TIMEOUT_SECS),
                    server_reader.read_exact(&mut body_buf),
                )
                .await
                {
                    Ok(Ok(_)) => {
                        //
                        // Forward to client.
                        //
                        if let Err(e) = client_write.write_all(&body_buf).await {
                            common::log_warn!("Error forwarding body to client: {}", e);
                        }
                        client_write.flush().await?;
                        body_buf
                    }
                    Ok(Err(e)) => {
                        common::log_warn!("Error reading response body for {}: {}", url, e);
                        Vec::new()
                    }
                    Err(_) => {
                        common::log_warn!(
                            "Timeout reading response body for {} after {}s",
                            url,
                            BODY_TIMEOUT_SECS
                        );
                        Vec::new()
                    }
                }
            }
        };

        //
        // Check for WebSocket upgrade (101 Switching Protocols)
        // Check response headers for upgrade confirmation from server (case-
        // insensitive key lookup).
        //
        let is_websocket_upgrade = status_code == Some(101)
            && response_headers.iter().any(|(k, v)| {
                k.eq_ignore_ascii_case("upgrade") && v.to_lowercase().contains("websocket")
            });

        if is_websocket_upgrade {
            //
            // Log the upgrade request.
            //
            let should_collect = match url_pattern {
                Some(pattern) => pattern.is_match(&url).unwrap_or(true),
                None => true,
            };

            if should_collect {
                let entry = InterceptedTrafficEntry {
                    id: None,
                    timestamp: chrono::Utc::now(),
                    node_id: config_node_id.clone(),
                    agent_short_name: agent.clone(),
                    intercept_method: config.intercept_method,
                    direction: TrafficDirection::Send,
                    method: Some("WS_UPGRADE".to_string()),
                    url: url.clone(),
                    host: host.clone(),
                    request_headers: Some(headers_map.clone()),
                    request_body: None,
                    response_status: status_code,
                    response_headers: Some(response_headers.clone()),
                    response_body: None,
                };
                let _ = traffic_tx.try_send(entry);
            }

            //
            // Switch to WebSocket frame handling
            // Keep using BufReaders to preserve any buffered data.
            //
            handle_websocket_traffic(
                client_reader,
                client_write,
                server_reader,
                server_write,
                &url,
                &host,
                &agent,
                &config_node_id,
                config.intercept_method,
                url_pattern,
                traffic_tx,
            )
            .await?;

            return Ok(());
        }

        //
        // Check if URL matches the pattern (if any)
        // Uses fancy-regex to support negative lookahead, e.g.,
        // ^(?!.*pacman).*$.
        //
        let should_collect = match url_pattern {
            Some(pattern) => pattern.is_match(&url).unwrap_or(true),
            //
            // No pattern = collect all.
            //
            None => true,
        };

        if should_collect {
            //
            // Decompress response body for storage (original is forwarded to
            // client as-is)
            // Case-insensitive header lookup.
            //
            let content_encoding = response_headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-encoding"))
                .map(|(_, v)| v.as_str());
            let decompressed_body = decompress_body(&response_body, content_encoding);

            //
            // Send to service.
            //
            let entry = InterceptedTrafficEntry {
                id: None,
                timestamp: chrono::Utc::now(),
                node_id: config_node_id.clone(),
                agent_short_name: agent.clone(),
                intercept_method: config.intercept_method,
                direction: TrafficDirection::Send,
                method: Some(method),
                url,
                host: host.clone(),
                request_headers: Some(headers_map),
                request_body: if body.is_empty() { None } else { Some(body) },
                response_status: status_code,
                response_headers: Some(response_headers),
                response_body: if decompressed_body.is_empty() {
                    None
                } else {
                    Some(decompressed_body)
                },
            };

            let _ = traffic_tx.try_send(entry);
        }
    }

    Ok(())
}

/// Handle WebSocket traffic after upgrade
async fn handle_websocket_traffic<CR, CW, SR, SW>(
    mut client_read: CR,
    mut client_write: CW,
    mut server_read: SR,
    mut server_write: SW,
    url: &str,
    host: &str,
    agent: &str,
    node_id: &str,
    intercept_method: InterceptMethod,
    url_pattern: Option<&fancy_regex::Regex>,
    traffic_tx: &mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<()>
where
    CR: tokio::io::AsyncRead + Unpin + Send,
    CW: tokio::io::AsyncWrite + Unpin + Send,
    SR: tokio::io::AsyncRead + Unpin + Send,
    SW: tokio::io::AsyncWrite + Unpin + Send,
{
    let should_collect = match url_pattern {
        Some(pattern) => pattern.is_match(url).unwrap_or(true),
        None => true,
    };

    let url = url.to_string();
    let host = host.to_string();
    let agent = agent.to_string();
    let node_id = node_id.to_string();

    //
    // Use tokio::select! to handle bidirectional traffic.
    //
    loop {
        tokio::select! {
            //
            // Prefer server responses to ensure we read them promptly.
            //
            biased;

            //
            // Read frame from server, forward to client.
            //
            result = read_websocket_frame(&mut server_read) => {
                match result {
                    Ok(Some((fin, opcode, payload))) => {
                        //
                        // Forward to client (server frames are not masked),
                        // preserving FIN bit.
                        //
                        if write_websocket_frame(&mut client_write, fin, opcode, &payload, false).await.is_err() {
                            break;
                        }

                        //
                        // Only collect complete messages (FIN=1 and data
                        // frames).
                        //
                        if should_collect && fin && (opcode == 0x1 || opcode == 0x2) {
                            let msg_type = if opcode == 0x1 { "TEXT" } else { "BINARY" };
                            let entry = InterceptedTrafficEntry {
                                id: None,
                                timestamp: chrono::Utc::now(),
                                node_id: node_id.clone(),
                                agent_short_name: agent.clone(),
                                intercept_method,
                                direction: TrafficDirection::Receive,
                                method: Some(format!("WS_{}", msg_type)),
                                url: url.clone(),
                                host: host.clone(),
                                request_headers: None,
                                request_body: None,
                                response_status: None,
                                response_headers: None,
                                response_body: Some(payload),
                            };
                            let _ = traffic_tx.try_send(entry);
                        }

                        if opcode == 0x8 {
                            break;
                        }
                    }
                    Ok(None) | Err(_) => {
                        break;
                    }
                }
            }

            //
            // Read frame from client, forward to server.
            //
            result = read_websocket_frame(&mut client_read) => {
                match result {
                    Ok(Some((fin, opcode, payload))) => {
                        //
                        // Forward to server (client-to-server frames MUST be
                        // masked per WebSocket spec), preserving FIN bit.
                        //
                        if write_websocket_frame(&mut server_write, fin, opcode, &payload, true).await.is_err() {
                            break;
                        }

                        //
                        // Only collect complete messages (FIN=1 and data
                        // frames).
                        //
                        if should_collect && fin && (opcode == 0x1 || opcode == 0x2) {
                            let msg_type = if opcode == 0x1 { "TEXT" } else { "BINARY" };
                            let entry = InterceptedTrafficEntry {
                                id: None,
                                timestamp: chrono::Utc::now(),
                                node_id: node_id.clone(),
                                agent_short_name: agent.clone(),
                                intercept_method,
                                direction: TrafficDirection::Send,
                                method: Some(format!("WS_{}", msg_type)),
                                url: url.clone(),
                                host: host.clone(),
                                request_headers: None,
                                request_body: Some(payload),
                                response_status: None,
                                response_headers: None,
                                response_body: None,
                            };
                            let _ = traffic_tx.try_send(entry);
                        }

                        if opcode == 0x8 {
                            break;
                        }
                    }
                    Ok(None) | Err(_) => {
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Read a WebSocket frame, returning (fin, opcode, payload)
async fn read_websocket_frame<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<(bool, u8, Vec<u8>)>> {
    use tokio::io::AsyncReadExt;

    //
    // Read first two bytes.
    //
    let mut header = [0u8; 2];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }

    let fin = (header[0] & 0x80) != 0;
    let opcode = header[0] & 0x0F;
    let masked = (header[1] & 0x80) != 0;
    let mut payload_len = (header[1] & 0x7F) as u64;

    //
    // Extended payload length.
    //
    if payload_len == 126 {
        let mut ext = [0u8; 2];
        reader.read_exact(&mut ext).await?;
        payload_len = u16::from_be_bytes(ext) as u64;
    } else if payload_len == 127 {
        let mut ext = [0u8; 8];
        reader.read_exact(&mut ext).await?;
        payload_len = u64::from_be_bytes(ext);
    }

    //
    // Masking key (if present).
    //
    let mask = if masked {
        let mut m = [0u8; 4];
        reader.read_exact(&mut m).await?;
        Some(m)
    } else {
        None
    };

    //
    // Read payload.
    //
    let mut payload = vec![0u8; payload_len as usize];
    if payload_len > 0 {
        reader.read_exact(&mut payload).await?;
    }

    //
    // Unmask if needed.
    //
    if let Some(mask) = mask {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }
    }

    Ok(Some((fin, opcode, payload)))
}

/// Write a WebSocket frame
async fn write_websocket_frame<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    fin: bool,
    opcode: u8,
    payload: &[u8],
    mask: bool,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    //
    // Build first byte: FIN bit + opcode.
    //
    let first_byte = (if fin { 0x80 } else { 0 }) | opcode;
    let mut header = vec![first_byte];

    let len = payload.len();
    if len < 126 {
        header.push((if mask { 0x80 } else { 0 }) | len as u8);
    } else if len < 65536 {
        header.push((if mask { 0x80 } else { 0 }) | 126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push((if mask { 0x80 } else { 0 }) | 127);
        header.extend_from_slice(&(len as u64).to_be_bytes());
    }

    writer.write_all(&header).await?;

    if mask {
        let mask_key: [u8; 4] = rand::random();
        writer.write_all(&mask_key).await?;
        let masked: Vec<u8> = payload
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask_key[i % 4])
            .collect();
        writer.write_all(&masked).await?;
    } else {
        writer.write_all(payload).await?;
    }

    writer.flush().await?;
    Ok(())
}

/// Handle plain HTTP request (non-CONNECT) - forward to target server
async fn handle_http_request(
    req: Request<hyper::body::Incoming>,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::Sender<InterceptedTrafficEntry>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let method_str = method.to_string();

    //
    // Extract host and port from URI or Host header.
    //
    let (host, port) = match (uri.host(), uri.port_u16()) {
        (Some(h), Some(p)) => (h.to_string(), p),
        (Some(h), None) => (
            h.to_string(),
            if uri.scheme_str() == Some("https") {
                443
            } else {
                80
            },
        ),
        _ => {
            //
            // Try Host header.
            //
            match req.headers().get("host").and_then(|h| h.to_str().ok()) {
                Some(host_header) => {
                    let parts: Vec<&str> = host_header.split(':').collect();
                    let h = parts[0].to_string();
                    let p = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(80);
                    (h, p)
                }
                None => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Full::new(Bytes::from("Missing host")))
                        .unwrap());
                }
            }
        }
    };

    let url_str = uri.to_string();
    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");

    //
    // Check if this is a WebSocket upgrade.
    //
    let _is_websocket = req
        .headers()
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_lowercase().contains("websocket"))
        .unwrap_or(false);

    //
    // Collect request headers and body - preserve order and case.
    //
    let req_headers: IndexMap<String, String> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes().to_vec(),
        Err(e) => {
            common::log_error!("Failed to collect request body: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from("Failed to read request body")))
                .unwrap());
        }
    };

    //
    // Connect to target server.
    //
    let target = format!("{}:{}", host, port);
    let stream = match TcpStream::connect(&target).await {
        Ok(s) => s,
        Err(e) => {
            common::log_error!("Failed to connect to {}: {}", target, e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::new(Bytes::from(format!(
                    "Failed to connect to {}",
                    target
                ))))
                .unwrap());
        }
    };

    //
    // Build and send raw HTTP request.
    //
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    //
    // Send request line.
    //
    let request_line = format!("{} {} HTTP/1.1\r\n", method_str, path);
    if let Err(e) = writer.write_all(request_line.as_bytes()).await {
        common::log_error!("Failed to write request: {}", e);
        return Ok(Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(Full::new(Bytes::from("Failed to forward request")))
            .unwrap());
    }

    //
    // Send headers.
    //
    for (key, value) in &req_headers {
        let header_line = format!("{}: {}\r\n", key, value);
        writer.write_all(header_line.as_bytes()).await.ok();
    }
    writer.write_all(b"\r\n").await.ok();

    //
    // Send body.
    //
    if !body_bytes.is_empty() {
        writer.write_all(&body_bytes).await.ok();
    }
    writer.flush().await.ok();

    //
    // Read response.
    //
    let mut response_line = String::new();
    if let Err(e) = reader.read_line(&mut response_line).await {
        common::log_error!("Failed to read response: {}", e);
        return Ok(Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(Full::new(Bytes::from("Failed to read response")))
            .unwrap());
    }

    //
    // Parse status.
    //
    let parts: Vec<&str> = response_line.trim().splitn(3, ' ').collect();
    let status_code = parts
        .get(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(502);

    //
    // Read response headers - preserve original order and case.
    //
    let mut resp_headers = IndexMap::new();
    let mut content_length: usize = 0;
    let mut chunked = false;
    let mut content_encoding = None;

    loop {
        let mut header_line = String::new();
        if reader.read_line(&mut header_line).await.is_err() {
            break;
        }
        let line = header_line.trim();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let original_key = key.trim().to_string();
            let value = value.trim().to_string();
            if original_key.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or(0);
            }
            if original_key.eq_ignore_ascii_case("transfer-encoding")
                && value.to_lowercase().contains("chunked")
            {
                chunked = true;
            }
            if original_key.eq_ignore_ascii_case("content-encoding") {
                content_encoding = Some(value.clone());
            }
            resp_headers.insert(original_key, value);
        }
    }

    //
    // Read response body.
    //
    let response_body = if chunked {
        let mut body = Vec::new();
        loop {
            let mut size_line = String::new();
            if reader.read_line(&mut size_line).await.is_err() {
                break;
            }
            let chunk_size = usize::from_str_radix(size_line.trim(), 16).unwrap_or(0);
            if chunk_size == 0 {
                let mut trailing = String::new();
                let _ = reader.read_line(&mut trailing).await;
                break;
            }
            let mut chunk = vec![0u8; chunk_size];
            if reader.read_exact(&mut chunk).await.is_err() {
                break;
            }
            body.extend_from_slice(&chunk);
            let mut crlf = [0u8; 2];
            let _ = reader.read_exact(&mut crlf).await;
        }
        body
    } else if content_length > 0 {
        let mut body = vec![0u8; content_length];
        if reader.read_exact(&mut body).await.is_err() {
            Vec::new()
        } else {
            body
        }
    } else {
        Vec::new()
    };

    //
    // Check if should collect telemetry.
    //
    let should_intercept = {
        let domains = config.intercept_domains.read().await;
        domains
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{}", d)))
    };

    if should_intercept {
        let agent = config
            .domain_to_agent
            .get(&host)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        let url_pattern = config.domain_to_url_pattern.get(&host);
        let should_collect = match url_pattern {
            Some(pattern) => pattern.is_match(&url_str).unwrap_or(false),
            None => true,
        };

        if should_collect {
            let decompressed_body = decompress_body(&response_body, content_encoding.as_deref());

            let entry = InterceptedTrafficEntry {
                id: None,
                timestamp: chrono::Utc::now(),
                node_id: config.node_id.clone(),
                agent_short_name: agent,
                intercept_method: config.intercept_method,
                direction: TrafficDirection::Send,
                method: Some(method_str.clone()),
                url: url_str,
                host: host.clone(),
                request_headers: Some(req_headers),
                request_body: if body_bytes.is_empty() {
                    None
                } else {
                    Some(body_bytes)
                },
                response_status: Some(status_code),
                response_headers: Some(resp_headers.clone()),
                response_body: if decompressed_body.is_empty() {
                    None
                } else {
                    Some(decompressed_body)
                },
            };

            let _ = traffic_tx.try_send(entry);
        }
    }

    //
    // Build response to return to client.
    //
    let mut response = Response::builder()
        .status(StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY));

    for (key, value) in &resp_headers {
        response = response.header(key.as_str(), value.as_str());
    }

    Ok(response
        .body(Full::new(Bytes::from(response_body)))
        .unwrap())
}

/// Create a TLS acceptor from CertificateAuthority for a specific host
fn create_tls_acceptor(ca: &CertificateAuthority, host: &str) -> Result<TlsAcceptor> {
    let cert_data = ca.get_leaf_cert(host).context("No certificate for host")?;

    create_tls_acceptor_from_pem(&cert_data.cert_pem, &cert_data.key_pem)
}

/// Create a TLS acceptor from certificate data
fn create_tls_acceptor_from_pem(cert_pem: &str, key_pem: &str) -> Result<TlsAcceptor> {
    let certs = rustls_pemfile::certs(&mut Cursor::new(cert_pem))
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to parse certificate")?;

    let key = rustls_pemfile::private_key(&mut Cursor::new(key_pem))
        .context("Failed to parse private key")?
        .context("No private key found")?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("Failed to create TLS config")?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Response body type indicator
#[derive(Debug, Clone, Copy)]
enum ResponseBodyType {
    /// No body expected (e.g., 204, 304)
    None,
    /// Body with known Content-Length
    ContentLength(usize),
    /// Chunked transfer encoding
    Chunked,
}

/// Read only the response headers (status line + headers), don't read body
/// Returns (response_line, status_code, headers, body_type)
async fn read_response_headers<R>(
    reader: &mut tokio::io::BufReader<R>,
) -> Result<(
    String,
    Option<u16>,
    IndexMap<String, String>,
    ResponseBodyType,
)>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;

    //
    // Read response line.
    //
    let mut response_line = String::new();
    let bytes_read = reader
        .read_line(&mut response_line)
        .await
        .context("Failed to read response line")?;

    if bytes_read == 0 {
        return Err(anyhow::anyhow!("Connection closed before response"));
    }

    //
    // Parse status code.
    //
    let status_code = response_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok());

    //
    // Read headers - preserve original order and case.
    //
    let mut response_headers = IndexMap::new();
    let mut content_length: Option<usize> = None;
    let mut is_chunked = false;

    loop {
        let mut header_line = String::new();
        reader
            .read_line(&mut header_line)
            .await
            .context("Failed to read response header")?;
        let line = header_line.trim();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let original_key = key.trim().to_string();
            let value = value.trim().to_string();
            if original_key.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().ok();
            }
            if original_key.eq_ignore_ascii_case("transfer-encoding")
                && value.to_lowercase().contains("chunked")
            {
                is_chunked = true;
            }
            response_headers.insert(original_key, value);
        }
    }

    //
    // Determine body type
    // 1xx, 204 No Content, 304 Not Modified have no body.
    //
    let body_type = match status_code {
        Some(code) if code < 200 || code == 204 || code == 304 => ResponseBodyType::None,
        _ if is_chunked => ResponseBodyType::Chunked,
        _ => match content_length {
            Some(0) => ResponseBodyType::None,
            Some(len) => ResponseBodyType::ContentLength(len),
            //
            // No Content-Length and not chunked = no body.
            //
            None => ResponseBodyType::None,
        },
    };

    Ok((response_line, status_code, response_headers, body_type))
}

/// Maximum body size to buffer for logging (10 MB)
const MAX_BODY_BUFFER_SIZE: usize = 10 * 1024 * 1024;

/// Per-chunk timeout for streaming responses (60 seconds)
const CHUNK_TIMEOUT_SECS: u64 = 60;

/// Stream chunked response body from server to client, buffering for logging
/// Returns the buffered body (may be truncated if too large)
async fn stream_chunked_body<R, W>(
    reader: &mut tokio::io::BufReader<R>,
    writer: &mut W,
) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

    let mut body_buffer = Vec::new();

    loop {
        //
        // Read chunk size with timeout.
        //
        let mut size_line = String::new();
        let read_result = timeout(
            Duration::from_secs(CHUNK_TIMEOUT_SECS),
            reader.read_line(&mut size_line),
        )
        .await;

        let bytes_read = match read_result {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(anyhow::anyhow!("Failed to read chunk size: {}", e)),
            Err(_) => {
                //
                // Timeout - send terminating chunk and return what we have.
                //
                common::log_debug!(
                    "Chunk read timeout after {}s, terminating stream",
                    CHUNK_TIMEOUT_SECS
                );
                writer.write_all(b"0\r\n\r\n").await?;
                writer.flush().await?;
                return Ok(body_buffer);
            }
        };

        if bytes_read == 0 {
            //
            // Connection closed - send terminating chunk.
            //
            writer.write_all(b"0\r\n\r\n").await?;
            writer.flush().await?;
            return Ok(body_buffer);
        }

        //
        // Forward chunk size line to client.
        //
        writer.write_all(size_line.as_bytes()).await?;

        let chunk_size = match usize::from_str_radix(size_line.trim(), 16) {
            Ok(size) => size,
            Err(_) => {
                //
                // Invalid chunk size - terminate.
                //
                writer.write_all(b"0\r\n\r\n").await?;
                writer.flush().await?;
                return Err(anyhow::anyhow!("Invalid chunk size: {}", size_line.trim()));
            }
        };

        if chunk_size == 0 {
            //
            // Final chunk - read and forward trailing headers/CRLF.
            //
            let mut trailer = String::new();
            let _ = reader.read_line(&mut trailer).await;
            writer.write_all(trailer.as_bytes()).await?;
            writer.flush().await?;
            break;
        }

        //
        // Read chunk data with timeout.
        //
        let mut chunk = vec![0u8; chunk_size];
        let chunk_result = timeout(
            Duration::from_secs(CHUNK_TIMEOUT_SECS),
            reader.read_exact(&mut chunk),
        )
        .await;

        match chunk_result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(anyhow::anyhow!("Failed to read chunk data: {}", e)),
            Err(_) => {
                common::log_debug!("Chunk data read timeout, terminating stream");
                writer.write_all(b"0\r\n\r\n").await?;
                writer.flush().await?;
                return Ok(body_buffer);
            }
        }

        //
        // Forward chunk data to client.
        //
        writer.write_all(&chunk).await?;

        //
        // Buffer for logging (up to limit).
        //
        if body_buffer.len() < MAX_BODY_BUFFER_SIZE {
            let space_left = MAX_BODY_BUFFER_SIZE - body_buffer.len();
            let to_copy = chunk_size.min(space_left);
            body_buffer.extend_from_slice(&chunk[..to_copy]);
        }

        //
        // Read and forward trailing CRLF.
        //
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf).await?;
        writer.write_all(&crlf).await?;

        //
        // Flush periodically for streaming responsiveness.
        //
        writer.flush().await?;
    }

    Ok(body_buffer)
}

/// Discover the default network interface by parsing `ip route show default`.
#[cfg(target_os = "linux")]
fn discover_default_interface() -> Option<String> {
    use std::process::Command;

    let output = Command::new("ip")
        .args(["route", "show", "default"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    //
    // Parse output like: "default via 192.168.1.1 dev eth0 proto dhcp metric 100"
    //
    for line in stdout.lines() {
        if line.starts_with("default") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, part) in parts.iter().enumerate() {
                if *part == "dev" && i + 1 < parts.len() {
                    return Some(parts[i + 1].to_string());
                }
            }
        }
    }

    None
}

/// Discover an IP address that is not the TUN IP (10.255.x.x).
/// Used on Windows to bind sockets for VPN bypass.
#[cfg(target_os = "windows")]
fn discover_non_tun_ip() -> Option<std::net::IpAddr> {
    use std::net::IpAddr;

    //
    // Use local_ip crate if available, or fall back to a simple method.
    // For now, iterate through interfaces looking for a non-TUN IPv4.
    //
    if let Ok(addrs) = if_addrs::get_if_addrs() {
        for iface in addrs {
            if let IpAddr::V4(ipv4) = iface.ip() {
                //
                // Skip loopback and TUN subnet (10.255.x.x).
                //
                if ipv4.is_loopback() {
                    continue;
                }
                if ipv4.octets()[0] == 10 && ipv4.octets()[1] == 255 {
                    continue;
                }

                //
                // Found a non-TUN IPv4 address.
                //
                return Some(IpAddr::V4(ipv4));
            }
        }
    }

    None
}
