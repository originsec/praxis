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
use tokio_util::task::TaskTracker;
use tokio_util::sync::CancellationToken;

use super::certificate::CertificateAuthority;
use body::{MAX_CAPTURE_BODY_SIZE, decompress_body, decompress_grpc_payload};

const HTTP_HEADER_TIMEOUT_SECS: u64 = 30;
const MAX_HTTP_HEADER_LINE_SIZE: usize = 64 * 1024;
const MAX_HTTP_HEADER_SECTION_SIZE: usize = 1024 * 1024;

type HttpResponseHead = (
    String,
    Option<u16>,
    Vec<(String, String)>,
    ResponseBodyType,
);

/// Configuration for the intercept proxy
pub struct ProxyConfig {
    /// Domains to intercept (extract and log traffic) - dynamically updatable
    pub intercept_domains: Arc<RwLock<HashSet<String>>>,
    /// Capture configuration for each domain. Multiple agents may own the
    /// same endpoint, so candidates are retained instead of overwritten.
    pub domain_capture_configs: HashMap<String, DomainCaptureConfig>,
    /// Node ID for traffic entries
    pub node_id: String,
    /// Interception method used
    pub intercept_method: InterceptMethod,
    /// Pre-resolved IPs for domains (used in Hosts mode to bypass hosts file redirection)
    pub domain_to_real_ip: HashMap<String, std::net::IpAddr>,
}

#[derive(Clone, Default)]
pub struct DomainCaptureConfig {
    pub agent_rules: Vec<AgentCaptureRule>,
}

#[derive(Clone)]
pub struct AgentCaptureRule {
    pub agent_short_name: String,
    pub url_pattern: Option<fancy_regex::Regex>,
}

impl DomainCaptureConfig {
    fn agent_label(&self) -> String {
        let mut agents = Vec::new();
        for rule in &self.agent_rules {
            if !agents.contains(&rule.agent_short_name.as_str()) {
                agents.push(rule.agent_short_name.as_str());
            }
        }
        if agents.is_empty() {
            "unknown".to_string()
        } else {
            agents.join("|")
        }
    }

    fn agent_label_for_url(&self, url: &str) -> Option<String> {
        let mut agents = Vec::new();
        for rule in &self.agent_rules {
            let matches = rule
                .url_pattern
                .as_ref()
                .map(|pattern| pattern.is_match(url).unwrap_or(false))
                .unwrap_or(true);
            if matches && !agents.contains(&rule.agent_short_name.as_str()) {
                agents.push(rule.agent_short_name.as_str());
            }
        }
        (!agents.is_empty()).then(|| agents.join("|"))
    }

    fn matches_url(&self, url: &str) -> bool {
        self.agent_label_for_url(url).is_some()
    }
}

impl ProxyConfig {
    //
    // Look up a map value by exact host, then by longest configured domain
    // suffix (host ends with ".domain"). Mirrors intercept-domain matching
    // so subdomains inherit agent tags and URL filters.
    //
    fn lookup_by_domain<'a, V>(&self, map: &'a HashMap<String, V>, host: &str) -> Option<&'a V> {
        if let Some(v) = map.get(host) {
            return Some(v);
        }
        map.iter()
            .filter(|(domain, _)| host.ends_with(&format!(".{}", domain)))
            .max_by_key(|(domain, _)| domain.len())
            .map(|(_, v)| v)
    }

    fn agent_for_host(&self, host: &str) -> String {
        self.capture_config_for_host(host)
            .map(DomainCaptureConfig::agent_label)
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn capture_config_for_host(&self, host: &str) -> Option<&DomainCaptureConfig> {
        self.lookup_by_domain(&self.domain_capture_configs, host)
    }

}

fn capture_agent_label(
    capture_config: Option<&DomainCaptureConfig>,
    configured_agent: &str,
    url: &str,
) -> Option<String> {
    match capture_config {
        Some(capture) => capture.agent_label_for_url(url),
        None => Some(configured_agent.to_string()),
    }
}

//
// Wraps the traffic channel sender so that entries dropped because the
// channel is full are counted and surfaced. Without this, a slow consumer
// silently loses captured traffic and the operator sees an apparently
// complete capture. Cloneable: the drop counter and last-warn timestamp are
// shared across all connection handlers of a proxy session.
//

#[derive(Clone)]
pub struct TrafficSink {
    tx: mpsc::Sender<InterceptedTrafficEntry>,
    dropped: Arc<std::sync::atomic::AtomicU64>,
    last_warn_unix: Arc<std::sync::atomic::AtomicU64>,
}

impl TrafficSink {
    const WARN_INTERVAL_SECS: u64 = 5;

