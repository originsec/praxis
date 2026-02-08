//
// Agent Discovery Manager - Probes network connections for OpenAI-compatible
// LLM endpoints.
//

use chrono::Utc;
use common::ai::probe_openai_compatible_endpoint;
use common::DiscoveredLlmEndpoint;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use super::CertificateAuthority;

/// Result of probing an IP address for an LLM endpoint
#[derive(Debug, Clone)]
enum ProbeResult {
    /// Found an LLM endpoint with models
    LlmEndpoint {
        domain: Option<String>,
        models: Vec<String>,
        base_url: String,
    },
    /// LLM endpoint that requires authentication (got 401/403)
    RequiresAuth {
        domain: Option<String>,
        base_url: String,
    },
    /// Not an LLM endpoint
    NotLlmEndpoint,
}

/// Agent Discovery Manager
///
/// Probes network connections for OpenAI-compatible LLM endpoints. When the
/// proxy intercepts a connection to port 80/443, this manager checks if the
/// destination is an LLM endpoint by trying to call /v1/models.
pub struct AgentDiscoveryManager {
    /// Whether discovery is enabled
    enabled: bool,
    /// Set of endpoints we've already probed (to avoid re-probing)
    probed_endpoints: Arc<RwLock<HashSet<String>>>,
    /// Set of IPs currently being probed (to avoid duplicate concurrent probes)
    probing_in_progress: Arc<RwLock<HashSet<String>>>,
    /// Discovered endpoints ready to report
    discovered_endpoints: Arc<RwLock<Vec<DiscoveredLlmEndpoint>>>,
    /// API keys observed for domains (domain -> api_key)
    observed_api_keys: Arc<RwLock<HashMap<String, String>>>,
    /// Domains pending interception (waiting for API key)
    pending_intercept_domains: Arc<RwLock<HashSet<String>>>,
    /// Channel for sending discoveries to service
    discovery_tx: mpsc::UnboundedSender<DiscoveredLlmEndpoint>,
    /// Channel for requesting domain interception
    intercept_domain_tx: Option<mpsc::UnboundedSender<String>>,
    /// Reference to CA for generating probe certificates (for HTTPS)
    ca: Option<Arc<RwLock<CertificateAuthority>>>,
    /// Node ID for endpoint records
    node_id: String,
}

impl AgentDiscoveryManager {
    /// Create a new agent discovery manager
    pub fn new(
        node_id: String,
        discovery_tx: mpsc::UnboundedSender<DiscoveredLlmEndpoint>,
    ) -> Self {
        Self {
            enabled: false,
            probed_endpoints: Arc::new(RwLock::new(HashSet::new())),
            probing_in_progress: Arc::new(RwLock::new(HashSet::new())),
            discovered_endpoints: Arc::new(RwLock::new(Vec::new())),
            observed_api_keys: Arc::new(RwLock::new(HashMap::new())),
            pending_intercept_domains: Arc::new(RwLock::new(HashSet::new())),
            discovery_tx,
            intercept_domain_tx: None,
            ca: None,
            node_id,
        }
    }

    /// Enable agent discovery with CA reference for HTTPS probing
    pub fn enable(
        &mut self,
        ca: Arc<RwLock<CertificateAuthority>>,
        intercept_domain_tx: mpsc::UnboundedSender<String>,
    ) {
        common::log_info!("Agent discovery enabled");
        self.enabled = true;
        self.ca = Some(ca);
        self.intercept_domain_tx = Some(intercept_domain_tx);
    }

    /// Disable agent discovery
    pub fn disable(&mut self) {
        common::log_info!("Agent discovery disabled");
        self.enabled = false;
        self.ca = None;
        self.intercept_domain_tx = None;
    }

    /// Check if agent discovery is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the count of discovered endpoints
    pub async fn discovered_count(&self) -> usize {
        self.discovered_endpoints.read().await.len()
    }

    /// Get a discovered endpoint by ID
    #[allow(dead_code)]
    pub async fn get_endpoint_by_id(&self, endpoint_id: &str) -> Option<DiscoveredLlmEndpoint> {
        let endpoints = self.discovered_endpoints.read().await;
        endpoints.iter().find(|e| e.id == endpoint_id).cloned()
    }

