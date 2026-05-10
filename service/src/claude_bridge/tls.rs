use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose,
    IsCa, Issuer, KeyPair, KeyUsagePurpose, SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::ServerConfig;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

//
// Bridge CA material lives under ~/.praxis/bridge/.
//

const CA_CERT_FILE: &str = "ca_cert.pem";
const CA_KEY_FILE: &str = "ca_key.pem";

fn bridge_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".praxis")
        .join("bridge")
}

fn ca_cert_path() -> PathBuf {
    bridge_dir().join(CA_CERT_FILE)
}

fn ca_key_path() -> PathBuf {
    bridge_dir().join(CA_KEY_FILE)
}

//
// CertificateParams describing the bridge CA. The CA is regenerated only when
// no key file exists on disk; the parameters here are used both to self-sign
// the freshly generated CA and to wrap a loaded key into an Issuer for
// signing leaf certs later.
//

fn ca_params() -> CertificateParams {
    let mut params = CertificateParams::default();

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Praxis Claude Bridge CA");
    dn.push(DnType::OrganizationName, "Praxis");
    params.distinguished_name = dn;

    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];

    params.not_before = time::OffsetDateTime::now_utc();
    params.not_after = time::OffsetDateTime::now_utc()
        .checked_add(time::Duration::days(3650))
        .unwrap_or_else(time::OffsetDateTime::now_utc);

    params
}

fn load_ca_pem() -> Option<(String, String)> {
    let cert = std::fs::read_to_string(ca_cert_path()).ok()?;
    let key = std::fs::read_to_string(ca_key_path()).ok()?;
    Some((cert, key))
}

fn save_ca_pem(cert_pem: &str, key_pem: &str) -> Result<()> {
    let cert_path = ca_cert_path();
    let key_path = ca_key_path();
    if let Some(parent) = cert_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    std::fs::write(&cert_path, cert_pem)
        .with_context(|| format!("Failed to write bridge CA cert to {}", cert_path.display()))?;
    std::fs::write(&key_path, key_pem)
        .with_context(|| format!("Failed to write bridge CA key to {}", key_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&key_path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(&key_path, perms);
        }
    }
    Ok(())
}

fn parse_first_cert_der(pem: &str) -> Result<CertificateDer<'static>> {
    let mut bytes = pem.as_bytes();
    rustls_pemfile::certs(&mut bytes)
        .next()
        .ok_or_else(|| anyhow::anyhow!("CA PEM contained no certificates"))?
        .map_err(|e| anyhow::anyhow!("Failed to parse CA cert PEM: {}", e))
}

//
// DynamicResolver issues a fresh leaf certificate for whatever SNI hostname
// the TLS client sends, signed by our persisted CA. Because Claude Code only
// accepts an --sdk-url that resolves to an approved Anthropic hostname, the
// operator points one of those hostnames at this service via DNS or
// /etc/hosts and the resolver mints a matching leaf on demand. Leaves are
// cached in memory so reconnects don't pay the keygen cost twice.
//

struct DynamicResolver {
    issuer: Issuer<'static, KeyPair>,
    ca_der: CertificateDer<'static>,
    cache: Mutex<HashMap<String, Arc<CertifiedKey>>>,
}

impl std::fmt::Debug for DynamicResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cached = self.cache.lock().map(|c| c.len()).unwrap_or(0);
        f.debug_struct("DynamicResolver")
            .field("cached_leaves", &cached)
            .finish()
    }
}

impl DynamicResolver {
    fn load_or_create() -> Result<Arc<Self>> {
        let (issuer, ca_der) = match load_ca_pem() {
            Some((cert_pem, key_pem)) => {
                let key = KeyPair::from_pem(&key_pem).context("Failed to parse bridge CA key PEM")?;
                let der = parse_first_cert_der(&cert_pem)?;
                (Issuer::new(ca_params(), key), der)
            }
            None => {
                common::log_info!("Generating self-signed CA for Claude bridge TLS");
                let key = KeyPair::generate().context("Failed to generate bridge CA key")?;
                let params = ca_params();
                let cert = params
                    .clone()
                    .self_signed(&key)
                    .context("Failed to self-sign bridge CA")?;
                let cert_pem = cert.pem();
                let der = cert.der().clone();
                drop(cert);
                save_ca_pem(&cert_pem, &key.serialize_pem())?;
                (Issuer::new(params, key), der)
            }
        };

        Ok(Arc::new(Self {
            issuer,
            ca_der,
            cache: Mutex::new(HashMap::new()),
        }))
    }

