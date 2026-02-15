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
        target.to_socket_addrs()
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
                    common::log_warn!("Failed to bind to device {}: {} (may need CAP_NET_ADMIN)", iface, e);
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

    socket.set_nonblocking(true)
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
            common::log_debug!("connect_bypass_tun: connect() returned EINPROGRESS macOS (expected)");
        }
        Err(e) => {
            common::log_error!("connect_bypass_tun: connect() failed: {} (os_error={:?})", e, e.raw_os_error());
            return Err(e).context("Failed to connect");
        }
    }

    //
    // Convert to tokio TcpStream.
    //
    let std_stream: std::net::TcpStream = socket.into();
    let stream = TcpStream::from_std(std_stream)
        .context("Failed to convert to tokio stream")?;

    //
    // Wait for connection to complete.
    //
    common::log_debug!("connect_bypass_tun: waiting for connection to {}", addr);
    stream.writable().await
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
    traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
) -> Result<()> {
    //
    // For Hosts mode, use pre-resolved IP to avoid hosts file loop.
    //
    let pre_resolved_ip = config.domain_to_real_ip.get(host).copied();

    //
    // Connect to real server, bypassing TUN routing and hosts file.
    //
    common::log_debug!("handle_intercepted_tunnel_vpn: connecting to {}:{}", host, port);
    let server_tcp = connect_bypass_tun(host, port, pre_resolved_ip, config.intercept_method).await
        .context(format!("Failed to connect to {}:{}", host, port))?;
    common::log_debug!("handle_intercepted_tunnel_vpn: TCP connected to {}:{}", host, port);

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
    let server_tls = connector.connect(server_name, server_tcp).await
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
    traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
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
    traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
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
    // Send observation for agent discovery.
    //
    if let Some(ref tx) = config.connection_observer_tx {
        let _ = tx.send(ObservedConnection {
            ip: std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            port,
            domain: Some(host.clone()),
            is_https: port == 443,
            api_key: None,
        });
    }

    //
    // Check if this domain should be intercepted.
    //
    let should_intercept = {
        let domains = config.intercept_domains.read().await;
        domains.iter().any(|d| host == *d || host.ends_with(&format!(".{}", d)))
    };

    //
    // Establish tunnel to the target server.
    //
    tokio::task::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let _ = tunnel(upgraded, &host, port, should_intercept, ca, &config, &traffic_tx).await;
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
    traffic_tx: &mpsc::UnboundedSender<InterceptedTrafficEntry>,
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
        let mut server = TcpStream::connect(&target).await
            .context(format!("Failed to connect to {}", target))?;
        let mut upgraded = TokioIo::new(upgraded);

        let _ = tokio::io::copy_bidirectional(&mut upgraded, &mut server).await;
        Ok(())
    }
}