    pub fn new(tx: mpsc::Sender<InterceptedTrafficEntry>) -> Self {
        Self {
            tx,
            dropped: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            last_warn_unix: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    //
    // Send an entry, recording and warning (rate-limited) when the channel
    // is full. Returns Err(()) if the entry was not enqueued; callers ignore
    // it (best-effort capture) but the signature keeps the `let _ =` idiom.
    //
    pub fn try_send(
        &self,
        entry: InterceptedTrafficEntry,
    ) -> std::result::Result<(), ()> {
        match self.tx.try_send(entry) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                let total = self.dropped.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                self.maybe_warn(total);
                Err(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(()),
        }
    }

    fn maybe_warn(&self, total_dropped: u64) {
        use std::sync::atomic::Ordering;
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let last = self.last_warn_unix.load(Ordering::Relaxed);
        if now.saturating_sub(last) < Self::WARN_INTERVAL_SECS {
            return;
        }
        if self
            .last_warn_unix
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        common::log_warn!(
            "Intercept traffic channel full — {} captured entr{} dropped so far; \
             capture is lossy (service/consumer not keeping up)",
            total_dropped,
            if total_dropped == 1 { "y" } else { "ies" }
        );
    }
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
    /// Tracks every accepted connection and upgraded CONNECT tunnel.
    task_tracker: TaskTracker,
}

impl InterceptProxy {
    /// Start the intercept proxy server
    pub async fn start(
        ca: Arc<RwLock<CertificateAuthority>>,
        config: ProxyConfig,
        traffic_tx: TrafficSink,
    ) -> Result<Self> {
        let shutdown_token = CancellationToken::new();
        let task_tracker = TaskTracker::new();
        let config = Arc::new(config);
        let mut extra_task_handles = Vec::new();

        //
        // Bind address comes solely from listen_policy (same values unit tests
        // assert). Method-specific extras (Hosts :80, TPROXY IP_TRANSPARENT)
        // layer on top of that primary bind.
        //
        let listen_plan = super::listen_policy::plan_for(config.intercept_method);
        let primary_bind = super::listen_policy::primary_bind_addr(listen_plan.bind);
        let (listener, port) = match listen_plan.bind {
            super::listen_policy::BindSpec::LoopbackFixedHttps => {
                let https_listener = TcpListener::bind(&primary_bind).await.context(
                    "Failed to bind to port 443. Hosts-based interception requires running as root/administrator.",
                )?;

                common::log_info!("Intercept proxy (Hosts mode) listening on port 443");

                //
                // Also try to bind to port 80 for HTTP (best effort).
                //
                match TcpListener::bind("127.0.0.1:80").await {
                    Ok(http_listener) => {
                        common::log_info!(
                            "Intercept proxy (Hosts mode) also listening on port 80"
                        );
                        let ca_clone = Arc::clone(&ca);
                        let config_clone = Arc::clone(&config);
                        let traffic_tx_clone = traffic_tx.clone();
                        let http_shutdown = shutdown_token.clone();
                        let http_tasks = task_tracker.clone();

                        let http_task = tokio::spawn(run_proxy_http(
                            http_listener,
                            ca_clone,
                            config_clone,
                            traffic_tx_clone,
                            http_shutdown,
                            http_tasks,
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
            }
            super::listen_policy::BindSpec::LoopbackEphemeral => {
                let listener = TcpListener::bind(&primary_bind).await?;
                let port = listener.local_addr()?.port();
                common::log_info!("Intercept proxy (Proxy mode) starting on port {}", port);
                (listener, port)
            }
            super::listen_policy::BindSpec::WildcardTransparent => {
                #[cfg(target_os = "linux")]
                {
                    let std_listener = super::tproxy::create_transparent_listener(&primary_bind)
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
            }
            super::listen_policy::BindSpec::TunIpEphemeral => {
                let listener = TcpListener::bind(&primary_bind)
                    .await
                    .with_context(|| format!("Failed to bind VPN proxy on {}", primary_bind))?;
                let port = listener.local_addr()?.port();
                common::log_info!(
                    "Intercept proxy (VPN mode) starting on {}",
                    primary_bind.replace(":0", &format!(":{port}"))
                );
                (listener, port)
            }
        };

        let task_handle = tokio::spawn(run_proxy(
            listener,
            ca,
            config,
            traffic_tx,
            shutdown_token.clone(),
            task_tracker.clone(),
        ));

        Ok(Self {
            port,
            shutdown_token,
            task_handle: Some(task_handle),
            extra_task_handles,
            task_tracker,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn stop(&mut self) -> Result<()> {
        self.shutdown_token.cancel();
        self.task_tracker.close();
        let mut failures = Vec::new();
        //
        // Bound joins so disable/reset cannot hang forever. On timeout put
        // the handle back so ownership is not detached under a live task.
        //
        if let Some(mut handle) = self.task_handle.take() {
            match tokio::time::timeout(std::time::Duration::from_secs(5), &mut handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => failures.push(format!("primary listener task: {}", e)),
                Err(_) => {
                    handle.abort();
                    match tokio::time::timeout(std::time::Duration::from_secs(2), &mut handle).await
                    {
                        //
                        // Abort confirmed the task is gone and its socket
                        // released — a clean stop, not a failure. (Matches the
                        // additional-listener handling below.)
                        //
                        Ok(_) => {}
                        Err(_) => {
                            self.task_handle = Some(handle);
                            failures.push(
                                "primary listener join timed out; handle retained for retry".into(),
                            );
                        }
                    }
                }
            }
        }
        let mut retained_extra = Vec::new();
        for mut handle in self.extra_task_handles.drain(..) {
            match tokio::time::timeout(std::time::Duration::from_secs(5), &mut handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => failures.push(format!("additional listener task: {}", e)),
                Err(_) => {
                    handle.abort();
                    match tokio::time::timeout(std::time::Duration::from_secs(2), &mut handle).await
                    {
                        Ok(_) => {}
                        Err(_) => retained_extra.push(handle),
                    }
                }
            }
        }
        if !retained_extra.is_empty() {
            self.extra_task_handles = retained_extra;
            failures.push("additional listener task(s) retained after join timeout".into());
        }
        match tokio::time::timeout(std::time::Duration::from_secs(5), self.task_tracker.wait())
            .await
        {
            Ok(()) => {}
            Err(_) => failures.push("proxy task tracker wait timed out".into()),
        }
        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }
}

impl Drop for InterceptProxy {
    fn drop(&mut self) {
        self.shutdown_token.cancel();
        self.task_tracker.close();
        if let Some(handle) = &self.task_handle {
            handle.abort();
        }
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
    traffic_tx: TrafficSink,
    shutdown: CancellationToken,
    tasks: TaskTracker,
) {
    {
        let domains = config.intercept_domains.read().await;
        common::log_info!("Proxy server running, intercepting domains: {:?}", *domains);
    }

    accept_loop(
        listener,
        ca,
        config,
        traffic_tx,
        shutdown,
        tasks,
        "Proxy server",
    )
    .await;
}

/// Run the HTTP proxy server (port 80) for Hosts mode.
///
/// This handles plain HTTP connections which are less common for AI APIs
/// but may be needed for some services.
async fn run_proxy_http(
    listener: TcpListener,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: TrafficSink,
    shutdown: CancellationToken,
    tasks: TaskTracker,
) {
    common::log_info!("HTTP proxy server running on port 80 (Hosts mode)");

    accept_loop(
        listener,
        ca,
        config,
        traffic_tx,
        shutdown,
        tasks,
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
    traffic_tx: TrafficSink,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    label: &str,
) {
    //
    // The actual bound listen port (random for VPN/TPROXY). Used to distinguish
    // direct connects to the proxy from TPROXY redirects whose original port
    // is the real destination (e.g. 443).
    //
    let proxy_listen_port = listener
        .local_addr()
        .map(|addr| addr.port())
        .unwrap_or(0);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                common::log_info!("{} shutting down", label);
                break;
            }
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        if let Err(reason) =
                            admit_client_peer(config.intercept_method, addr.ip())
                        {
                            common::log_warn!(
                                "Rejected intercept peer {} ({}): {}",
                                addr,
                                config.intercept_method,
                                reason
                            );
                            continue;
                        }
                        let ca = Arc::clone(&ca);
                        let config = Arc::clone(&config);
                        let traffic_tx = traffic_tx.clone();
                        let connection_shutdown = shutdown.clone();
                        let connection_tasks = tasks.clone();

                        tasks.spawn(async move {
                            tokio::select! {
                                biased;
                                _ = connection_shutdown.cancelled() => {}
                                result = handle_connection(
                                    stream,
                                    addr,
                                    ca,
                                    config,
                                    traffic_tx,
                                    connection_shutdown.clone(),
                                    connection_tasks,
                                    proxy_listen_port,
                                ) => {
                                    if let Err(e) = result {
                                        common::log_debug!("Intercept connection ended: {}", e);
                                    }
                                }
                            }
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
    traffic_tx: TrafficSink,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    proxy_listen_port: u16,
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
        handle_tls_connection(stream, addr, ca, config, traffic_tx, proxy_listen_port).await
    } else {
        //
        // TPROXY must not serve plain HTTP/CONNECT to direct peers on the
        // wildcard bind — require a redirected local_addr first.
        //
        if config.intercept_method == InterceptMethod::Tproxy {
            require_tproxy_redirect(&stream, addr, proxy_listen_port)?;
        }

        let io = TokioIo::new(stream);

        //
        // Serve the connection with HTTP/1.1.
        //
        let service = service_fn(move |req| {
            let ca = Arc::clone(&ca);
            let config = Arc::clone(&config);
            let traffic_tx = traffic_tx.clone();
            let shutdown = shutdown.clone();
            let tasks = tasks.clone();
            async move {
                handle_request(req, addr, ca, config, traffic_tx, shutdown, tasks).await
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

//
// TPROXY preserves the original destination as the accepted socket's local
// address (getsockname / local_addr) under IP_TRANSPARENT — not via
// SO_ORIGINAL_DST (that is NAT/REDIRECT). Direct connects land on the proxy
// listen port and are rejected by admit_tproxy_redirect.
//
fn require_tproxy_redirect(
    stream: &TcpStream,
    peer: SocketAddr,
    proxy_listen_port: u16,
) -> Result<SocketAddr> {
    let local = stream
        .local_addr()
        .context("Failed to read accepted socket local address for TPROXY")?;
    match tproxy_original_destination_from_local_addr(local, proxy_listen_port) {
        Ok(dst) => Ok(dst),
        Err(reason) => anyhow::bail!(
            "Rejected TPROXY peer {} original_dst={}: {}",
            peer,
            local,
            reason
        ),
    }
}

/// Handle a direct TLS connection (VPN/TPROXY mode)
///
/// In VPN/TPROXY mode, clients connect directly with TLS, not via HTTP CONNECT.
/// We need to:
/// 1. Read ClientHello to extract SNI (for certificate selection)
/// 2. For TPROXY mode, use the accepted socket local_addr as original dest
/// 3. Perform TLS termination with our certificate
/// 4. Forward decrypted traffic to the real server
async fn handle_tls_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: TrafficSink,
    proxy_listen_port: u16,
) -> Result<()> {
    //
    // For TPROXY mode, the original destination is the accepted socket's
    // local_addr under IP_TRANSPARENT. Direct connections (local port equals
    // the proxy listen port) are rejected so the wildcard bind cannot be used
    // as an open forward proxy.
    //

    let original_dst = if config.intercept_method == InterceptMethod::Tproxy {
        let dst = require_tproxy_redirect(&stream, addr, proxy_listen_port)?;
        common::log_debug!("TPROXY: Original destination: {}", dst);
        Some(dst)
    } else {
        None
    };

    //
    // Read and retain the complete ClientHello. A single `peek` can return a
    // partial TLS record, which made valid connections fail nondeterministically.
    // The retained wire bytes are replayed into either the passthrough tunnel
    // or the rustls acceptor below.
    //
    let (client_hello_prefix, sni) = read_client_hello(&mut stream)
        .await
        .context("Failed to read SNI from ClientHello")?;
    let sni = sni.to_ascii_lowercase();

    //
    // Destination port from TPROXY local_addr, or default 443 for VPN TLS.
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
        // Tunnel bytes bidirectionally, replaying the ClientHello bytes that
        // were consumed while extracting SNI.
        //
        let client_stream = PrefixedStream::new(client_hello_prefix, stream);
        let (mut client_read, mut client_write) = tokio::io::split(client_stream);
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
        .accept(PrefixedStream::new(client_hello_prefix, stream))
        .await
        .context("TLS handshake with client failed")?;

    //
    // Now handle the decrypted traffic similar to CONNECT tunnel.
    //
    handle_intercepted_tunnel_vpn(tls_stream, &sni, dest_port, config, traffic_tx).await
}

async fn read_client_hello(stream: &mut TcpStream) -> Result<(Vec<u8>, String)> {
    use tokio::io::AsyncReadExt;

    const MAX_CLIENT_HELLO_BYTES: usize = 1024 * 1024;

    let mut wire = Vec::new();
    let mut handshake = Vec::new();
    let mut expected_handshake_len = None;

    loop {
        let mut record_header = [0u8; 5];
        stream.read_exact(&mut record_header).await?;
        if record_header[0] != 0x16 {
            anyhow::bail!("Expected a TLS handshake record");
        }
        let record_len = u16::from_be_bytes([record_header[3], record_header[4]]) as usize;
        if wire
            .len()
            .saturating_add(record_header.len())
            .saturating_add(record_len)
            > MAX_CLIENT_HELLO_BYTES
        {
            anyhow::bail!("TLS ClientHello exceeded the 1 MiB safety limit");
        }

        let mut record = vec![0u8; record_len];
        stream.read_exact(&mut record).await?;
        wire.extend_from_slice(&record_header);
        wire.extend_from_slice(&record);
        handshake.extend_from_slice(&record);

        if expected_handshake_len.is_none() && handshake.len() >= 4 {
            if handshake[0] != 0x01 {
                anyhow::bail!("First TLS handshake message was not ClientHello");
            }
            let message_len = ((handshake[1] as usize) << 16)
                | ((handshake[2] as usize) << 8)
                | handshake[3] as usize;
            let expected = 4usize
                .checked_add(message_len)
                .context("ClientHello length overflow")?;
            if expected > MAX_CLIENT_HELLO_BYTES {
                anyhow::bail!("TLS ClientHello exceeded the 1 MiB safety limit");
            }
            expected_handshake_len = Some(expected);
        }

        if let Some(expected) = expected_handshake_len
            && handshake.len() >= expected
        {
            let sni = parse_sni_from_client_hello(&handshake[..expected])?;
            return Ok((wire, sni));
        }
    }
}

/// Parse SNI (Server Name Indication) from a complete ClientHello handshake.
fn parse_sni_from_client_hello(handshake: &[u8]) -> Result<String> {
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
    pos = pos
        .checked_add(1 + session_id_len)
        .context("ClientHello session ID length overflow")?;

    if pos + 2 > handshake.len() {
        anyhow::bail!("ClientHello too short for cipher suites");
    }

    //
    // Skip cipher suites.
    //
    let cipher_suites_len = u16::from_be_bytes([handshake[pos], handshake[pos + 1]]) as usize;
    pos = pos
        .checked_add(2 + cipher_suites_len)
        .context("ClientHello cipher-suite length overflow")?;

    if pos + 1 > handshake.len() {
        anyhow::bail!("ClientHello too short for compression methods");
    }

    //
    // Skip compression methods.
    //
    let compression_len = handshake[pos] as usize;
    pos = pos
        .checked_add(1 + compression_len)
        .context("ClientHello compression length overflow")?;

    if pos + 2 > handshake.len() {
        anyhow::bail!("No extensions in ClientHello");
    }

    //
    // Extensions length.
    //
    let extensions_len = u16::from_be_bytes([handshake[pos], handshake[pos + 1]]) as usize;
    pos += 2;

    let extensions_end = pos
        .checked_add(extensions_len)
        .context("ClientHello extensions length overflow")?;
    if extensions_end > handshake.len() {
        anyhow::bail!("ClientHello extensions are truncated");
    }

    //
    // Parse extensions looking for SNI (type 0x0000).
    //
    while pos + 4 <= extensions_end && pos + 4 <= handshake.len() {
        let ext_type = u16::from_be_bytes([handshake[pos], handshake[pos + 1]]);
        let ext_len = u16::from_be_bytes([handshake[pos + 2], handshake[pos + 3]]) as usize;
        pos += 4;

        let extension_end = pos
            .checked_add(ext_len)
            .context("ClientHello extension length overflow")?;
        if extension_end > extensions_end {
            anyhow::bail!("ClientHello extension is truncated");
        }

        if ext_type == 0x0000 {
            //
            // SNI extension.
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
            while sni_pos + 3 <= extension_end {
                let name_type = handshake[sni_pos];
                let name_len =
                    u16::from_be_bytes([handshake[sni_pos + 1], handshake[sni_pos + 2]]) as usize;
                sni_pos += 3;

                let name_end = sni_pos
                    .checked_add(name_len)
                    .context("SNI name length overflow")?;
                if name_end > extension_end {
                    anyhow::bail!("SNI name is truncated");
                }

                if name_type == 0x00 {
                    //
                    // Host name type.
                    //
                    let sni = std::str::from_utf8(&handshake[sni_pos..name_end])
                        .context("Invalid SNI hostname")?;
                    return Ok(sni.to_string());
                }

                sni_pos = name_end;
            }
        }

        pos = extension_end;
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
        let mut addresses = target
            .to_socket_addrs()
            .context("Failed to resolve target address")?;
        if matches!(intercept_method, InterceptMethod::Vpn | InterceptMethod::Tproxy) {
            addresses
                .find(std::net::SocketAddr::is_ipv4)
                .context("VPN/TPROXY interception requires an IPv4 origin address")?
        } else {
            addresses.next().context("No addresses found for target")?
        }
    };

    //
    // Create socket.
    //
    let socket_domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(socket_domain, Type::STREAM, Some(Protocol::TCP))
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
            socket
                .set_mark(super::routing::VPN_BYPASS_MARK)
                .context("Failed to mark VPN bypass socket")?;
            let iface = discover_default_interface()
                .context("Failed to discover the VPN bypass interface")?;
            common::log_debug!("VPN bypass: binding to interface {}", iface);
            socket
                .bind_device(Some(iface.as_bytes()))
                .with_context(|| format!("Failed to bind VPN bypass socket to {}", iface))?;
        }
        InterceptMethod::Tproxy => {
            //
            // TPROXY mode: Only need SO_MARK so the iptables bypass rule
            // (-m mark --mark 0x2 -j RETURN) skips our outbound packets.
            //
            common::log_debug!("TPROXY bypass: setting SO_MARK=0x2");
            socket
                .set_mark(super::tproxy::TPROXY_BYPASS_MARK)
                .context("Failed to mark TPROXY bypass socket")?;
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
        let bind_ip = discover_non_tun_ip().context("Could not find a Windows VPN bypass IP")?;
        common::log_debug!("Windows VPN bypass: binding to {}", bind_ip);
        let bind_addr = std::net::SocketAddr::new(bind_ip, 0);
        socket
            .bind(&bind_addr.into())
            .with_context(|| format!("Failed to bind Windows VPN bypass socket to {}", bind_ip))?;
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
async fn handle_intercepted_tunnel_vpn<C>(
    client_tls: tokio_rustls::server::TlsStream<C>,
    host: &str,
    port: u16,
    config: Arc<ProxyConfig>,
    traffic_tx: TrafficSink,
) -> Result<()>
where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
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

    let mut server_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    if let Some(protocol) = client_tls.get_ref().1.alpn_protocol() {
        server_config.alpn_protocols = vec![protocol.to_vec()];
    }

    let connector = tokio_rustls::TlsConnector::from(Arc::new(server_config));
    let server_name = rustls_pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| anyhow::anyhow!("Invalid server name"))?;

    common::log_debug!("handle_intercepted_tunnel_vpn: starting TLS to {}", host);
    let server_tls = connector
        .connect(server_name, server_tcp)
        .await
        .context("Failed to establish TLS with server")?;
    ensure_compatible_alpn(
        client_tls.get_ref().1.alpn_protocol(),
        server_tls.get_ref().1.alpn_protocol(),
    )?;
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
    traffic_tx: TrafficSink,
    shutdown: CancellationToken,
    tasks: TaskTracker,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    if req.method() == Method::CONNECT {
        //
        // Handle HTTPS CONNECT tunnel.
        //
        handle_connect(req, ca, config, traffic_tx, shutdown, tasks).await
    } else {
        //
        // Handle plain HTTP request (forward as-is).
        //
        handle_http_request(req, config, traffic_tx, shutdown, tasks).await
    }
}

/// Handle HTTP CONNECT request for HTTPS tunneling
async fn handle_connect(
    req: Request<hyper::body::Incoming>,
    ca: Arc<RwLock<CertificateAuthority>>,
    config: Arc<ProxyConfig>,
    traffic_tx: TrafficSink,
    shutdown: CancellationToken,
    tasks: TaskTracker,
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
    tasks.spawn(async move {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => {}
            upgraded = hyper::upgrade::on(req) => {
                match upgraded {
                    Ok(upgraded) => {
                        tokio::select! {
                            biased;
                            _ = shutdown.cancelled() => {}
                            _ = tunnel(
                                upgraded,
                                &host,
                                port,
                                should_intercept,
                                ca,
                                &config,
                                &traffic_tx,
                            ) => {}
                        }
                    }
                    Err(e) => common::log_warn!("Upgrade error: {}", e),
                }
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
    traffic_tx: &TrafficSink,
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
    traffic_tx: &TrafficSink,
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

    let mut server_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    if let Some(protocol) = client_tls.get_ref().1.alpn_protocol() {
        server_config.alpn_protocols = vec![protocol.to_vec()];
    }

    let connector = tokio_rustls::TlsConnector::from(Arc::new(server_config));
    let server_name = rustls_pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| anyhow::anyhow!("Invalid server name"))?;

    let server_tls = connector
        .connect(server_name, server_tcp)
        .await
        .context("Failed to establish TLS with server")?;
    ensure_compatible_alpn(
        client_tls.get_ref().1.alpn_protocol(),
        server_tls.get_ref().1.alpn_protocol(),
    )?;

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
// HTTP/2 frame flags (RFC 7540 Section 6).
//

const H2_FLAG_END_HEADERS: u8 = 0x4;
const H2_FLAG_ACK: u8 = 0x1;
const H2_FLAG_PADDED: u8 = 0x8;
const H2_FLAG_PRIORITY: u8 = 0x20;

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
    traffic_tx: &TrafficSink,
    flow_id: &str,
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

    let agent = config.agent_for_host(host);
    let capture_config = config.capture_config_for_host(host);

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
        capture_config,
        traffic_tx,
        flow_id,
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
    capture_config: Option<&DomainCaptureConfig>,
    traffic_tx: &TrafficSink,
    flow_id: &str,
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
    let flow_id = flow_id.to_string();

    //
    // Track stream paths for logging context (stream_id -> path).
    //

    let mut stream_paths: std::collections::HashMap<u32, String> = std::collections::HashMap::new();

    //
    // Per-direction HPACK decoders. Kept for the lifetime of the connection
    // so the dynamic table stays in sync across frames.
    //

    let mut req_hpack = H2HeaderDecoder::new();
    let mut resp_hpack = H2HeaderDecoder::new();

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

                        if let Some(size) = settings_header_table_size(&frame) {
                            req_hpack.set_max_table_size(size);
                        }

                        //
                        // Log DATA frames (response body).
                        //

                        if let Some(data) = data_frame_payload(&frame).filter(|data| !data.is_empty()) {
                            let path = stream_paths
                                .get(&frame.stream_id)
                                .cloned()
                                .unwrap_or_else(|| format!("/stream/{}", frame.stream_id));
                            let url = format!("https://{}{}", host, path);

                            if let Some(matched_agent) =
                                capture_agent_label(capture_config, &agent, &url)
                            {
                                //
                                // Decompress gRPC payload for readability.
                                //

                                let decompressed = decompress_grpc_payload(data);

                                let entry = InterceptedTrafficEntry {
                                    id: None,
                                    timestamp: chrono::Utc::now(),
                                    node_id: node_id.clone(),
                                    agent_short_name: matched_agent,
                                    intercept_method,
                                    direction: TrafficDirection::Receive,
                                    method: Some(format!(
                                        "H2_DATA#{}:{}",
                                        flow_id, frame.stream_id
                                    )),
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
                        // Decode HEADERS/CONTINUATION/PUSH_PROMISE frames to
                        // keep the HPACK table in sync, and log response
                        // headers when a HEADERS block completes.
                        //

                        if let Some(decoded) = resp_hpack.feed(&frame)
                            && !decoded.is_push_promise
                        {
                            let path = stream_paths
                                .get(&decoded.stream_id)
                                .cloned()
                                .unwrap_or_else(|| format!("/stream/{}", decoded.stream_id));
                            let url = format!("https://{}{}", host, path);

                            if let Some(matched_agent) =
                                capture_agent_label(capture_config, &agent, &url)
                            {
                                let (headers, _, _, status) = split_h2_headers(&decoded.headers);
                                let entry = InterceptedTrafficEntry {
                                    id: None,
                                    timestamp: chrono::Utc::now(),
                                    node_id: node_id.clone(),
                                    agent_short_name: matched_agent,
                                    intercept_method,
                                    direction: TrafficDirection::Receive,
                                    method: Some(format!(
                                        "H2_HEADERS#{}:{}",
                                        flow_id, decoded.stream_id
                                    )),
                                    url: url.clone(),
                                    host: host.clone(),
                                    request_headers: None,
                                    request_body: None,
                                    response_status: status,
                                    response_headers: Some(headers),
                                    response_body: None,
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

                        if let Some(size) = settings_header_table_size(&frame) {
                            resp_hpack.set_max_table_size(size);
                        }

                        //
                        // Decode HEADERS/CONTINUATION frames. HPACK carries
                        // the :path pseudo-header, which we use both for
                        // stream-path tracking and to log request headers.
                        // Fall back to the byte-scan path heuristic while a
                        // block is incomplete or the decoder is poisoned.
                        //

                        match req_hpack.feed(&frame) {
                            Some(decoded) if !decoded.is_push_promise => {
                                let (headers, path, _method, _) =
                                    split_h2_headers(&decoded.headers);
                                if let Some(path) = path {
                                    stream_paths.insert(decoded.stream_id, path);
                                }

                                let path = stream_paths
                                    .get(&decoded.stream_id)
                                    .cloned()
                                    .unwrap_or_else(|| format!("/stream/{}", decoded.stream_id));
                                let url = format!("https://{}{}", host, path);

                                if let Some(matched_agent) =
                                    capture_agent_label(capture_config, &agent, &url)
                                {
                                    let entry = InterceptedTrafficEntry {
                                        id: None,
                                        timestamp: chrono::Utc::now(),
                                        node_id: node_id.clone(),
                                        agent_short_name: matched_agent,
                                        intercept_method,
                                        direction: TrafficDirection::Send,
                                        method: Some(format!(
                                            "H2_HEADERS#{}:{}",
                                            flow_id, decoded.stream_id
                                        )),
                                        url: url.clone(),
                                        host: host.clone(),
                                        request_headers: Some(headers),
                                        request_body: None,
                                        response_status: None,
                                        response_headers: None,
                                        response_body: None,
                                    };
                                    let _ = traffic_tx.try_send(entry);
                                }
                            }
                            //
                            // PUSH_PROMISE was fed only to keep the HPACK table
                            // synced; nothing to surface.
                            //
                            Some(_) => {}
                            //
                            // Block not yet decodable (spanning CONTINUATION,
                            // or decoder poisoned): still try to recover the
                            // path via byte-scan so DATA frames get a URL.
                            //
                            None if frame.frame_type == H2_FRAME_HEADERS => {
                                if let Some(fragment) = header_block_fragment(&frame)
                                    && let Some(path) = extract_path_from_headers(fragment)
                                {
                                    stream_paths.insert(frame.stream_id, path);
                                }
                            }
                            None => {}
                        }

                        //
                        // Log DATA frames (request body).
                        //

                        if let Some(data) = data_frame_payload(&frame).filter(|data| !data.is_empty()) {
                            let path = stream_paths
                                .get(&frame.stream_id)
                                .cloned()
                                .unwrap_or_else(|| format!("/stream/{}", frame.stream_id));
                            let url = format!("https://{}{}", host, path);

                            if let Some(matched_agent) =
                                capture_agent_label(capture_config, &agent, &url)
                            {
                                //
                                // Decompress gRPC payload for readability.
                                //

                                let decompressed = decompress_grpc_payload(data);

                                let entry = InterceptedTrafficEntry {
                                    id: None,
                                    timestamp: chrono::Utc::now(),
                                    node_id: node_id.clone(),
                                    agent_short_name: matched_agent,
                                    intercept_method,
                                    direction: TrafficDirection::Send,
                                    method: Some(format!(
                                        "H2_DATA#{}:{}",
                                        flow_id, frame.stream_id
                                    )),
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
// A completed, decoded HTTP/2 header block. `is_push_promise` blocks are
// decoded only to keep the HPACK dynamic table in sync and are not surfaced.
//

struct DecodedHeaders {
    stream_id: u32,
    is_push_promise: bool,
    headers: Vec<(String, String)>,
}

//
// Stateful HPACK decoder for one direction of an HTTP/2 connection. HPACK is
// stateful: every header block mutates a shared dynamic table, so blocks must
// be decoded in order and none may be skipped — even ones we won't surface —
// or the table desyncs and all later blocks decode to garbage. This wraps a
// per-direction decoder, reassembles HEADERS/PUSH_PROMISE + CONTINUATION
// spans, and poisons itself on the first decode error so it never emits
// corrupt headers.
//

struct H2HeaderDecoder {
    decoder: hpack::Decoder<'static>,
    pending: Vec<u8>,
    pending_stream: u32,
    pending_is_push: bool,
    in_block: bool,
    poisoned: bool,
}

impl H2HeaderDecoder {
    const MAX_HEADER_BLOCK_BYTES: usize = 1024 * 1024;
    const MAX_DYNAMIC_TABLE_BYTES: usize = 1024 * 1024;

    fn new() -> Self {
        Self {
            decoder: hpack::Decoder::new(),
            pending: Vec::new(),
            pending_stream: 0,
            pending_is_push: false,
            in_block: false,
            poisoned: false,
        }
    }

    //
    // Feed one frame. Returns the decoded headers when a full block (possibly
    // spanning CONTINUATION frames) completes; None while more fragments are
    // expected, for non-header frames, or once poisoned.
    //

    fn feed(&mut self, frame: &H2Frame) -> Option<DecodedHeaders> {
        if self.poisoned {
            return None;
        }

        if self.in_block
            && (frame.frame_type != H2_FRAME_CONTINUATION
                || frame.stream_id != self.pending_stream)
        {
            self.poison("invalid or interleaved CONTINUATION sequence");
            return None;
        }

        let (fragment, is_push) = match frame.frame_type {
            H2_FRAME_HEADERS => (header_block_fragment(frame)?, false),
            H2_FRAME_PUSH_PROMISE => (push_promise_block_fragment(frame)?, true),
            H2_FRAME_CONTINUATION if self.in_block => (frame.payload.as_slice(), self.pending_is_push),
            _ => return None,
        };

        if !self.in_block {
            self.in_block = true;
            self.pending_stream = frame.stream_id;
            self.pending_is_push = is_push;
            self.pending.clear();
        }
        if self.pending.len().saturating_add(fragment.len()) > Self::MAX_HEADER_BLOCK_BYTES {
            self.poison("header block exceeded 1 MiB capture limit");
            return None;
        }
        self.pending.extend_from_slice(fragment);

        if frame.flags & H2_FLAG_END_HEADERS == 0 {
            return None;
        }

        //
        // Block complete. Decode against the persistent dynamic table.
        //

        let block = std::mem::take(&mut self.pending);
        let stream_id = self.pending_stream;
        let is_push_promise = self.pending_is_push;
        self.in_block = false;

        match self.decoder.decode(&block) {
            Ok(pairs) => Some(DecodedHeaders {
                stream_id,
                is_push_promise,
                headers: pairs
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            String::from_utf8_lossy(&k).into_owned(),
                            String::from_utf8_lossy(&v).into_owned(),
                        )
                    })
                    .collect(),
            }),
            Err(e) => {
                self.poison(&format!("HPACK decode failed: {:?}", e));
                None
            }
        }
    }

    fn set_max_table_size(&mut self, size: usize) {
        if !self.poisoned {
            self.decoder
                .set_max_table_size(size.min(Self::MAX_DYNAMIC_TABLE_BYTES));
        }
    }

    fn poison(&mut self, reason: &str) {
        self.poisoned = true;
        self.in_block = false;
        self.pending.clear();
        common::log_warn!(
            "Disabling H2 header decode for this direction: {}",
            reason
        );
    }
}

fn settings_header_table_size(frame: &H2Frame) -> Option<usize> {
    if frame.frame_type != H2_FRAME_SETTINGS || frame.flags & H2_FLAG_ACK != 0 {
        return None;
    }
    frame.payload.chunks_exact(6).find_map(|setting| {
        let id = u16::from_be_bytes([setting[0], setting[1]]);
        (id == 0x1).then(|| {
            u32::from_be_bytes([setting[2], setting[3], setting[4], setting[5]]) as usize
        })
    })
}

fn ensure_compatible_alpn(client: Option<&[u8]>, server: Option<&[u8]>) -> Result<()> {
    if client == Some(b"h2".as_slice()) && server != Some(b"h2".as_slice()) {
        anyhow::bail!("Client negotiated HTTP/2 but the origin did not accept HTTP/2");
    }
    Ok(())
}

fn data_frame_payload(frame: &H2Frame) -> Option<&[u8]> {
    if frame.frame_type != H2_FRAME_DATA {
        return None;
    }
    if frame.flags & H2_FLAG_PADDED == 0 {
        return Some(&frame.payload);
    }
    let pad_len = *frame.payload.first()? as usize;
    let end = frame.payload.len().checked_sub(pad_len)?;
    frame.payload.get(1..end)
}

//
// Return the HPACK header-block fragment of a HEADERS frame, stripping the
// optional pad-length byte, trailing padding, and 5-byte priority prefix.
//

fn header_block_fragment(frame: &H2Frame) -> Option<&[u8]> {
    let payload = &frame.payload;
    let mut start = 0usize;
    let mut pad_len = 0usize;

    if frame.flags & H2_FLAG_PADDED != 0 {
        pad_len = *payload.first()? as usize;
        start += 1;
    }
    if frame.flags & H2_FLAG_PRIORITY != 0 {
        start += 5;
    }
    let end = payload.len().checked_sub(pad_len)?;
    payload.get(start..end)
}

//
// Return the header-block fragment of a PUSH_PROMISE frame, stripping the
// optional pad-length byte, the 4-byte promised stream id, and padding.
//

fn push_promise_block_fragment(frame: &H2Frame) -> Option<&[u8]> {
    let payload = &frame.payload;
    let mut start = 0usize;
    let mut pad_len = 0usize;

    if frame.flags & H2_FLAG_PADDED != 0 {
        pad_len = *payload.first()? as usize;
        start += 1;
    }
    start += 4; // promised stream id
    let end = payload.len().checked_sub(pad_len)?;
    payload.get(start..end)
}

//
// Turn decoded HPACK pairs into a display header map plus the pseudo-headers
// we surface separately (:path, :method, :status).
//

fn split_h2_headers(
    pairs: &[(String, String)],
) -> (IndexMap<String, String>, Option<String>, Option<String>, Option<u16>) {
    let mut headers = IndexMap::new();
    let mut path = None;
    let mut method = None;
    let mut status = None;
    for (k, v) in pairs {
        match k.as_str() {
            ":path" => path = Some(v.clone()),
            ":method" => method = Some(v.clone()),
            ":status" => status = v.parse::<u16>().ok(),
            _ => {}
        }
        headers.insert(k.clone(), v.clone());
    }
    (headers, path, method, status)
}

//
// Extract :path from HPACK-encoded headers.
// This is a simplified extraction that looks for common patterns.
// Used only as a fallback for stream-path context when full HPACK decoding
// is unavailable (decoder poisoned or block not yet complete).
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
    traffic_tx: &TrafficSink,
) -> Result<()>
where
    C: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let flow_id = uuid::Uuid::new_v4().simple().to_string();
    let negotiated_h2 = client_tls.get_ref().1.alpn_protocol() == Some(b"h2".as_slice());
    let origin_negotiated_h2 = server_tls.get_ref().1.alpn_protocol() == Some(b"h2".as_slice());
    let mut prefix = [0u8; 4];
    client_tls.read_exact(&mut prefix).await?;

    if negotiated_h2 || prefix == HTTP2_PREFACE_PREFIX {
        if !origin_negotiated_h2 {
            anyhow::bail!(
                "Client sent HTTP/2 but the origin connection did not negotiate HTTP/2"
            );
        }
        //
        // ALPN is authoritative for TLS HTTP/2. Read and validate the complete
        // connection preface exactly, without consuming bytes from SETTINGS.
        //

        let mut remainder = [0u8; 20];
        client_tls.read_exact(&mut remainder).await?;
        let mut preface = Vec::with_capacity(HTTP2_PREFACE.len());
        preface.extend_from_slice(&prefix);
        preface.extend_from_slice(&remainder);
        if preface != HTTP2_PREFACE {
            anyhow::bail!("Negotiated HTTP/2 but received an invalid connection preface");
        }

        common::log_info!("HTTP/2 detected for {}, using h2 proxy", host);
        let client_prefixed = PrefixedStream::new(Vec::new(), client_tls);
        return proxy_h2_traffic(
            client_prefixed,
            server_tls,
            host,
            config,
            traffic_tx,
            &flow_id,
        )
        .await;
    }

    //
    // HTTP/1.1 - continue with existing logic.
    // Prepend the peeked bytes back to the client stream.
    //

    common::log_debug!("proxy_https_traffic: HTTP/1.1 detected for {}", host);
    let client_prefixed = PrefixedStream::new(prefix.to_vec(), client_tls);

    let (client_read, mut client_write) = tokio::io::split(client_prefixed);
    let (server_read, mut server_write) = tokio::io::split(server_tls);

    let mut client_reader = BufReader::new(client_read);
    let mut server_reader = BufReader::new(server_read);

    let host = host.to_string();
    let config_node_id = config.node_id.clone();
    let configured_agent = config.agent_for_host(&host);
    let capture_config = config.capture_config_for_host(&host);

    //
    // Process requests from client.
    //
    common::log_debug!("proxy_https_traffic: starting for {}", host);
    loop {
        //
        // Read HTTP request from client.
        //
        let mut request_line = String::new();
        match timeout(
            Duration::from_secs(HTTP_HEADER_TIMEOUT_SECS),
            client_reader.read_line(&mut request_line),
        )
        .await
        {
            //
            // Connection closed.
            //
            Ok(Ok(0)) => {
                common::log_debug!("proxy_https_traffic: client closed (0 bytes)");
                break;
            }
            Ok(Ok(n)) => {
                common::log_debug!(
                    "proxy_https_traffic: read {} bytes: {:?}",
                    n,
                    request_line.trim()
                );
            }
            Ok(Err(e)) => {
                common::log_warn!("proxy_https_traffic: read error: {}", e);
                break;
            }
            Err(_) => break,
        }

        if request_line.len() > MAX_HTTP_HEADER_LINE_SIZE {
            anyhow::bail!("HTTP request line exceeds safety limit");
        }

        if request_line.trim().is_empty() {
            continue;
        }

        //
        // Parse request line.
        //
        let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
        if parts.len() != 3 || !matches!(parts[2], "HTTP/1.0" | "HTTP/1.1") {
            anyhow::bail!("Invalid HTTP request line");
        }
        let method = parts[0].to_string();
        let path = parts[1].to_string();
        let url = format!("https://{}{}", host, path);
        let agent = capture_config
            .and_then(|capture| capture.agent_label_for_url(&url))
            .unwrap_or_else(|| configured_agent.clone());

        //
        // Read headers - preserve original case for forwarding and logging.
        //
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut content_length: Option<usize> = None;
        let mut transfer_encodings = Vec::new();
        let mut header_bytes = request_line.len();
        loop {
            let mut header_line = String::new();
            let bytes_read = timeout(
                Duration::from_secs(HTTP_HEADER_TIMEOUT_SECS),
                client_reader.read_line(&mut header_line),
            )
            .await
            .context("Timed out reading request header")?
            .context("Failed to read request header")?;
            if bytes_read == 0 {
                anyhow::bail!("Client connection closed in request headers");
            }
            header_bytes = header_bytes.saturating_add(bytes_read);
            if header_line.len() > MAX_HTTP_HEADER_LINE_SIZE
                || header_bytes > MAX_HTTP_HEADER_SECTION_SIZE
            {
                anyhow::bail!("HTTP request headers exceed safety limit");
            }
            let line = header_line.trim();
            if line.is_empty() {
                break;
            }
            if let Some((key, value)) = line.split_once(':') {
                let original_key = key.trim().to_string();
                let value = value.trim().to_string();
                if original_key.eq_ignore_ascii_case("content-length") {
                    let parsed = value.parse::<usize>().context("Invalid Content-Length")?;
                    if content_length.is_some_and(|length| length != parsed) {
                        anyhow::bail!("Conflicting Content-Length request headers");
                    }
                    content_length = Some(parsed);
                }
                if original_key.eq_ignore_ascii_case("transfer-encoding") {
                    transfer_encodings.extend(
                        value
                            .split(',')
                            .map(|encoding| encoding.trim().to_ascii_lowercase())
                            .filter(|encoding| !encoding.is_empty()),
                    );
                }
                headers.push((original_key, value));
            } else {
                anyhow::bail!("Malformed HTTP request header");
            }
        }
        let request_chunked = transfer_encodings
            .last()
            .is_some_and(|encoding| encoding == "chunked");
        if transfer_encodings
            .iter()
            .filter(|encoding| encoding.as_str() == "chunked")
            .count()
            > 1
            || (transfer_encodings.iter().any(|encoding| encoding == "chunked")
                && !request_chunked)
        {
            anyhow::bail!("Invalid chunked Transfer-Encoding request");
        }
        if !transfer_encodings.is_empty() && !request_chunked {
            anyhow::bail!("Unsupported request Transfer-Encoding");
        }
        if !transfer_encodings.is_empty() && content_length.is_some() {
            anyhow::bail!("Ambiguous request with both chunked encoding and Content-Length");
        }
        //
        // Convert to IndexMap for logging (preserves original order and case).
        //
        let headers_map: IndexMap<String, String> = headers.iter().cloned().collect();
        let request_close = headers.iter().any(|(key, value)| {
            key.eq_ignore_ascii_case("connection")
                && value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("close"))
        });
        let expect_continue = headers.iter().any(|(key, value)| {
            key.eq_ignore_ascii_case("expect")
                && value.eq_ignore_ascii_case("100-continue")
        });

        //
        // Forward request headers, then stream its body while retaining only
        // the bounded capture copy.
        //
        server_write.write_all(request_line.as_bytes()).await?;
        for (key, value) in &headers {
            server_write
                .write_all(format!("{}: {}\r\n", key, value).as_bytes())
                .await?;
        }
        server_write.write_all(b"\r\n").await?;
        server_write.flush().await?;
        let early_response = if expect_continue {
            timeout(
                Duration::from_secs(HTTP_HEADER_TIMEOUT_SECS),
                relay_expect_continue(&mut server_reader, &mut client_write, &method),
            )
            .await
            .context("Timed out waiting for 100 Continue")??
        } else {
            None
        };
        let close_after_rejection = early_response.is_some();
        let body = if close_after_rejection {
            Vec::new()
        } else if request_chunked {
            stream_chunked_body(&mut client_reader, &mut server_write).await?
        } else if let Some(length) = content_length.filter(|length| *length > 0) {
            stream_fixed_body(&mut client_reader, &mut server_write, length).await?
        } else {
            Vec::new()
        };
        server_write.flush().await?;

        //
        // Read response headers from server with timeout (30 seconds for
        // headers only).
        //
        let headers_result: std::result::Result<Result<HttpResponseHead>, _> =
            if let Some(response) = early_response {
                Ok(Ok(response))
            } else {
                timeout(
                    Duration::from_secs(HTTP_HEADER_TIMEOUT_SECS),
                    read_final_response_headers(&mut server_reader, &mut client_write, &method),
                )
                .await
            };

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
                let should_collect = capture_config
                    .map(|capture| capture.matches_url(&url))
                    .unwrap_or(true);
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
                break;
            }
            Err(_) => {
                //
                // Timeout waiting for response headers.
                //
                common::log_warn!(
                    "Intercepted [TIMEOUT]: {} {} - no response headers after {}s",
                    method,
                    url,
                    HTTP_HEADER_TIMEOUT_SECS
                );

                //
                // Record request without response if pattern matches.
                //
                let should_collect = capture_config
                    .map(|capture| capture.matches_url(&url))
                    .unwrap_or(true);
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
                break;
            }
        };
        let response_close = response_headers.iter().any(|(key, value)| {
            key.eq_ignore_ascii_case("connection")
                && value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("close"))
        });
        let close_after_response =
            request_close
                || close_after_rejection
                || response_close
                || matches!(body_type, ResponseBodyType::UntilEof);

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
        let response_headers_map: IndexMap<String, String> =
            response_headers.iter().cloned().collect();

        //
        // Read and forward body based on type.
        //
        let response_body = match body_type {
            ResponseBodyType::None => Vec::new(),
            ResponseBodyType::Chunked => stream_chunked_body(
                &mut server_reader,
                &mut client_write,
            )
            .await
            .with_context(|| format!("Failed to stream chunked response for {}", url))?,
            ResponseBodyType::ContentLength(len) => stream_fixed_body(
                &mut server_reader,
                &mut client_write,
                len,
            )
            .await
            .with_context(|| format!("Failed to stream fixed response for {}", url))?,
            ResponseBodyType::UntilEof => stream_until_eof(
                &mut server_reader,
                &mut client_write,
            )
            .await
            .with_context(|| format!("Failed to stream close-delimited response for {}", url))?,
        };

        //
        // Check for WebSocket upgrade (101 Switching Protocols). Require both
        // Upgrade: websocket and a Connection: upgrade token — matching the
        // plain-path checks so a 101 alone cannot open a WS tunnel.
        //
        let is_websocket_upgrade = status_code == Some(101)
            && header_has_token(&response_headers, "upgrade", "websocket")
            && header_has_token(&response_headers, "connection", "upgrade");

        if is_websocket_upgrade {
            //
            // Log the upgrade request.
            //
            let should_collect = capture_config
                .map(|capture| capture.matches_url(&url))
                .unwrap_or(true);

            if should_collect {
                let request_entry = InterceptedTrafficEntry {
                    id: None,
                    timestamp: chrono::Utc::now(),
                    node_id: config_node_id.clone(),
                    agent_short_name: agent.clone(),
                    intercept_method: config.intercept_method,
                    direction: TrafficDirection::Send,
                    method: Some(format!("WS_UPGRADE#{}", flow_id)),
                    url: url.clone(),
                    host: host.clone(),
                    request_headers: Some(headers_map.clone()),
                    request_body: None,
                    response_status: None,
                    response_headers: None,
                    response_body: None,
                };
                let _ = traffic_tx.try_send(request_entry);

                let response_entry = InterceptedTrafficEntry {
                    id: None,
                    timestamp: chrono::Utc::now(),
                    node_id: config_node_id.clone(),
                    agent_short_name: agent.clone(),
                    intercept_method: config.intercept_method,
                    direction: TrafficDirection::Receive,
                    method: Some(format!("WS_UPGRADE#{}", flow_id)),
                    url: url.clone(),
                    host: host.clone(),
                    request_headers: None,
                    request_body: None,
                    response_status: status_code,
                    response_headers: Some(response_headers_map.clone()),
                    response_body: None,
                };
                let _ = traffic_tx.try_send(response_entry);
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
                capture_config,
                true,
                traffic_tx,
                &flow_id,
            )
            .await?;

            return Ok(());
        }

        //
        // Check if URL matches the pattern (if any)
        // Uses fancy-regex to support negative lookahead, e.g.,
        // ^(?!.*pacman).*$.
        //
        let should_collect = capture_config
            .map(|capture| capture.matches_url(&url))
            .unwrap_or(true);

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
            let request_entry = InterceptedTrafficEntry {
                id: None,
                timestamp: chrono::Utc::now(),
                node_id: config_node_id.clone(),
                agent_short_name: agent.clone(),
                intercept_method: config.intercept_method,
                direction: TrafficDirection::Send,
                method: Some(method.clone()),
                url: url.clone(),
                host: host.clone(),
                request_headers: Some(headers_map),
                request_body: if body.is_empty() { None } else { Some(body) },
                response_status: None,
                response_headers: None,
                response_body: None,
            };
            let _ = traffic_tx.try_send(request_entry);

            let response_entry = InterceptedTrafficEntry {
                id: None,
                timestamp: chrono::Utc::now(),
                node_id: config_node_id.clone(),
                agent_short_name: agent.clone(),
                intercept_method: config.intercept_method,
                direction: TrafficDirection::Receive,
                method: Some(method),
                url,
                host: host.clone(),
                request_headers: None,
                request_body: None,
                response_status: status_code,
                response_headers: Some(response_headers_map),
                response_body: if decompressed_body.is_empty() {
                    None
                } else {
                    Some(decompressed_body)
                },
            };
            let _ = traffic_tx.try_send(response_entry);
        }

        if close_after_response {
            break;
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
    capture_config: Option<&DomainCaptureConfig>,
    capture_enabled: bool,
    traffic_tx: &TrafficSink,
    flow_id: &str,
) -> Result<()>
where
    CR: tokio::io::AsyncRead + Unpin + Send,
    CW: tokio::io::AsyncWrite + Unpin + Send,
    SR: tokio::io::AsyncRead + Unpin + Send,
    SW: tokio::io::AsyncWrite + Unpin + Send,
{
    let should_collect = capture_enabled
        && capture_config
            .map(|capture| capture.matches_url(url))
            .unwrap_or(true);

    let url = url.to_string();
    let host = host.to_string();
    let agent = agent.to_string();
    let node_id = node_id.to_string();
    let flow_id = flow_id.to_string();
    let mut server_fragments: Option<WebSocketFragments> = None;
    let mut client_fragments: Option<WebSocketFragments> = None;

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
            result = read_websocket_frame(&mut server_read, false) => {
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
                        // Reassemble fragmented data messages for capture.
                        //
                        let collected = match collect_websocket_message(
                            fin,
                            opcode,
                            payload,
                            &mut server_fragments,
                        ) {
                            Ok(collected) => collected,
                            Err(_) => break,
                        };
                        if should_collect
                            && let Some((message_opcode, message)) = collected
                        {
                            let msg_type = if message_opcode == 0x1 { "TEXT" } else { "BINARY" };
                            let entry = InterceptedTrafficEntry {
                                id: None,
                                timestamp: chrono::Utc::now(),
                                node_id: node_id.clone(),
                                agent_short_name: agent.clone(),
                                intercept_method,
                                direction: TrafficDirection::Receive,
                                method: Some(format!("WS_{}#{}", msg_type, flow_id)),
                                url: url.clone(),
                                host: host.clone(),
                                request_headers: None,
                                request_body: None,
                                response_status: None,
                                response_headers: None,
                                response_body: Some(message),
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
            result = read_websocket_frame(&mut client_read, true) => {
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
                        // Reassemble fragmented data messages for capture.
                        //
                        let collected = match collect_websocket_message(
                            fin,
                            opcode,
                            payload,
                            &mut client_fragments,
                        ) {
                            Ok(collected) => collected,
                            Err(_) => break,
                        };
                        if should_collect
                            && let Some((message_opcode, message)) = collected
                        {
                            let msg_type = if message_opcode == 0x1 { "TEXT" } else { "BINARY" };
                            let entry = InterceptedTrafficEntry {
                                id: None,
                                timestamp: chrono::Utc::now(),
                                node_id: node_id.clone(),
                                agent_short_name: agent.clone(),
                                intercept_method,
                                direction: TrafficDirection::Send,
                                method: Some(format!("WS_{}#{}", msg_type, flow_id)),
                                url: url.clone(),
                                host: host.clone(),
                                request_headers: None,
                                request_body: Some(message),
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

struct WebSocketFragments {
    opcode: u8,
    body: Option<Vec<u8>>,
}

fn collect_websocket_message(
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
    pending: &mut Option<WebSocketFragments>,
) -> Result<Option<(u8, Vec<u8>)>> {
    match opcode {
        0x1 | 0x2 if fin => {
            if pending.is_some() {
                anyhow::bail!("New WebSocket data frame during fragmented message");
            }
            let mut payload = payload;
            if payload.len() > MAX_CAPTURE_BODY_SIZE {
                common::log_warn!(
                    "Truncating captured WebSocket message from {} to {} bytes",
                    payload.len(),
                    MAX_CAPTURE_BODY_SIZE
                );
                payload.truncate(MAX_CAPTURE_BODY_SIZE);
            }
            Ok(Some((opcode, payload)))
        }
        0x1 | 0x2 => {
            if pending.is_some() {
                anyhow::bail!("New WebSocket data frame during fragmented message");
            }
            let body = if payload.len() <= MAX_CAPTURE_BODY_SIZE {
                Some(payload)
            } else {
                common::log_warn!("Skipping oversized fragmented WebSocket message");
                None
            };
            *pending = Some(WebSocketFragments { opcode, body });
            Ok(None)
        }
        0x0 => {
            let Some(fragments) = pending.as_mut() else {
                anyhow::bail!("WebSocket continuation without fragmented message");
            };
            if let Some(body) = fragments.body.as_mut() {
                if body.len().saturating_add(payload.len()) > MAX_CAPTURE_BODY_SIZE {
                    fragments.body = None;
                    common::log_warn!("Skipping oversized fragmented WebSocket message");
                } else {
                    body.extend_from_slice(&payload);
                }
            }
            if fin {
                let Some(fragments) = pending.take() else {
                    anyhow::bail!("WebSocket fragment state disappeared");
                };
                Ok(fragments.body.map(|body| (fragments.opcode, body)))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

/// Read a WebSocket frame, returning (fin, opcode, payload)
async fn read_websocket_frame<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    expect_masked: bool,
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
    if header[0] & 0x70 != 0 {
        anyhow::bail!("WebSocket extensions are not supported");
    }
    if !matches!(opcode, 0x0 | 0x1 | 0x2 | 0x8 | 0x9 | 0xA) {
        anyhow::bail!("Invalid WebSocket opcode");
    }
    if masked != expect_masked {
        anyhow::bail!("Invalid WebSocket masking direction");
    }

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
        if ext[0] & 0x80 != 0 {
            anyhow::bail!("Invalid WebSocket payload length");
        }
        payload_len = u64::from_be_bytes(ext);
    }

    if opcode >= 0x8 && (!fin || payload_len > 125) {
        anyhow::bail!("Invalid fragmented or oversized WebSocket control frame");
    }

    const MAX_WEBSOCKET_FRAME_SIZE: u64 = 16 * 1024 * 1024;
    if payload_len > MAX_WEBSOCKET_FRAME_SIZE {
        anyhow::bail!(
            "WebSocket frame length {} exceeds the {} byte safety limit",
            payload_len,
            MAX_WEBSOCKET_FRAME_SIZE
        );
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

const MAX_PLAIN_HTTP_BODY_SIZE: usize = 64 * 1024 * 1024;

fn bad_gateway_response(message: &'static str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(Full::new(Bytes::from(message)))
        .unwrap()
}

fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

//
// VPN client traffic is NAT'd from the synthetic TUN client (10.255.0.100 /
// fd00:255:0::100). Admit any host in the TUN /24 (or matching ULA prefix)
// so only TUN-path peers reach the VPN-bound listener.
//
pub(crate) fn is_tun_client_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 10 && octets[1] == 255 && octets[2] == 0
        }
        std::net::IpAddr::V6(v6) => {
            let segments = v6.segments();
            segments[0] == 0xfd00 && segments[1] == 0x0255 && segments[2] == 0
        }
    }
}

//
// Source admission for accepted TCP peers. TPROXY uses local_addr-based
// redirect admission separately because a wildcard bind is required.
//
pub(crate) fn admit_client_peer(
    method: InterceptMethod,
    peer_ip: std::net::IpAddr,
) -> Result<(), &'static str> {
    match method {
        InterceptMethod::Proxy | InterceptMethod::Hosts => {
            if peer_ip.is_loopback() {
                Ok(())
            } else {
                Err("loopback-only listener")
            }
        }
        InterceptMethod::Vpn => {
            if is_tun_client_ip(peer_ip) {
                Ok(())
            } else {
                Err("VPN peer must come from the Praxis TUN subnet")
            }
        }
        InterceptMethod::Tproxy => Ok(()),
    }
}

//
// Admit TPROXY connections using the accepted socket local address as the
// original destination (IP_TRANSPARENT). Direct connects to the proxy's
// wildcard listen port show local_addr.port() == proxy_listen_port. Real
// redirects preserve the original destination port (typically 443/80).
//
pub(crate) fn admit_tproxy_redirect(
    original_dst: SocketAddr,
    proxy_listen_port: u16,
) -> Result<(), &'static str> {
    if original_dst.ip().is_loopback() {
        return Err("loopback original destination is not a TPROXY redirect");
    }
    if original_dst.port() == proxy_listen_port {
        return Err(
            "original destination port is the proxy listen port (direct connect, not redirected)",
        );
    }
    Ok(())
}

//
// Resolve the destination used for TPROXY admission from an accepted peer.
// Pure helper so unit tests can pass fabricated local addresses without sockets.
//
pub(crate) fn tproxy_original_destination_from_local_addr(
    local_addr: SocketAddr,
    proxy_listen_port: u16,
) -> Result<SocketAddr, &'static str> {
    admit_tproxy_redirect(local_addr, proxy_listen_port)?;
    Ok(local_addr)
}

//
// Forward headers for plain WebSocket upgrades: keep Connection/Upgrade/
// Sec-WebSocket-* and application headers; drop proxy credentials and other
// hop-by-hop / connection-nominated fields.
//
pub(crate) fn should_forward_websocket_request_header(
    name: &str,
    connection_tokens: &HashSet<String>,
) -> bool {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "proxy-authorization" | "proxy-authenticate" | "proxy-connection"
    ) {
        return false;
    }
    if lower == "connection" || lower == "upgrade" || lower.starts_with("sec-websocket-") {
        return true;
    }
    if is_hop_by_hop_header(name) {
        return false;
    }
    if connection_tokens.contains(&lower) {
        return false;
    }
    true
}

fn connection_header_tokens(headers: &[(String, String)]) -> HashSet<String> {
    headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("connection"))
        .flat_map(|(_, value)| value.split(','))
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| !token.is_empty())
        .collect()
}

//
// True when any header named `name` contains a comma-separated token equal
// to `token` (ASCII case-insensitive). Used for Upgrade/Connection checks.
//
fn header_has_token(headers: &[(String, String)], name: &str, token: &str) -> bool {
    let token = token.to_ascii_lowercase();
    headers.iter().any(|(header_name, value)| {
        header_name.eq_ignore_ascii_case(name)
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(&token))
    })
}

//
// Validate HTTP/1.x status lines: version SP 3-digit status [SP reason] CR?LF.
// Rejects garbage like missing version, non-numeric codes, or codes outside
// the 100-599 range.
//
fn parse_http_status_line(line: &str) -> Result<u16> {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    let mut parts = trimmed.splitn(3, ' ');
    let version = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing HTTP version"))?;
    if version != "HTTP/1.0" && version != "HTTP/1.1" {
        anyhow::bail!("unsupported HTTP version in status line");
    }
    let code_str = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing status code"))?;
    if code_str.len() != 3 || !code_str.bytes().all(|b| b.is_ascii_digit()) {
        anyhow::bail!("status code must be three digits");
    }
    let code: u16 = code_str
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid status code"))?;
    if !(100..=599).contains(&code) {
        anyhow::bail!("status code out of range");
    }
    Ok(code)
}

fn should_strip_proxy_header(name: &str, connection_tokens: &HashSet<String>) -> bool {
    is_hop_by_hop_header(name) || connection_tokens.contains(&name.to_ascii_lowercase())
}

#[cfg(test)]
mod protocol_tests {
    use super::{
        admit_client_peer, admit_tproxy_redirect, connection_header_tokens, header_has_token,
        parse_http_status_line, should_forward_websocket_request_header,
        tproxy_original_destination_from_local_addr,
    };
    use common::InterceptMethod;
    use std::net::SocketAddr;

    #[test]
    fn status_line_accepts_http11() {
        assert_eq!(parse_http_status_line("HTTP/1.1 200 OK\r\n").unwrap(), 200);
        assert_eq!(parse_http_status_line("HTTP/1.0 404 Not Found").unwrap(), 404);
    }

    #[test]
    fn status_line_rejects_garbage() {
        assert!(parse_http_status_line("OK 200").is_err());
        assert!(parse_http_status_line("HTTP/2 200 OK").is_err());
        assert!(parse_http_status_line("HTTP/1.1 20 OK").is_err());
        assert!(parse_http_status_line("HTTP/1.1 999 OK").is_err());
        assert!(parse_http_status_line("HTTP/1.1 abc OK").is_err());
    }

    #[test]
    fn header_token_matches_comma_list() {
        let headers = vec![
            ("Connection".into(), "keep-alive, Upgrade".into()),
            ("Upgrade".into(), "websocket".into()),
        ];
        assert!(header_has_token(&headers, "connection", "upgrade"));
        assert!(header_has_token(&headers, "upgrade", "websocket"));
        assert!(!header_has_token(&headers, "connection", "close"));
    }

    #[test]
    fn admit_client_peer_policy() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

        assert!(admit_client_peer(
            InterceptMethod::Proxy,
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        )
        .is_ok());
        assert!(admit_client_peer(
            InterceptMethod::Proxy,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5))
        )
        .is_err());
        assert!(admit_client_peer(
            InterceptMethod::Vpn,
            IpAddr::V4(Ipv4Addr::new(10, 255, 0, 100))
        )
        .is_ok());
        assert!(admit_client_peer(
            InterceptMethod::Vpn,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))
        )
        .is_err());
        assert!(admit_client_peer(
            InterceptMethod::Vpn,
            IpAddr::V6("fd00:255:0::100".parse::<Ipv6Addr>().unwrap().into())
        )
        .is_ok());
        assert!(admit_client_peer(
            InterceptMethod::Tproxy,
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))
        )
        .is_ok());
    }

    #[test]
    fn admit_tproxy_redirect_policy() {
        let proxy_listen_port = 45678u16;
        let loopback: SocketAddr = "127.0.0.1:443".parse().unwrap();
        //
        // With IP_TRANSPARENT, the accepted socket local_addr IS the original
        // destination for redirected flows (e.g. remote:443).
        //
        let redirected_local: SocketAddr = "93.184.216.34:443".parse().unwrap();
        assert_eq!(
            tproxy_original_destination_from_local_addr(redirected_local, proxy_listen_port)
                .unwrap(),
            redirected_local
        );
        assert!(admit_tproxy_redirect(loopback, proxy_listen_port).is_err());
        //
        // Direct connect: local_addr port equals the proxy listen port.
        //
        let direct: SocketAddr = format!("10.0.0.5:{}", proxy_listen_port).parse().unwrap();
        assert!(tproxy_original_destination_from_local_addr(direct, proxy_listen_port).is_err());
        assert!(admit_tproxy_redirect(direct, proxy_listen_port).is_err());
    }

    #[test]
    fn websocket_header_filter_strips_proxy_credentials() {
        let headers = vec![
            ("Connection".into(), "Upgrade".into()),
            ("Upgrade".into(), "websocket".into()),
            ("Sec-WebSocket-Key".into(), "abc".into()),
            ("Proxy-Authorization".into(), "Basic secret".into()),
            ("Proxy-Authenticate".into(), "Basic".into()),
            ("Proxy-Connection".into(), "keep-alive".into()),
            ("Keep-Alive".into(), "timeout=5".into()),
            ("X-App".into(), "1".into()),
        ];
        let tokens = connection_header_tokens(&headers);
        assert!(should_forward_websocket_request_header(
            "Connection",
            &tokens
        ));
        assert!(should_forward_websocket_request_header("Upgrade", &tokens));
        assert!(should_forward_websocket_request_header(
            "Sec-WebSocket-Key",
            &tokens
        ));
        assert!(should_forward_websocket_request_header("X-App", &tokens));
        assert!(!should_forward_websocket_request_header(
            "Proxy-Authorization",
            &tokens
        ));
        assert!(!should_forward_websocket_request_header(
            "Proxy-Authenticate",
            &tokens
        ));
        assert!(!should_forward_websocket_request_header(
            "Proxy-Connection",
            &tokens
        ));
        assert!(!should_forward_websocket_request_header(
            "Keep-Alive",
            &tokens
        ));
    }
}

/// Handle plain HTTP request (non-CONNECT) - forward to target server
async fn handle_http_request(
    mut req: Request<hyper::body::Incoming>,
    config: Arc<ProxyConfig>,
    traffic_tx: TrafficSink,
    shutdown: CancellationToken,
    tasks: TaskTracker,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let method_str = method.to_string();
    if matches!(uri.scheme_str(), Some("https" | "wss")) {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Full::new(Bytes::from(
                "TLS proxy requests must use CONNECT",
            )))
            .unwrap());
    }

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
                    match host_header.parse::<hyper::http::uri::Authority>() {
                        Ok(authority) => (
                            authority.host().to_string(),
                            authority.port_u16().unwrap_or(80),
                        ),
                        Err(_) => {
                            return Ok(Response::builder()
                                .status(StatusCode::BAD_REQUEST)
                                .body(Full::new(Bytes::from("Invalid host")))
                                .unwrap());
                        }
                    }
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

    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let url_str = if uri.scheme().is_some() {
        uri.to_string()
    } else {
        format!("http://{}{}", host, path)
    };

    //
    // Check if this is a WebSocket upgrade.
    //
    let has_websocket_upgrade = req
        .headers()
        .get_all("upgrade")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case("websocket"));
    let has_connection_upgrade = req
        .headers()
        .get_all("connection")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case("upgrade"));
    let is_websocket = has_websocket_upgrade && has_connection_upgrade && method == Method::GET;
    if has_websocket_upgrade && !is_websocket {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Full::new(Bytes::from("Invalid WebSocket upgrade request")))
            .unwrap());
    }
    //
    // Collect request headers and body - preserve order and case.
    //
    let request_header_lines: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let req_headers: IndexMap<String, String> = request_header_lines.iter().cloned().collect();

    if is_websocket {
        return handle_plain_websocket(
            req,
            host,
            port,
            path.to_string(),
            url_str,
            method_str,
            request_header_lines,
            config,
            traffic_tx,
            shutdown,
            tasks,
        )
        .await;
    }

    let mut body_bytes = Vec::new();
    while let Some(frame) = req.body_mut().frame().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(e) => {
                common::log_error!("Failed to collect request body: {}", e);
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::from("Failed to read request body")))
                    .unwrap());
            }
        };
        if let Some(data) = frame.data_ref() {
            if body_bytes.len().saturating_add(data.len()) > MAX_PLAIN_HTTP_BODY_SIZE {
                return Ok(Response::builder()
                    .status(StatusCode::PAYLOAD_TOO_LARGE)
                    .body(Full::new(Bytes::from("Request body is too large")))
                    .unwrap());
            }
            body_bytes.extend_from_slice(data);
        }
    }

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
    use tokio::io::{AsyncWriteExt, BufReader};
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
    let request_connection_tokens = connection_header_tokens(&request_header_lines);
    for (key, value) in &request_header_lines {
        if should_strip_proxy_header(key, &request_connection_tokens)
            || key.eq_ignore_ascii_case("content-length")
            || key.eq_ignore_ascii_case("transfer-encoding")
        {
            continue;
        }
        let header_line = format!("{}: {}\r\n", key, value);
        if writer.write_all(header_line.as_bytes()).await.is_err() {
            return Ok(bad_gateway_response("Failed to forward request headers"));
        }
    }
    if writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body_bytes.len()).as_bytes())
        .await
        .is_err()
    {
        return Ok(bad_gateway_response("Failed to forward request headers"));
    }

