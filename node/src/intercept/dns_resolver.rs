use anyhow::{Context, Result};
use dashmap::DashMap;
use hickory_resolver::TokioResolver;
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

/// DNS resolver that tracks domain-to-IP mappings for interception
pub struct DomainResolver {
    /// Async DNS resolver
    resolver: TokioResolver,
    /// Mapping of domain to resolved IPs
    domain_to_ips: DashMap<String, HashSet<IpAddr>>,
}

impl DomainResolver {
    /// Create a new domain resolver using system DNS configuration
    pub async fn new() -> Result<Self> {
        let resolver = TokioResolver::builder(TokioRuntimeProvider::default())
            .context("Failed to read system DNS configuration")?
            .build()
            .context("Failed to build DNS resolver")?;

        Ok(Self {
            resolver,
            domain_to_ips: DashMap::new(),
        })
    }

    /// Resolve a domain and cache the IPs
    ///
    /// Returns the set of resolved IPs for the domain
    pub async fn resolve_domain(&self, domain: &str) -> Result<HashSet<IpAddr>> {
        //
        // Strip any wildcard prefix for DNS lookup.
        //
        let lookup_domain = domain.trim_start_matches("*.");

        common::log_debug!(
            "Resolving DNS for domain: {} (lookup: {})",
            domain,
            lookup_domain
        );

        let response = self
            .resolver
            .lookup_ip(lookup_domain)
            .await
            .context(format!("Failed to resolve DNS for {}", lookup_domain))?;

        let mut ips = HashSet::new();
        for ip in response.iter() {
            ips.insert(ip);
            common::log_debug!("  Resolved {} -> {}", domain, ip);
        }

        if ips.is_empty() {
            anyhow::bail!("No IP addresses found for domain '{}'", domain);
        }

        common::log_info!("Resolved {} to {} IP(s): {:?}", domain, ips.len(), ips);

        self.domain_to_ips.insert(domain.to_string(), ips.clone());

        Ok(ips)
    }

    pub async fn resolve_required_domains(
        &self,
        domains: &HashSet<String>,
    ) -> Result<HashMap<String, HashSet<IpAddr>>> {
        let mut resolved = HashMap::with_capacity(domains.len());
        let mut failures = Vec::new();

        for domain in domains {
            match self.resolve_domain(domain).await {
                Ok(ips) => {
                    resolved.insert(domain.clone(), ips);
                }
                Err(e) => failures.push(format!("{}: {}", domain, e)),
            }
        }

        if !failures.is_empty() {
            failures.sort();
            anyhow::bail!(
                "Failed to resolve required intercept domain(s): {}",
                failures.join("; ")
            );
        }

        Ok(resolved)
    }

    pub fn get_all_intercept_ips(&self) -> HashSet<IpAddr> {
        let mut all_ips = HashSet::new();
        for entry in self.domain_to_ips.iter() {
            all_ips.extend(entry.value().iter().cloned());
        }
        all_ips
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires external DNS"]
    async fn test_resolve_domain() {
        let resolver = DomainResolver::new().await.unwrap();
        let ips = resolver.resolve_domain("google.com").await.unwrap();
        assert!(!ips.is_empty(), "Should resolve at least one IP");
    }
}
