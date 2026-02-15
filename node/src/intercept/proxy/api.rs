use anyhow::{Context, Result};
use bytes::Bytes;
use common::{InterceptedTrafficEntry, InterceptMethod, TrafficDirection};
use flate2::read::{GzDecoder, DeflateDecoder};
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::collections::{HashMap, HashSet};
use indexmap::IndexMap;
use std::io::{Cursor, Read};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};
use tokio_rustls::TlsAcceptor;

use super::certificate::CertificateAuthority;

/// Observed connection for agent discovery
#[derive(Debug, Clone)]
pub struct ObservedConnection {
    /// Target IP address
    pub ip: std::net::IpAddr,
    /// Target port
    pub port: u16,
    /// Domain name (from SNI or Host header)
    pub domain: Option<String>,
    /// Whether it's HTTPS
    pub is_https: bool,
    /// API key extracted from Authorization or x-api-key header
    pub api_key: Option<String>,
}

/// Extract API key from HTTP headers.
///
/// Checks x-api-key header first, then Authorization header for Bearer token.
fn extract_api_key_from_headers(headers: &IndexMap<String, String>, host: &str) -> Option<String> {
    //
    // Dump all header names for debugging.
    //
    let header_names: Vec<&str> = headers.keys().map(|k| k.as_str()).collect();
    common::log_debug!(
        "Checking headers for API key on {}: {:?}",
        host, header_names
    );

    //
    // Check x-api-key header first (most specific).
    //
    for (key, value) in headers {
        if key.to_lowercase() == "x-api-key" {
            common::log_debug!(
                "Found x-api-key header for {} (key length: {})",
                host,
                value.len()
            );
            return Some(value.to_string());
        }
    }

    //
    // Check Authorization header for Bearer token.
    //
    for (key, value) in headers {
        if key.to_lowercase() == "authorization" {
            if let Some(token) = value.strip_prefix("Bearer ") {
                common::log_debug!(
                    "Found Authorization Bearer token for {} (token length: {})",
                    host,
                    token.len()
                );
                return Some(token.to_string());
            } else {
                common::log_debug!(
                    "Found Authorization header for {} but not Bearer format: {}",
                    host,
                    &value[..value.len().min(20)]
                );
            }
        }
    }

    common::log_debug!("No API key found in headers for {}", host);
    None
}

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
    /// Optional channel for observed connections (for agent discovery)
    pub connection_observer_tx: Option<mpsc::UnboundedSender<ObservedConnection>>,
    /// Pre-resolved IPs for domains (used in Hosts mode to bypass hosts file redirection)
    pub domain_to_real_ip: HashMap<String, std::net::IpAddr>,
}

/// The intercept proxy server
pub struct InterceptProxy {
    /// Primary port the proxy is listening on (443 for Hosts, random for others)
    port: u16,
    /// Shutdown signal sender
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
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
        traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
    ) -> Result<Self> {
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
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

                    //
                    // Spawn a separate task for the HTTP listener.
                    //
                    let http_task = tokio::spawn(run_proxy_http(
                        http_listener,
                        ca_clone,
                        config_clone,
                        traffic_tx_clone,
                    ));
                    extra_task_handles.push(http_task);
                }
                Err(e) => {
                    common::log_warn!("Could not bind to port 80 (HTTP): {}. Only HTTPS interception will work.", e);
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

        let task_handle = tokio::spawn(run_proxy(listener, ca, config, traffic_tx, shutdown_rx));

        Ok(Self {
            port,
            shutdown_tx: Some(shutdown_tx),
            task_handle: Some(task_handle),
            extra_task_handles,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
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
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        for handle in &self.extra_task_handles {
            handle.abort();
        }
    }
}