    //
    // Send body.
    //
    if !body_bytes.is_empty() {
        if writer.write_all(&body_bytes).await.is_err() {
            return Ok(bad_gateway_response("Failed to forward request body"));
        }
    }
    if writer.flush().await.is_err() {
        return Ok(bad_gateway_response("Failed to flush forwarded request"));
    }

    //
    // Read response.
    //
    let mut interim_sink = tokio::io::sink();
    let (_, status_code, response_headers, body_type) = match timeout(
        Duration::from_secs(HTTP_HEADER_TIMEOUT_SECS),
        read_final_response_headers(&mut reader, &mut interim_sink, &method_str),
    )
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => {
            common::log_error!("Failed to read response: {}", e);
            return Ok(bad_gateway_response("Failed to read response"));
        }
        Err(_) => return Ok(bad_gateway_response("Timed out reading response")),
    };
    let status_code = status_code.unwrap_or(502);
    let resp_headers: IndexMap<String, String> = response_headers.iter().cloned().collect();
    let response_connection_tokens = connection_header_tokens(&response_headers);
    let content_encoding = response_headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-encoding"))
        .map(|(_, value)| value.clone());

    //
    // Read response body.
    //
    let response_body = match read_plain_http_body(&mut reader, body_type).await {
        Ok(body) => body,
        Err(e) => {
            common::log_error!("Failed to read response body: {}", e);
            return Ok(bad_gateway_response("Failed to read response body"));
        }
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
        let configured_agent = config.agent_for_host(&host);
        if let Some(agent) = capture_agent_label(
            config.capture_config_for_host(&host),
            &configured_agent,
            &url_str,
        ) {
            let decompressed_body = decompress_body(&response_body, content_encoding.as_deref());

            let request_entry = InterceptedTrafficEntry {
                id: None,
                timestamp: chrono::Utc::now(),
                node_id: config.node_id.clone(),
                agent_short_name: agent.clone(),
                intercept_method: config.intercept_method,
                direction: TrafficDirection::Send,
                method: Some(method_str.clone()),
                url: url_str.clone(),
                host: host.clone(),
                request_headers: Some(req_headers),
                request_body: if body_bytes.is_empty() {
                    None
                } else {
                    Some(decompress_body(&body_bytes, None))
                },
                response_status: None,
                response_headers: None,
                response_body: None,
            };
            let _ = traffic_tx.try_send(request_entry);

            let response_entry = InterceptedTrafficEntry {
                id: None,
                timestamp: chrono::Utc::now(),
                node_id: config.node_id.clone(),
                agent_short_name: agent,
                intercept_method: config.intercept_method,
                direction: TrafficDirection::Receive,
                method: Some(method_str.clone()),
                url: url_str,
                host: host.clone(),
                request_headers: None,
                request_body: None,
                response_status: Some(status_code),
                response_headers: Some(resp_headers.clone()),
                response_body: if decompressed_body.is_empty() {
                    None
                } else {
                    Some(decompressed_body)
                },
            };
            let _ = traffic_tx.try_send(response_entry);
        }
    }

    //
    // Build response to return to client.
    //
    let mut response = Response::builder()
        .status(StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY));

    for (key, value) in &response_headers {
        if should_strip_proxy_header(key, &response_connection_tokens)
            || key.eq_ignore_ascii_case("content-length")
            || key.eq_ignore_ascii_case("transfer-encoding")
        {
            continue;
        }
        response = response.header(key.as_str(), value.as_str());
    }

    Ok(response
        .body(Full::new(Bytes::from(response_body)))
        .unwrap_or_else(|_| bad_gateway_response("Invalid origin response headers")))
}

