async fn intercept_tls_traffic(
    upgraded: hyper::upgrade::Upgraded,
    host: &str,
    port: u16,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: &ProxyConfig,
    traffic_tx: &mpsc::UnboundedSender<InterceptedTrafficEntry>,
) -> Result<()> {
    //
    // Get leaf certificate for this domain.
    //
    let (cert_pem, key_pem) = {
        let mut ca_guard = ca.write().await;
        let cert_data = ca_guard.generate_leaf_cert(host)
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
    let server_tcp = TcpStream::connect(&target).await
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

    let server_tls = connector.connect(server_name, server_tcp).await
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
#[allow(dead_code)]
const H2_FRAME_PRIORITY: u8 = 0x2;
#[allow(dead_code)]
const H2_FRAME_RST_STREAM: u8 = 0x3;
const H2_FRAME_SETTINGS: u8 = 0x4;
#[allow(dead_code)]
const H2_FRAME_PUSH_PROMISE: u8 = 0x5;
#[allow(dead_code)]
const H2_FRAME_PING: u8 = 0x6;
const H2_FRAME_GOAWAY: u8 = 0x7;
#[allow(dead_code)]
const H2_FRAME_WINDOW_UPDATE: u8 = 0x8;
#[allow(dead_code)]
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

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
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
    traffic_tx: &mpsc::UnboundedSender<InterceptedTrafficEntry>,
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
async fn read_h2_frame<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<H2Frame>> {
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

//
// Decompress gRPC payload.
// gRPC DATA frames have a 5-byte prefix:
//   - Byte 0: Compression flag (0=uncompressed, 1=gzip)
//   - Bytes 1-4: Message length (big-endian uint32)
//   - Bytes 5+: Message data (possibly gzip compressed)
//

fn decompress_grpc_payload(payload: &[u8]) -> Vec<u8> {
    if payload.len() < 5 {
        return payload.to_vec();
    }

    let compressed = payload[0] == 1;
    let message_len = ((payload[1] as u32) << 24)
        | ((payload[2] as u32) << 16)
        | ((payload[3] as u32) << 8)
        | (payload[4] as u32);

    let message_data = &payload[5..];

    if message_data.len() < message_len as usize {
        return payload.to_vec();
    }

    if !compressed {
        message_data.to_vec()
    } else {
        let mut decoder = GzDecoder::new(Cursor::new(message_data));
        let mut decompressed = Vec::new();
        match decoder.read_to_end(&mut decompressed) {
            Ok(_) => decompressed,
            Err(e) => {
                common::log_debug!("gRPC decompression failed: {}", e);
                payload.to_vec()
            }
        }
    }
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
    traffic_tx: &mpsc::UnboundedSender<InterceptedTrafficEntry>,
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
                                let _ = traffic_tx.send(entry);
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
                                let _ = traffic_tx.send(entry);
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
                                let _ = traffic_tx.send(entry);
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
                                let _ = traffic_tx.send(entry);
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