    fn issue(&self, sni: Option<&str>) -> Result<Arc<CertifiedKey>> {
        let name = sni.unwrap_or("localhost").to_string();

        if let Some(cached) = self.cache.lock().unwrap().get(&name) {
            return Ok(cached.clone());
        }

        let key_pair = KeyPair::generate().context("Failed to generate leaf key")?;
        let mut params = CertificateParams::default();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, &name);
        params.distinguished_name = dn;
        params.is_ca = IsCa::NoCa;
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.not_before = time::OffsetDateTime::now_utc();
        params.not_after = time::OffsetDateTime::now_utc()
            .checked_add(time::Duration::days(365))
            .unwrap_or_else(time::OffsetDateTime::now_utc);

        let mut sans: Vec<SanType> = Vec::new();
        if let Ok(ip) = name.parse::<std::net::IpAddr>() {
            sans.push(SanType::IpAddress(ip));
        } else if let Ok(dns) = name.as_str().try_into() {
            sans.push(SanType::DnsName(dns));
        }

        //
        // Without SNI we have no idea what hostname the client expects, so
        // produce a fallback cert that covers the most common loopback
        // identities.
        //
        if sni.is_none() {
            if let Ok(ip4) = "127.0.0.1".parse() {
                sans.push(SanType::IpAddress(ip4));
            }
            if let Ok(ip6) = "::1".parse() {
                sans.push(SanType::IpAddress(ip6));
            }
        }
        params.subject_alt_names = sans;

        let leaf = params
            .signed_by(&key_pair, &self.issuer)
            .context("Failed to sign leaf certificate")?;
        let leaf_der: CertificateDer<'static> = leaf.der().clone();

        let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
        let signing_key = rustls::crypto::ring::sign::any_supported_type(&key_der)
            .context("Failed to convert leaf key into rustls signing key")?;

        let chain = vec![leaf_der, self.ca_der.clone()];
        let certified = Arc::new(CertifiedKey::new(chain, signing_key));

        self.cache
            .lock()
            .unwrap()
            .insert(name.clone(), certified.clone());

        common::log_info!("Issued bridge TLS leaf for SNI '{}'", name);

        Ok(certified)
    }
}

impl ResolvesServerCert for DynamicResolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let sni = hello.server_name();
        common::log_debug!("Bridge TLS ClientHello: SNI={:?}", sni);
        match self.issue(sni) {
            Ok(ck) => Some(ck),
            Err(e) => {
                common::log_error!("Failed to resolve bridge cert for SNI {:?}: {}", sni, e);
                None
            }
        }
    }
}

//
// Build a rustls ServerConfig wired to the dynamic resolver. The same config
// is shared between the CCRv1 and CCRv2 listeners.
//

pub fn build_server_config() -> Result<Arc<ServerConfig>> {
    //
    // rustls 0.23 requires a crypto provider; install ring's once. Repeat
    // calls return AlreadyInstalled which we ignore.
    //
    let _ = rustls::crypto::ring::default_provider().install_default();

    let resolver = DynamicResolver::load_or_create()?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);

    //
    // Do NOT set alpn_protocols. With ALPN negotiation active, rustls picks
    // a protocol from the intersection of the client's and server's lists,
    // and Claude Code's WebSocket client (Node ws) closes the connection
    // immediately after a successful TLS handshake when the negotiated
    // protocol doesn't match what its HTTP/1.1 upgrade path expects --
    // which manifests as "WebSocket protocol error: Handshake not finished"
    // in tungstenite. With alpn_protocols empty the server simply doesn't
    // participate in ALPN, and the connection proceeds without it. axum's
    // hyper-util auto::Builder still detects HTTP/1.1 vs HTTP/2 from the
    // connection preface for CCRv2.
    //

    Ok(Arc::new(config))
}