#[allow(clippy::too_many_arguments)]
async fn handle_plain_websocket(
    mut req: Request<hyper::body::Incoming>,
    host: String,
    port: u16,
    path: String,
    url: String,
    method: String,
    request_header_lines: Vec<(String, String)>,
    config: Arc<ProxyConfig>,
    traffic_tx: TrafficSink,
    shutdown: CancellationToken,
    tasks: TaskTracker,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    use tokio::io::{AsyncWriteExt, BufReader};

    let has_body = req
        .headers()
        .get_all("content-length")
        .iter()
        .any(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                != Some(0)
        })
        || req.headers().contains_key("transfer-encoding");
    if has_body {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Full::new(Bytes::from(
                "WebSocket upgrade requests with bodies are not supported",
            )))
            .unwrap());
    }

    let on_upgrade = hyper::upgrade::on(&mut req);
    let request_headers: IndexMap<String, String> =
        request_header_lines.iter().cloned().collect();
    let origin = match TcpStream::connect(format!("{}:{}", host, port)).await {
        Ok(stream) => stream,
        Err(error) => {
            common::log_warn!("Failed to connect WebSocket origin {}: {}", host, error);
            return Ok(bad_gateway_response("Failed to connect WebSocket origin"));
        }
    };
    let (origin_read, mut origin_write) = origin.into_split();
    let mut origin_reader = BufReader::new(origin_read);
    if origin_write
        .write_all(format!("{} {} HTTP/1.1\r\n", method, path).as_bytes())
        .await
        .is_err()
    {
        return Ok(bad_gateway_response("Failed to forward WebSocket request"));
    }
    let ws_connection_tokens = connection_header_tokens(&request_header_lines);
    for (key, value) in &request_header_lines {
        if !should_forward_websocket_request_header(key, &ws_connection_tokens) {
            continue;
        }
        if origin_write
            .write_all(format!("{}: {}\r\n", key, value).as_bytes())
            .await
            .is_err()
        {
            return Ok(bad_gateway_response("Failed to forward WebSocket headers"));
        }
    }
    if origin_write.write_all(b"\r\n").await.is_err()
        || origin_write.flush().await.is_err()
    {
        return Ok(bad_gateway_response("Failed to flush WebSocket request"));
    }

    let mut interim_sink = tokio::io::sink();
    let (_, status, response_headers, body_type) = match timeout(
        Duration::from_secs(HTTP_HEADER_TIMEOUT_SECS),
        read_final_response_headers(&mut origin_reader, &mut interim_sink, &method),
    )
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            common::log_warn!("Failed to read WebSocket response: {}", error);
            return Ok(bad_gateway_response("Failed to read WebSocket response"));
        }
        Err(_) => return Ok(bad_gateway_response("Timed out reading WebSocket response")),
    };
    let status = status.unwrap_or(502);
    let upgraded = status == 101
        && header_has_token(&response_headers, "upgrade", "websocket")
        && header_has_token(&response_headers, "connection", "upgrade");
    let should_intercept = {
        let domains = config.intercept_domains.read().await;
        domains
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{}", domain)))
    };
    let configured_agent = config.agent_for_host(&host);
    let capture_config = config.capture_config_for_host(&host).cloned();
    let capture_enabled = should_intercept
        && capture_config
            .as_ref()
            .map(|capture| capture.matches_url(&url))
            .unwrap_or(true);
    let agent = capture_agent_label(capture_config.as_ref(), &configured_agent, &url)
        .unwrap_or(configured_agent);

    if !upgraded {
        let response_body = match read_plain_http_body(&mut origin_reader, body_type).await {
            Ok(body) => body,
            Err(error) => {
                common::log_warn!("Failed to read rejected WebSocket response: {}", error);
                return Ok(bad_gateway_response("Failed to read WebSocket response body"));
            }
        };
        if capture_enabled {
            let response_headers_map: IndexMap<String, String> =
                response_headers.iter().cloned().collect();
            let content_encoding = response_headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("content-encoding"))
                .map(|(_, value)| value.as_str());
            let captured_response = decompress_body(&response_body, content_encoding);
            let _ = traffic_tx.try_send(InterceptedTrafficEntry {
                id: None,
                timestamp: chrono::Utc::now(),
                node_id: config.node_id.clone(),
                agent_short_name: agent.clone(),
                intercept_method: config.intercept_method,
                direction: TrafficDirection::Send,
                method: Some(method.clone()),
                url: url.clone(),
                host: host.clone(),
                request_headers: Some(request_headers.clone()),
                request_body: None,
                response_status: None,
                response_headers: None,
                response_body: None,
            });
            let _ = traffic_tx.try_send(InterceptedTrafficEntry {
                id: None,
                timestamp: chrono::Utc::now(),
                node_id: config.node_id.clone(),
                agent_short_name: agent.clone(),
                intercept_method: config.intercept_method,
                direction: TrafficDirection::Receive,
                method: Some(method.clone()),
                url: url.clone(),
                host: host.clone(),
                request_headers: None,
                request_body: None,
                response_status: Some(status),
                response_headers: Some(response_headers_map),
                response_body: (!captured_response.is_empty()).then_some(captured_response),
            });
        }
        let mut response = Response::builder()
            .status(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY));
        let response_connection_tokens = connection_header_tokens(&response_headers);
        for (key, value) in &response_headers {
            if !should_strip_proxy_header(key, &response_connection_tokens)
                && !key.eq_ignore_ascii_case("content-length")
                && !key.eq_ignore_ascii_case("transfer-encoding")
            {
                response = response.header(key.as_str(), value.as_str());
            }
        }
        return Ok(response
            .body(Full::new(Bytes::from(response_body)))
            .unwrap_or_else(|_| bad_gateway_response("Invalid WebSocket origin response")));
    }

    let response_headers_map: IndexMap<String, String> =
        response_headers.iter().cloned().collect();
    let flow_id = uuid::Uuid::new_v4().simple().to_string();

    if capture_enabled {
        let request_entry = InterceptedTrafficEntry {
            id: None,
            timestamp: chrono::Utc::now(),
            node_id: config.node_id.clone(),
            agent_short_name: agent.clone(),
            intercept_method: config.intercept_method,
            direction: TrafficDirection::Send,
            method: Some(format!("WS_UPGRADE#{}", flow_id)),
            url: url.clone(),
            host: host.clone(),
            request_headers: Some(request_headers.clone()),
            request_body: None,
            response_status: None,
            response_headers: None,
            response_body: None,
        };
        let _ = traffic_tx.try_send(request_entry);
        let response_entry = InterceptedTrafficEntry {
            id: None,
            timestamp: chrono::Utc::now(),
            node_id: config.node_id.clone(),
            agent_short_name: agent.clone(),
            intercept_method: config.intercept_method,
            direction: TrafficDirection::Receive,
            method: Some(format!("WS_UPGRADE#{}", flow_id)),
            url: url.clone(),
            host: host.clone(),
            request_headers: None,
            request_body: None,
            response_status: Some(status),
            response_headers: Some(response_headers_map),
            response_body: None,
        };
        let _ = traffic_tx.try_send(response_entry);
    }

    let task_cancel = shutdown.clone();
    let task_traffic = traffic_tx.clone();
    let task_node_id = config.node_id.clone();
    let intercept_method = config.intercept_method;
    let task_flow_id = flow_id.clone();
    tasks.spawn(async move {
        tokio::select! {
            biased;
            _ = task_cancel.cancelled() => {}
            result = async {
                let upgraded = on_upgrade.await.context("Client WebSocket upgrade failed")?;
                let upgraded = TokioIo::new(upgraded);
                let (client_read, client_write) = tokio::io::split(upgraded);
                handle_websocket_traffic(
                    client_read,
                    client_write,
                    origin_reader,
                    origin_write,
                    &url,
                    &host,
                    &agent,
                    &task_node_id,
                    intercept_method,
                    capture_config.as_ref(),
                    capture_enabled,
                    &task_traffic,
                    &task_flow_id,
                ).await
            } => {
                if let Err(error) = result {
                    common::log_debug!("Plain WebSocket tunnel ended: {}", error);
                }
            }
        }
    });

    let mut response = Response::builder().status(StatusCode::SWITCHING_PROTOCOLS);
    for (key, value) in &response_headers {
        response = response.header(key.as_str(), value.as_str());
    }
    Ok(response
        .body(Full::new(Bytes::new()))
        .unwrap_or_else(|_| bad_gateway_response("Invalid WebSocket upgrade response")))
}