    /// Record an API key observed in traffic for a domain
    pub async fn record_api_key(&self, domain: &str, api_key: String) {
        let mut keys = self.observed_api_keys.write().await;
        if !keys.contains_key(domain) {
            common::log_info!(
                "Recorded API key for domain {} (key length: {})",
                domain,
                api_key.len()
            );
            keys.insert(domain.to_string(), api_key.clone());

            //
            // Update any discovered endpoints for this domain.
            //
            let mut endpoints = self.discovered_endpoints.write().await;
            for endpoint in endpoints.iter_mut() {
                if endpoint.domain.as_deref() == Some(domain) && endpoint.api_key.is_none() {
                    endpoint.api_key = Some(api_key.clone());
                    // Send updated endpoint to service
                    let _ = self.discovery_tx.send(endpoint.clone());
                }
            }
        }
    }

    /// Get the API key for a domain if we've observed one
    #[allow(dead_code)]
    pub async fn get_api_key(&self, domain: &str) -> Option<String> {
        let keys = self.observed_api_keys.read().await;
        keys.get(domain).cloned()
    }

    /// Probe an IP for an OpenAI-compatible endpoint
    ///
    /// This is called from the proxy when a new connection is detected.
    /// It spawns a background task to probe the endpoint.
    pub async fn probe_endpoint(
        &self,
        ip: IpAddr,
        port: u16,
        domain: Option<String>,
        is_https: bool,
    ) {
        if !self.enabled {
            return;
        }

        //
        // Create a key for this endpoint. Use domain if available, otherwise IP.
        // Track probing with/without API key separately.
        //
        let base_key = match &domain {
            Some(d) => format!("{}:{}", d, port),
            None => format!("{}:{}", ip, port),
        };

        //
        // Look up any API key we have for this domain.
        //
        let api_key = if let Some(ref d) = domain {
            let keys = self.observed_api_keys.read().await;
            let key = keys.get(d).cloned();
            if key.is_some() {
                common::log_debug!("Found stored API key for domain {}", d);
            } else {
                common::log_debug!("No stored API key for domain {}", d);
            }
            key
        } else {
            common::log_debug!("No domain provided, cannot look up API key");
            None
        };

        //
        // Create key that includes whether we have an API key.
        //
        let key = if api_key.is_some() {
            format!("{}:withkey", base_key)
        } else {
            format!("{}:nokey", base_key)
        };

        //
        // Check if we've already probed this endpoint (with or without key).
        //
        {
            let probed = self.probed_endpoints.read().await;
            if probed.contains(&key) {
                return;
            }
        }

        //
        // Check if a probe is already in progress.
        //
        {
            let mut in_progress = self.probing_in_progress.write().await;
            if !in_progress.insert(key.clone()) {
                return;
            }
        }

        let probed_endpoints = Arc::clone(&self.probed_endpoints);
        let probing_in_progress = Arc::clone(&self.probing_in_progress);
        let discovered_endpoints = Arc::clone(&self.discovered_endpoints);
        let observed_api_keys = Arc::clone(&self.observed_api_keys);
        let pending_intercept_domains = Arc::clone(&self.pending_intercept_domains);
        let discovery_tx = self.discovery_tx.clone();
        let intercept_domain_tx = self.intercept_domain_tx.clone();
        let node_id = self.node_id.clone();
        let ca = self.ca.clone();

        //
        // Spawn background probe task.
        //
        tokio::spawn(async move {
            let result = do_probe(ip, port, domain.clone(), is_https, ca, api_key.as_deref()).await;

            //
            // Mark as probed (so we don't probe again).
            //
            {
                let mut probed = probed_endpoints.write().await;
                probed.insert(key.clone());
            }

            //
            // Remove from in-progress.
            //
            {
                let mut in_progress = probing_in_progress.write().await;
                in_progress.remove(&key);
            }

            //
            // Handle RequiresAuth - request interception of this domain.
            //
            if let ProbeResult::RequiresAuth {
                domain: ref auth_domain,
                base_url: ref auth_base_url,
            } = result
            {
                if let Some(d) = auth_domain {
                    common::log_info!(
                        "Requesting interception of domain {} to capture API key",
                        d
                    );

                    //
                    // Mark domain as pending interception.
                    //
                    {
                        let mut pending = pending_intercept_domains.write().await;
                        pending.insert(d.clone());
                    }

                    //
                    // Request proxy to intercept this domain.
                    //
                    if let Some(ref tx) = intercept_domain_tx {
                        let _ = tx.send(d.clone());
                    }

                    //
                    // Create a preliminary endpoint record (without models).
                    //
                    let endpoint = DiscoveredLlmEndpoint {
                        id: Uuid::new_v4().to_string(),
                        ip_address: ip.to_string(),
                        domain: Some(d.clone()),
                        port,
                        is_https,
                        models: vec![],  // Will be populated after we get API key
                        base_url: auth_base_url.clone(),
                        api_key: None,   // Will be populated after interception
                        discovered_at: Utc::now(),
                        node_id: node_id.clone(),
                    };

                    //
                    // Store locally.
                    //
                    {
                        let mut endpoints = discovered_endpoints.write().await;
                        endpoints.push(endpoint.clone());
                    }

                    //
                    // Send to service (partial discovery).
                    //
                    let _ = discovery_tx.send(endpoint);
                }
            }

            //
            // If we found an LLM endpoint with models, record it and notify service.
            //
            if let ProbeResult::LlmEndpoint {
                domain: found_domain,
                models,
                base_url,
            } = result
            {
                //
                // Use the API key we already have, or look up from observed keys.
                //
                let endpoint_api_key = if api_key.is_some() {
                    api_key.clone()
                } else if let Some(ref d) = found_domain.as_ref().or(domain.as_ref()) {
                    let keys = observed_api_keys.read().await;
                    keys.get(*d).cloned()
                } else {
                    None
                };

                let endpoint = DiscoveredLlmEndpoint {
                    id: Uuid::new_v4().to_string(),
                    ip_address: ip.to_string(),
                    domain: found_domain.or(domain),
                    port,
                    is_https,
                    models,
                    base_url,
                    api_key: endpoint_api_key,
                    discovered_at: Utc::now(),
                    node_id,
                };

                common::log_info!(
                    "Discovered LLM endpoint at {}:{} with {} models",
                    ip,
                    port,
                    endpoint.models.len()
                );

                //
                // Store locally.
                //
                {
                    let mut endpoints = discovered_endpoints.write().await;
                    endpoints.push(endpoint.clone());
                }

                //
                // Send to service.
                //
                let _ = discovery_tx.send(endpoint);
            }
        });
    }
}

