async fn run_proxy(
    listener: TcpListener,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    {
        let domains = config.intercept_domains.read().await;
        common::log_info!("Proxy server running, intercepting domains: {:?}", *domains);
    }

    loop {
        tokio::select! {
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
                        common::log_error!("Failed to accept connection: {}", e);
                    }
                }
            }
            _ = &mut shutdown_rx => {
                common::log_info!("Proxy server shutting down");
                break;
            }
        }
    }
}

/// Run the HTTP proxy server (port 80) for Hosts mode.
///
/// This handles plain HTTP connections which are less common for AI APIs
/// but may be needed for some services.
async fn run_proxy_http(
    listener: TcpListener,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
) {
    common::log_info!("HTTP proxy server running on port 80 (Hosts mode)");

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let ca = Arc::clone(&ca);
                let config = Arc::clone(&config);
                let traffic_tx = traffic_tx.clone();

                tokio::spawn(async move {
                    let _ = handle_connection(stream, addr, ca, config, traffic_tx).await;
                });
            }
            Err(e) => {
                common::log_error!("Failed to accept HTTP connection: {}", e);
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
    traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
) -> Result<()> {
    //
    // Peek at first byte to detect TLS vs HTTP.
    //
    let mut peek_buf = [0u8; 1];
    stream.peek(&mut peek_buf).await.context("Failed to peek connection")?;

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
            async move {
                handle_request(req, addr, ca, config, traffic_tx).await
            }
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
    traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
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
                common::log_warn!("Failed to get original destination via SO_ORIGINAL_DST: {}", e);
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
    let n = stream.peek(&mut client_hello_buf).await.context("Failed to peek ClientHello")?;

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
    // Send observation for agent discovery.
    //
    if let Some(ref tx) = config.connection_observer_tx {
        let ip = original_dst
            .map(|a| a.ip())
            .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
        let _ = tx.send(ObservedConnection {
            ip,
            port: dest_port,
            domain: Some(sni.clone()),
            is_https: true,
            api_key: None,
        });
    }

    //
    // Check if this domain should be intercepted.
    //
    let should_intercept = {
        let domains = config.intercept_domains.read().await;
        domains.iter().any(|d| sni == *d || sni.ends_with(&format!(".{}", d)))
    };

    if !should_intercept {
        //
        // Non-intercepted domain reached proxy (likely shares IP with intercepted domain).
        // Tunnel through without TLS termination.
        //
        common::log_info!("Passthrough for non-intercepted domain {}", sni);

        let pre_resolved_ip = config.domain_to_real_ip.get(&sni).copied();
        let server = connect_bypass_tun(&sni, dest_port, pre_resolved_ip, config.intercept_method).await
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
            ca_guard.generate_leaf_cert(&sni)
                .context("Failed to generate leaf certificate")?;
        }
        create_tls_acceptor(&ca_guard, &sni)?
    };

    //
    // Perform TLS handshake with client.
    //
    let tls_stream = acceptor.accept(stream).await
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
                let name_len = u16::from_be_bytes([handshake[sni_pos + 1], handshake[sni_pos + 2]]) as usize;
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