async fn read_plain_http_body<R>(
    reader: &mut tokio::io::BufReader<R>,
    body_type: ResponseBodyType,
) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};

    let mut body = Vec::new();
    match body_type {
        ResponseBodyType::None => {}
        ResponseBodyType::ContentLength(length) => {
            if length > MAX_PLAIN_HTTP_BODY_SIZE {
                anyhow::bail!("Plain HTTP response body exceeds safety limit");
            }
            body.resize(length, 0);
            timeout(
                Duration::from_secs(CHUNK_TIMEOUT_SECS),
                reader.read_exact(&mut body),
            )
            .await
            .context("Timed out reading plain HTTP response body")?
            .context("Failed to read plain HTTP response body")?;
        }
        ResponseBodyType::UntilEof => {
            let mut buffer = vec![0u8; STREAM_BUFFER_SIZE];
            loop {
                let count = timeout(
                    Duration::from_secs(CHUNK_TIMEOUT_SECS),
                    reader.read(&mut buffer),
                )
                .await
                .context("Timed out reading plain HTTP response body")?
                .context("Failed to read plain HTTP response body")?;
                if count == 0 {
                    break;
                }
                if body.len().saturating_add(count) > MAX_PLAIN_HTTP_BODY_SIZE {
                    anyhow::bail!("Plain HTTP response body exceeds safety limit");
                }
                body.extend_from_slice(&buffer[..count]);
            }
        }
        ResponseBodyType::Chunked => loop {
            let mut size_line = String::new();
            let bytes_read = timeout(
                Duration::from_secs(CHUNK_TIMEOUT_SECS),
                reader.read_line(&mut size_line),
            )
            .await
            .context("Timed out reading plain HTTP chunk size")?
            .context("Failed to read plain HTTP chunk size")?;
            if bytes_read == 0
                || size_line.len() > MAX_CHUNK_METADATA_LINE_SIZE
                || !size_line.ends_with("\r\n")
            {
                anyhow::bail!("Invalid plain HTTP chunk size line");
            }
            let size_token = size_line
                .trim_end_matches(['\r', '\n'])
                .split(';')
                .next()
                .unwrap_or_default()
                .trim();
            let chunk_size = usize::from_str_radix(size_token, 16)
                .with_context(|| format!("Invalid HTTP chunk size: {}", size_token))?;
            if chunk_size == 0 {
                loop {
                    let mut trailer = String::new();
                    let bytes_read = timeout(
                        Duration::from_secs(CHUNK_TIMEOUT_SECS),
                        reader.read_line(&mut trailer),
                    )
                    .await
                    .context("Timed out reading plain HTTP chunk trailer")?
                    .context("Failed to read plain HTTP chunk trailer")?;
                    if bytes_read == 0
                        || trailer.len() > MAX_CHUNK_METADATA_LINE_SIZE
                        || !trailer.ends_with("\r\n")
                    {
                        anyhow::bail!("Invalid plain HTTP chunk trailer");
                    }
                    if trailer == "\r\n" {
                        break;
                    }
                }
                break;
            }
            if body.len().saturating_add(chunk_size) > MAX_PLAIN_HTTP_BODY_SIZE {
                anyhow::bail!("Plain HTTP response body exceeds safety limit");
            }
            let start = body.len();
            body.resize(start + chunk_size, 0);
            timeout(
                Duration::from_secs(CHUNK_TIMEOUT_SECS),
                reader.read_exact(&mut body[start..]),
            )
            .await
            .context("Timed out reading plain HTTP chunk data")?
            .context("Failed to read plain HTTP chunk data")?;
            let mut crlf = [0u8; 2];
            timeout(
                Duration::from_secs(CHUNK_TIMEOUT_SECS),
                reader.read_exact(&mut crlf),
            )
            .await
            .context("Timed out reading plain HTTP chunk terminator")?
            .context("Failed to read plain HTTP chunk terminator")?;
            if crlf != *b"\r\n" {
                anyhow::bail!("Invalid plain HTTP chunk terminator");
            }
        },
    }
    Ok(body)
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

    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("Failed to create TLS config")?;
    //
    // Prefer HTTP/1.1 so origins without HTTP/2 are not forced into a
    // protocol the client selected only because of the MITM. HTTP/2 remains
    // available to clients that require it, and origin negotiation is
    // verified before any frames are proxied.
    //
    config.alpn_protocols = vec![b"http/1.1".to_vec(), b"h2".to_vec()];

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
    /// Body delimited by closing the origin connection.
    UntilEof,
}