/// Perform the actual probe to check if an endpoint is OpenAI-compatible
async fn do_probe(
    ip: IpAddr,
    port: u16,
    domain: Option<String>,
    is_https: bool,
    _ca: Option<Arc<RwLock<CertificateAuthority>>>,
    api_key: Option<&str>,
) -> ProbeResult {
    //
    // Build the base URL. For HTTPS, we need to use the domain if available,
    // otherwise the IP (which may fail TLS verification).
    //
    let ip_str = ip.to_string();
    let host = domain.as_ref().map(|d| d.as_str()).unwrap_or(&ip_str);
    let scheme = if is_https { "https" } else { "http" };

    //
    // Handle standard ports.
    //
    let base_url = if (is_https && port == 443) || (!is_https && port == 80) {
        format!("{}://{}/v1", scheme, host)
    } else {
        format!("{}://{}:{}/v1", scheme, host, port)
    };

    common::log_debug!(
        "Probing {} for LLM endpoint (with_key={})",
        base_url,
        api_key.is_some()
    );

    //
    // Use the common probe function to check for OpenAI-compatible models.
    //
    match probe_openai_compatible_endpoint(&base_url, api_key, is_https).await {
        Ok(models) => {
            if !models.is_empty() {
                common::log_info!(
                    "Found LLM endpoint at {} with {} models: {:?}",
                    base_url,
                    models.len(),
                    models
                );
                ProbeResult::LlmEndpoint {
                    domain,
                    models,
                    base_url,
                }
            } else {
                common::log_debug!("Endpoint {} returned no models", base_url);
                ProbeResult::NotLlmEndpoint
            }
        }
        Err(e) => {
            //
            // Check if this is an auth error - that means it IS an LLM endpoint,
            // just needs authentication.
            //
            let err_lower = e.to_lowercase();
            if err_lower.contains("401") || err_lower.contains("403")
                || err_lower.contains("unauthorized") || err_lower.contains("not authenticated")
                || err_lower.contains("authentication") || err_lower.contains("forbidden")
            {
                common::log_info!(
                    "Found LLM endpoint at {} that requires authentication: {}",
                    base_url, e
                );
                ProbeResult::RequiresAuth { domain, base_url }
            } else {
                common::log_debug!("Failed to probe {}: {}", base_url, e);
                ProbeResult::NotLlmEndpoint
            }
        }
    }
}


