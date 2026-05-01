#![allow(dead_code)]

use anyhow::{Context, Result};
use dashmap::DashMap;
use hickory_resolver::TokioResolver;
use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use std::collections::HashSet;
use std::net::IpAddr;

/// DNS resolver that tracks domain-to-IP mappings for interception
pub struct DomainResolver {
    /// Async DNS resolver
    resolver: TokioResolver,
    /// Mapping of domain to resolved IPs
    domain_to_ips: DashMap<String, HashSet<IpAddr>>,
    /// Reverse mapping of IP to domain (for packet engine lookups)
    ip_to_domain: DashMap<IpAddr, String>,
}

impl DomainResolver {
    /// Create a new domain resolver using system DNS configuration
    pub async fn new() -> Result<Self> {
        let resolver = TokioResolver::builder_with_config(
            ResolverConfig::default(),
            TokioRuntimeProvider::default(),
        )
        .with_options(ResolverOpts::default())
        .build()
        .context("Failed to build DNS resolver")?;

        Ok(Self {
            resolver,
            domain_to_ips: DashMap::new(),
            ip_to_domain: DashMap::new(),
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
            //
            // Store reverse mapping.
            //
            self.ip_to_domain.insert(ip, domain.to_string());
            common::log_debug!("  Resolved {} -> {}", domain, ip);
        }

        if ips.is_empty() {
            common::log_warn!("No IP addresses found for domain: {}", domain);
        } else {
            common::log_info!("Resolved {} to {} IP(s): {:?}", domain, ips.len(), ips);
        }

        self.domain_to_ips.insert(domain.to_string(), ips.clone());

        Ok(ips)
    }

    pub fn get_all_intercept_ips(&self) -> HashSet<IpAddr> {
        let mut all_ips = HashSet::new();
        for entry in self.domain_to_ips.iter() {
            all_ips.extend(entry.value().iter().cloned());
        }
        all_ips
    }

    /// Used by the packet engine to determine if a packet should be intercepted.
    pub fn lookup_domain_for_ip(&self, ip: IpAddr) -> Option<String> {
        self.ip_to_domain.get(&ip).map(|r| r.value().clone())
    }

    pub fn is_intercept_ip(&self, ip: &IpAddr) -> bool {
        self.ip_to_domain.contains_key(ip)
    }

    pub fn get_resolved_domains(&self) -> Vec<String> {
        self.domain_to_ips.iter().map(|e| e.key().clone()).collect()
    }

    pub fn clear(&self) {
        self.domain_to_ips.clear();
        self.ip_to_domain.clear();
    }

    /// Re-resolve all domains (for periodic refresh)
    pub async fn refresh_all(&self) -> Result<()> {
        let domains: Vec<String> = self.domain_to_ips.iter().map(|e| e.key().clone()).collect();

        for domain in domains {
            if let Err(e) = self.resolve_domain(&domain).await {
                common::log_warn!("Failed to refresh DNS for {}: {}", domain, e);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resolve_domain() {
        let resolver = DomainResolver::new().await.unwrap();
        let ips = resolver.resolve_domain("google.com").await.unwrap();
        assert!(!ips.is_empty(), "Should resolve at least one IP");
    }

    #[tokio::test]
    async fn test_reverse_lookup() {
        let resolver = DomainResolver::new().await.unwrap();
        let ips = resolver.resolve_domain("google.com").await.unwrap();

        let first_ip = ips.iter().next().unwrap();
        let domain = resolver.lookup_domain_for_ip(*first_ip);
        assert_eq!(domain, Some("google.com".to_string()));
    }
}