/// Read only the response headers (status line + headers), don't read body
/// Returns (response_line, status_code, headers, body_type)
async fn read_response_headers<R>(
    reader: &mut tokio::io::BufReader<R>,
    request_method: &str,
) -> Result<(
    String,
    Option<u16>,
    Vec<(String, String)>,
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
    if response_line.len() > MAX_HTTP_HEADER_LINE_SIZE {
        anyhow::bail!("HTTP response line exceeds safety limit");
    }

    //
    // Parse and validate status line: HTTP/1.x <3-digit-status> [reason].
    //
    let status_code = parse_http_status_line(&response_line)
        .context("Invalid HTTP response status line")?;

    //
    // Read headers - preserve original order and case.
    //
    let mut response_headers = Vec::new();
    let mut content_length: Option<usize> = None;
    let mut transfer_encodings = Vec::new();
    let mut header_bytes = response_line.len();

    loop {
        let mut header_line = String::new();
        let bytes_read = reader
            .read_line(&mut header_line)
            .await
            .context("Failed to read response header")?;
        if bytes_read == 0 {
            anyhow::bail!("Connection closed in response headers");
        }
        header_bytes = header_bytes.saturating_add(bytes_read);
        if header_line.len() > MAX_HTTP_HEADER_LINE_SIZE
            || header_bytes > MAX_HTTP_HEADER_SECTION_SIZE
        {
            anyhow::bail!("HTTP response headers exceed safety limit");
        }
        let line = header_line.trim();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let original_key = key.trim().to_string();
            let value = value.trim().to_string();
            if original_key.eq_ignore_ascii_case("content-length") {
                let parsed = value.parse::<usize>().context("Invalid Content-Length")?;
                if content_length.is_some_and(|length| length != parsed) {
                    anyhow::bail!("Conflicting Content-Length response headers");
                }
                content_length = Some(parsed);
            }
            if original_key.eq_ignore_ascii_case("transfer-encoding") {
                transfer_encodings.extend(
                    value
                        .split(',')
                        .map(|encoding| encoding.trim().to_ascii_lowercase())
                        .filter(|encoding| !encoding.is_empty()),
                );
            }
            response_headers.push((original_key, value));
        } else {
            anyhow::bail!("Malformed HTTP response header");
        }
    }

    let is_chunked = transfer_encodings
        .last()
        .is_some_and(|encoding| encoding == "chunked");
    if transfer_encodings
        .iter()
        .filter(|encoding| encoding.as_str() == "chunked")
        .count()
        > 1
        || (transfer_encodings.iter().any(|encoding| encoding == "chunked") && !is_chunked)
    {
        anyhow::bail!("Invalid chunked Transfer-Encoding response");
    }
    if !transfer_encodings.is_empty() && content_length.is_some() {
        anyhow::bail!("Ambiguous response with both chunked encoding and Content-Length");
    }

    //
    // Determine body type
    // 1xx, 204 No Content, 304 Not Modified have no body.
    //
    let body_type = match status_code {
        _ if request_method.eq_ignore_ascii_case("HEAD") => ResponseBodyType::None,
        code if code < 200 || code == 204 || code == 205 || code == 304 => {
            ResponseBodyType::None
        }
        _ if is_chunked => ResponseBodyType::Chunked,
        _ if !transfer_encodings.is_empty() => ResponseBodyType::UntilEof,
        _ => match content_length {
            Some(0) => ResponseBodyType::None,
            Some(len) => ResponseBodyType::ContentLength(len),
            //
            // Without framing, an HTTP/1 response body ends at EOF.
            //
            None => ResponseBodyType::UntilEof,
        },
    };

    Ok((response_line, Some(status_code), response_headers, body_type))
}

