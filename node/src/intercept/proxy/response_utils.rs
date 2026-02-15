fn create_tls_acceptor(ca: &CertificateAuthority, host: &str) -> Result<TlsAcceptor> {
    let cert_data = ca.get_leaf_cert(host)
        .context("No certificate for host")?;

    create_tls_acceptor_from_pem(&cert_data.cert_pem, &cert_data.key_pem)
}

/// Create a TLS acceptor from certificate data
#[allow(dead_code)]
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

/// Read HTTP response from server
/// Returns (response_line, status_code, headers, body)
#[allow(dead_code)]
async fn read_response<R>(reader: &mut tokio::io::BufReader<R>) -> Result<(String, Option<u16>, IndexMap<String, String>, Vec<u8>)>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};

    //
    // Read response line.
    //
    let mut response_line = String::new();
    let bytes_read = reader.read_line(&mut response_line).await
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
    let mut response_content_length: usize = 0;
    let mut is_chunked = false;

    loop {
        let mut header_line = String::new();
        reader.read_line(&mut header_line).await
            .context("Failed to read response header")?;
        let line = header_line.trim();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let original_key = key.trim().to_string();
            let value = value.trim().to_string();
            if original_key.eq_ignore_ascii_case("content-length") {
                response_content_length = value.parse().unwrap_or(0);
            }
            if original_key.eq_ignore_ascii_case("transfer-encoding") && value.to_lowercase().contains("chunked") {
                is_chunked = true;
            }
            response_headers.insert(original_key, value);
        }
    }

    //
    // Read body.
    //
    let response_body = if is_chunked {
        read_chunked_body(reader).await.unwrap_or_default()
    } else if response_content_length > 0 {
        let mut body = vec![0u8; response_content_length];
        reader.read_exact(&mut body).await
            .context("Failed to read response body")?;
        body
    } else {
        Vec::new()
    };

    Ok((response_line, status_code, response_headers, response_body))
}

/// Read chunked transfer-encoded body (non-streaming, for non-chunked fallback)
#[allow(dead_code)]
async fn read_chunked_body<R>(reader: &mut tokio::io::BufReader<R>) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};

    let mut body = Vec::new();

    loop {
        let mut size_line = String::new();
        reader.read_line(&mut size_line).await
            .context("Failed to read chunk size")?;

        let chunk_size = usize::from_str_radix(size_line.trim(), 16)
            .context("Invalid chunk size")?;

        if chunk_size == 0 {
            //
            // Read trailing CRLF.
            //
            let mut trailer = String::new();
            let _ = reader.read_line(&mut trailer).await;
            break;
        }

        let mut chunk = vec![0u8; chunk_size];
        reader.read_exact(&mut chunk).await
            .context("Failed to read chunk data")?;
        body.extend_from_slice(&chunk);

        //
        // Read trailing CRLF after chunk.
        //
        let mut crlf = [0u8; 2];
        let _ = reader.read_exact(&mut crlf).await;
    }

    Ok(body)
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
) -> Result<(String, Option<u16>, IndexMap<String, String>, ResponseBodyType)>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;

    //
    // Read response line.
    //
    let mut response_line = String::new();
    let bytes_read = reader.read_line(&mut response_line).await
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
        reader.read_line(&mut header_line).await
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
            if original_key.eq_ignore_ascii_case("transfer-encoding") && value.to_lowercase().contains("chunked") {
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
        }
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
            reader.read_line(&mut size_line)
        ).await;

        let bytes_read = match read_result {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(anyhow::anyhow!("Failed to read chunk size: {}", e)),
            Err(_) => {
                //
                // Timeout - send terminating chunk and return what we have.
                //
                common::log_debug!("Chunk read timeout after {}s, terminating stream", CHUNK_TIMEOUT_SECS);
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
            reader.read_exact(&mut chunk)
        ).await;

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

/// Decompress response body based on Content-Encoding header
fn decompress_body(body: &[u8], content_encoding: Option<&str>) -> Vec<u8> {
    let encoding = match content_encoding {
        Some(e) => e.to_lowercase(),
        None => return body.to_vec(),
    };

    if encoding.contains("gzip") {
        let mut decoder = GzDecoder::new(body);
        let mut decompressed = Vec::new();
        match decoder.read_to_end(&mut decompressed) {
            Ok(_) => decompressed,
            Err(e) => {
                common::log_debug!("Failed to decompress gzip body: {}", e);
                body.to_vec()
            }
        }
    } else if encoding.contains("deflate") {
        let mut decoder = DeflateDecoder::new(body);
        let mut decompressed = Vec::new();
        match decoder.read_to_end(&mut decompressed) {
            Ok(_) => decompressed,
            Err(e) => {
                common::log_debug!("Failed to decompress deflate body: {}", e);
                body.to_vec()
            }
        }
    } else if encoding.contains("br") {
        let mut decompressed = Vec::new();
        match brotli::BrotliDecompress(&mut std::io::Cursor::new(body), &mut decompressed) {
            Ok(_) => decompressed,
            Err(e) => {
                common::log_debug!("Failed to decompress brotli body: {}", e);
                body.to_vec()
            }
        }
    } else if encoding.contains("zstd") {
        match zstd::decode_all(std::io::Cursor::new(body)) {
            Ok(decompressed) => decompressed,
            Err(e) => {
                common::log_debug!("Failed to decompress zstd body: {}", e);
                body.to_vec()
            }
        }
    } else {
        //
        // Unknown encoding or "identity", return as-is.
        //
        body.to_vec()
    }
}
