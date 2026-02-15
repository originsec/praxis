async fn proxy_https_traffic<C, S>(
    mut client_tls: tokio_rustls::server::TlsStream<C>,
    server_tls: tokio_rustls::client::TlsStream<S>,
    host: &str,
    config: &ProxyConfig,
    traffic_tx: &mpsc::UnboundedSender<InterceptedTrafficEntry>,
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

        let extra_bytes = if n > 24 { peeked[24..].to_vec() } else { Vec::new() };
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
    let agent = config.domain_to_agent.get(&host)
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
                common::log_debug!("proxy_https_traffic: read {} bytes: {:?}", n, request_line.trim());
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
        // Extract API key from headers and send observation for agent discovery.
        //
        if let Some(ref tx) = config.connection_observer_tx {
            let api_key = extract_api_key_from_headers(&headers_map, &host);
            if api_key.is_some() {
                common::log_debug!(
                    "Sending observation with API key for {} (method={}, path={})",
                    host, method, path
                );
                let _ = tx.send(ObservedConnection {
                    ip: std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
                    port: 443,
                    domain: Some(host.clone()),
                    is_https: true,
                    api_key,
                });
            }
        }

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
            server_write.write_all(format!("{}: {}\r\n", key, value).as_bytes()).await?;
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
            read_response_headers(&mut server_reader)
        ).await;

        let (response_line, status_code, response_headers, body_type) = match headers_result {
            Ok(Ok((line, status, headers, body_type))) => (line, status, headers, body_type),
            Ok(Err(e)) => {
                //
                // Error reading response headers.
                //
                common::log_warn!("Intercepted [NO RESPONSE]: {} {} - error: {}", method, url, e);

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
                        request_body: if body.is_empty() { None } else { Some(body.clone()) },
                        response_status: None,
                        response_headers: None,
                        response_body: None,
                    };
                    let _ = traffic_tx.send(entry);
                }
                continue;
            }
            Err(_) => {
                //
                // Timeout waiting for response headers.
                //
                common::log_warn!("Intercepted [TIMEOUT]: {} {} - no response headers after {}s", method, url, HEADER_TIMEOUT_SECS);

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
                        request_body: if body.is_empty() { None } else { Some(body.clone()) },
                        response_status: None,
                        response_headers: None,
                        response_body: None,
                    };
                    let _ = traffic_tx.send(entry);
                }
                continue;
            }
        };

        //
        // Forward response headers to client immediately (enables streaming).
        //
        client_write.write_all(response_line.as_bytes()).await?;
        for (key, value) in &response_headers {
            client_write.write_all(format!("{}: {}\r\n", key, value).as_bytes()).await?;
        }
        client_write.write_all(b"\r\n").await?;
        client_write.flush().await?;

        //
        // Read and forward body based on type.
        //
        let response_body = match body_type {
            ResponseBodyType::None => {
                Vec::new()
            }
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
                match timeout(Duration::from_secs(BODY_TIMEOUT_SECS), server_reader.read_exact(&mut body_buf)).await {
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
                        common::log_warn!("Timeout reading response body for {} after {}s", url, BODY_TIMEOUT_SECS);
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
            && response_headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("upgrade") && v.to_lowercase().contains("websocket"));

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
                let _ = traffic_tx.send(entry);
            }

            //
            // Switch to WebSocket frame handling
            // Keep using BufReaders to preserve any buffered data.
            //
            handle_websocket_traffic(
                client_reader, client_write,
                server_reader, server_write,
                &url, &host, &agent, &config_node_id,
                config.intercept_method,
                url_pattern, traffic_tx,
            ).await?;

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
            let content_encoding = response_headers.iter()
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
                response_body: if decompressed_body.is_empty() { None } else { Some(decompressed_body) },
            };

            let _ = traffic_tx.send(entry);
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
    traffic_tx: &mpsc::UnboundedSender<InterceptedTrafficEntry>,
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
                            let _ = traffic_tx.send(entry);
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
                            let _ = traffic_tx.send(entry);
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
        let masked: Vec<u8> = payload.iter().enumerate()
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
    traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let method_str = method.to_string();

    //
    // Extract host and port from URI or Host header.
    //
    let (host, port) = match (uri.host(), uri.port_u16()) {
        (Some(h), Some(p)) => (h.to_string(), p),
        (Some(h), None) => (h.to_string(), if uri.scheme_str() == Some("https") { 443 } else { 80 }),
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
    let _is_websocket = req.headers()
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
                .body(Full::new(Bytes::from(format!("Failed to connect to {}", target))))
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
    let status_code = parts.get(1)
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
            if original_key.eq_ignore_ascii_case("transfer-encoding") && value.to_lowercase().contains("chunked") {
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
        domains.iter().any(|d| host == *d || host.ends_with(&format!(".{}", d)))
    };

    if should_intercept {
        let agent = config.domain_to_agent.get(&host)
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
                request_body: if body_bytes.is_empty() { None } else { Some(body_bytes) },
                response_status: Some(status_code),
                response_headers: Some(resp_headers.clone()),
                response_body: if decompressed_body.is_empty() { None } else { Some(decompressed_body) },
            };

            let _ = traffic_tx.send(entry);
        }
    }

    //
    // Build response to return to client.
    //
    let mut response = Response::builder().status(StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY));

    for (key, value) in &resp_headers {
        response = response.header(key.as_str(), value.as_str());
    }

    Ok(response.body(Full::new(Bytes::from(response_body))).unwrap())
}