async fn read_final_response_headers<R, W>(
    reader: &mut tokio::io::BufReader<R>,
    writer: &mut W,
    request_method: &str,
) -> Result<(
    String,
    Option<u16>,
    Vec<(String, String)>,
    ResponseBodyType,
)>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    loop {
        let (response_line, status, headers, body_type) =
            read_response_headers(reader, request_method).await?;
        if !matches!(status, Some(100..=199)) || status == Some(101) {
            return Ok((response_line, status, headers, body_type));
        }

        writer.write_all(response_line.as_bytes()).await?;
        for (key, value) in &headers {
            writer
                .write_all(format!("{}: {}\r\n", key, value).as_bytes())
                .await?;
        }
        writer.write_all(b"\r\n").await?;
        writer.flush().await?;
    }
}

async fn relay_expect_continue<R, W>(
    reader: &mut tokio::io::BufReader<R>,
    writer: &mut W,
    request_method: &str,
) -> Result<Option<HttpResponseHead>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    loop {
        let response = read_response_headers(reader, request_method).await?;
        let status = response.1;
        if !matches!(status, Some(100..=199)) || status == Some(101) {
            return Ok(Some(response));
        }

        writer.write_all(response.0.as_bytes()).await?;
        for (key, value) in &response.2 {
            writer
                .write_all(format!("{}: {}\r\n", key, value).as_bytes())
                .await?;
        }
        writer.write_all(b"\r\n").await?;
        writer.flush().await?;
        if status == Some(100) {
            return Ok(None);
        }
    }
}

const STREAM_BUFFER_SIZE: usize = 64 * 1024;
const MAX_CHUNK_METADATA_LINE_SIZE: usize = 8 * 1024;
const CHUNK_TIMEOUT_SECS: u64 = 60;

fn append_capture(buffer: &mut Vec<u8>, bytes: &[u8]) {
    let remaining = MAX_CAPTURE_BODY_SIZE.saturating_sub(buffer.len());
    buffer.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
}

async fn stream_fixed_body<R, W>(
    reader: &mut tokio::io::BufReader<R>,
    writer: &mut W,
    length: usize,
) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut capture = Vec::with_capacity(length.min(MAX_CAPTURE_BODY_SIZE));
    let mut buffer = vec![0u8; STREAM_BUFFER_SIZE];
    let mut remaining = length;
    while remaining > 0 {
        let count = remaining.min(buffer.len());
        timeout(
            Duration::from_secs(CHUNK_TIMEOUT_SECS),
            reader.read_exact(&mut buffer[..count]),
        )
        .await
        .context("Timed out reading fixed-length HTTP body")?
        .context("Failed to read fixed-length HTTP body")?;
        writer.write_all(&buffer[..count]).await?;
        writer.flush().await?;
        append_capture(&mut capture, &buffer[..count]);
        remaining -= count;
    }
    Ok(capture)
}

async fn stream_until_eof<R, W>(
    reader: &mut tokio::io::BufReader<R>,
    writer: &mut W,
) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut capture = Vec::new();
    let mut buffer = vec![0u8; STREAM_BUFFER_SIZE];
    loop {
        let count = timeout(
            Duration::from_secs(CHUNK_TIMEOUT_SECS),
            reader.read(&mut buffer),
        )
        .await
        .context("Timed out reading close-delimited HTTP body")?
        .context("Failed to read close-delimited HTTP body")?;
        if count == 0 {
            break;
        }
        writer.write_all(&buffer[..count]).await?;
        writer.flush().await?;
        append_capture(&mut capture, &buffer[..count]);
    }
    Ok(capture)
}

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
    let mut transport_buffer = vec![0u8; STREAM_BUFFER_SIZE];

    loop {
        let mut size_line = String::new();
        let bytes_read = timeout(
            Duration::from_secs(CHUNK_TIMEOUT_SECS),
            reader.read_line(&mut size_line),
        )
        .await
        .context("Timed out reading HTTP chunk size")?
        .context("Failed to read HTTP chunk size")?;

        if bytes_read == 0 {
            anyhow::bail!("Connection closed before terminating HTTP chunk");
        }
        if size_line.len() > MAX_CHUNK_METADATA_LINE_SIZE || !size_line.ends_with("\r\n") {
            anyhow::bail!("Invalid HTTP chunk size line");
        }

        writer.write_all(size_line.as_bytes()).await?;

        let size_token = size_line
            .trim_end_matches(['\r', '\n'])
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        let chunk_size = usize::from_str_radix(size_token, 16)
            .with_context(|| format!("Invalid HTTP chunk size: {}", size_token))?;

        if chunk_size == 0 {
            loop {
                let mut trailer = String::new();
                let bytes_read = timeout(
                    Duration::from_secs(CHUNK_TIMEOUT_SECS),
                    reader.read_line(&mut trailer),
                )
                .await
                .context("Timed out reading HTTP chunk trailer")?
                .context("Failed to read HTTP chunk trailer")?;
                if bytes_read == 0
                    || trailer.len() > MAX_CHUNK_METADATA_LINE_SIZE
                    || !trailer.ends_with("\r\n")
                {
                    anyhow::bail!("Invalid HTTP chunk trailer");
                }
                writer.write_all(trailer.as_bytes()).await?;
                if trailer == "\r\n" {
                    break;
                }
            }
            writer.flush().await?;
            break;
        }

        let mut remaining = chunk_size;
        while remaining > 0 {
            let count = remaining.min(transport_buffer.len());
            timeout(
                Duration::from_secs(CHUNK_TIMEOUT_SECS),
                reader.read_exact(&mut transport_buffer[..count]),
            )
            .await
            .context("Timed out reading HTTP chunk data")?
            .context("Failed to read HTTP chunk data")?;
            writer.write_all(&transport_buffer[..count]).await?;
            writer.flush().await?;
            append_capture(&mut body_buffer, &transport_buffer[..count]);
            remaining -= count;
        }

        let mut crlf = [0u8; 2];
        timeout(
            Duration::from_secs(CHUNK_TIMEOUT_SECS),
            reader.read_exact(&mut crlf),
        )
        .await
        .context("Timed out reading HTTP chunk terminator")?
        .context("Failed to read HTTP chunk terminator")?;
        if crlf != *b"\r\n" {
            anyhow::bail!("Invalid HTTP chunk terminator");
        }
        writer.write_all(&crlf).await?;
        writer.flush().await?;
    }

    Ok(body_buffer)
}

/// Discover the default network interface by parsing `ip route show default`.
#[cfg(target_os = "linux")]
fn discover_default_interface() -> Option<String> {
    use crate::utils::CommandOutputBounded;
    use std::process::Command;

    let output = Command::new("ip")
        .args(["route", "show", "default"])
        .output_bounded()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_frame(payload: Vec<u8>, flags: u8, stream_id: u32) -> H2Frame {
        H2Frame {
            frame_type: H2_FRAME_HEADERS,
            flags,
            stream_id,
            payload,
        }
    }

    fn encode(pairs: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut encoder = hpack::Encoder::new();
        encoder.encode(pairs.iter().map(|(k, v)| (*k, *v)))
    }

    #[test]
    fn header_block_fragment_strips_padding_and_priority() {
        //
        // PADDED + PRIORITY: [pad_len=2][5 priority bytes][block][2 pad bytes].
        //
        let block = vec![0xAA, 0xBB, 0xCC];
        let mut payload = vec![2u8];
        payload.extend_from_slice(&[0, 0, 0, 0, 0]); // priority
        payload.extend_from_slice(&block);
        payload.extend_from_slice(&[0xFF, 0xFF]); // padding

        let frame = headers_frame(payload, H2_FLAG_PADDED | H2_FLAG_PRIORITY, 1);
        assert_eq!(header_block_fragment(&frame), Some(block.as_slice()));
    }

    #[test]
    fn header_block_fragment_plain() {
        let block = vec![0x82, 0x86]; // two indexed static headers
        let frame = headers_frame(block.clone(), 0, 1);
        assert_eq!(header_block_fragment(&frame), Some(block.as_slice()));
    }

    #[test]
    fn decodes_complete_headers_block() {
        let payload = encode(&[
            (b":method", b"POST"),
            (b":path", b"/svc.V1/Do"),
            (b"content-type", b"application/grpc"),
        ]);
        let frame = headers_frame(payload, H2_FLAG_END_HEADERS, 1);

        let mut dec = H2HeaderDecoder::new();
        let decoded = dec.feed(&frame).expect("headers should decode");
        assert!(!decoded.is_push_promise);

        let (headers, path, method, _status) = split_h2_headers(&decoded.headers);
        assert_eq!(path.as_deref(), Some("/svc.V1/Do"));
        assert_eq!(method.as_deref(), Some("POST"));
        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/grpc")
        );
    }

    #[test]
    fn reassembles_across_continuation() {
        let payload = encode(&[(b":path", b"/a/b/c"), (b"x-token", b"abcdef")]);
        let (first, second) = payload.split_at(payload.len() / 2);

        let mut dec = H2HeaderDecoder::new();
        // HEADERS without END_HEADERS -> still pending.
        assert!(
            dec.feed(&headers_frame(first.to_vec(), 0, 3)).is_none(),
            "incomplete block must not decode"
        );
        // CONTINUATION with END_HEADERS -> completes the block.
        let cont = H2Frame {
            frame_type: H2_FRAME_CONTINUATION,
            flags: H2_FLAG_END_HEADERS,
            stream_id: 3,
            payload: second.to_vec(),
        };
        let decoded = dec.feed(&cont).expect("reassembled block should decode");
        let (_, path, _, _) = split_h2_headers(&decoded.headers);
        assert_eq!(path.as_deref(), Some("/a/b/c"));
    }

    #[test]
    fn dynamic_table_persists_across_blocks() {
        //
        // Encode two blocks with one encoder so the second may reference the
        // dynamic table populated by the first. A single decoder must track
        // that state to decode the second correctly.
        //
        let mut encoder = hpack::Encoder::new();
        let b1 = encoder.encode([(&b"x-custom"[..], &b"value-1"[..])]);
        let b2 = encoder.encode([(&b"x-custom"[..], &b"value-1"[..])]);

        let mut dec = H2HeaderDecoder::new();
        let d1 = dec
            .feed(&headers_frame(b1, H2_FLAG_END_HEADERS, 1))
            .expect("first block decodes");
        let d2 = dec
            .feed(&headers_frame(b2, H2_FLAG_END_HEADERS, 3))
            .expect("second block decodes via dynamic table");

        assert_eq!(d1.headers, d2.headers);
        assert_eq!(d2.headers[0].0, "x-custom");
        assert_eq!(d2.headers[0].1, "value-1");
    }

    #[test]
    fn poisons_after_decode_error_and_stays_disabled() {
        //
        // 0xBE = indexed header field, index 62 — out of range with an empty
        // dynamic table, so the decode fails and the direction is poisoned.
        //
        let mut dec = H2HeaderDecoder::new();
        assert!(
            dec.feed(&headers_frame(vec![0xBE], H2_FLAG_END_HEADERS, 1))
                .is_none()
        );
        assert!(dec.poisoned, "decode error must poison the direction");

        // A subsequently valid block must not be decoded once poisoned.
        let valid = encode(&[(b":path", b"/ok")]);
        assert!(
            dec.feed(&headers_frame(valid, H2_FLAG_END_HEADERS, 3))
                .is_none(),
            "poisoned decoder must not emit headers"
        );
    }

    #[test]
    fn poisons_interleaved_continuation_sequence() {
        let payload = encode(&[(b":path", b"/continued")]);
        let mut dec = H2HeaderDecoder::new();
        assert!(dec.feed(&headers_frame(payload, 0, 1)).is_none());
        assert!(
            dec.feed(&headers_frame(vec![0x82], H2_FLAG_END_HEADERS, 3))
                .is_none()
        );
        assert!(dec.poisoned);
    }

    #[test]
    fn caps_oversized_header_block() {
        let mut dec = H2HeaderDecoder::new();
        let frame = headers_frame(
            vec![0; H2HeaderDecoder::MAX_HEADER_BLOCK_BYTES + 1],
            H2_FLAG_END_HEADERS,
            1,
        );
        assert!(dec.feed(&frame).is_none());
        assert!(dec.poisoned);
        assert!(dec.pending.is_empty());
    }

    #[test]
    fn extracts_padded_data_payload() {
        let frame = H2Frame {
            frame_type: H2_FRAME_DATA,
            flags: H2_FLAG_PADDED,
            stream_id: 1,
            payload: vec![2, b'a', b'b', 0, 0],
        };
        assert_eq!(data_frame_payload(&frame), Some(b"ab".as_slice()));
    }

    #[test]
    fn reads_header_table_size_setting() {
        let frame = H2Frame {
            frame_type: H2_FRAME_SETTINGS,
            flags: 0,
            stream_id: 0,
            payload: vec![0, 1, 0, 0, 16, 0],
        };
        assert_eq!(settings_header_table_size(&frame), Some(4096));
    }

    #[test]
    fn rejects_h2_when_origin_does_not_negotiate_it() {
        assert!(ensure_compatible_alpn(Some(b"h2"), None).is_err());
        assert!(ensure_compatible_alpn(Some(b"h2"), Some(b"http/1.1")).is_err());
        assert!(ensure_compatible_alpn(Some(b"h2"), Some(b"h2")).is_ok());
        assert!(ensure_compatible_alpn(Some(b"http/1.1"), None).is_ok());
    }
}
