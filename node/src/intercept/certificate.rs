use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose,
    IsCa, Issuer, KeyPair, KeyUsagePurpose, SanType,
};
use std::collections::HashMap;
use std::sync::Arc;
#[allow(unused_imports)]

#[cfg(target_os = "linux")]
use libc;

/// Linux distribution family for certificate installation
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
enum LinuxDistro {
    /// Debian, Ubuntu, Mint, etc.
    DebianBased,
    /// RHEL, Fedora, CentOS, Rocky, Alma
    RhelBased,
    /// Arch, Manjaro, EndeavourOS
    Arch,
    /// Unknown distro
    Unknown,
}

/// Certificate data containing the certificate and its private key
pub struct CertificateData {
    /// PEM-encoded certificate
    pub cert_pem: String,
    /// PEM-encoded private key
    pub key_pem: String,
}

/// Certificate Authority for generating and managing TLS certificates
pub struct CertificateAuthority {
    /// Root CA issuer (owns params + signing key, used to sign leaf certs)
    root_issuer: Issuer<'static, KeyPair>,
    /// Root CA certificate PEM (for easy access)
    #[allow(dead_code)]
    root_cert_pem: String,
    /// Root CA thumbprint (for cleanup on Windows)
    #[allow(dead_code)]
    root_thumbprint: Option<String>,
    /// Cached leaf certificates for domains
    leaf_certs: HashMap<String, Arc<CertificateData>>,
    /// Path where certificate was installed (Linux system cert store)
    #[cfg(target_os = "linux")]
    linux_cert_path: Option<std::path::PathBuf>,
    /// Linux distro type (for cleanup)
    #[cfg(target_os = "linux")]
    linux_distro: Option<LinuxDistro>,
}

impl CertificateAuthority {
    /// Generate a new root CA certificate
    pub fn new() -> Result<Self> {
        common::log_info!("Generating new root CA certificate");

        //
        // Generate key pair for root CA.
        //
        let root_key_pair = KeyPair::generate()
            .context("Failed to generate root CA key pair")?;

        //
        // Set up root CA certificate parameters.
        //
        let mut params = CertificateParams::default();

        //
        // Set distinguished name.
        //
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "Praxis Intercept CA");
        dn.push(DnType::OrganizationName, "Praxis");
        dn.push(DnType::OrganizationalUnitName, "Traffic Interception");
        params.distinguished_name = dn;

        //
        // Make it a CA certificate.
        //
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

        //
        // Set key usage for CA.
        //
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];

        //
        // Set validity period (1 year).
        //
        params.not_before = time::OffsetDateTime::now_utc();
        params.not_after = time::OffsetDateTime::now_utc()
            .checked_add(time::Duration::days(365))
            .unwrap_or_else(|| time::OffsetDateTime::now_utc());

        //
        // Self-sign the certificate, then wrap the params and key pair in an
        // Issuer for signing leaf certs later. rcgen 0.14 removed the direct
        // (cert, key_pair) arguments to signed_by and replaced them with an
        // Issuer handle.
        //
        let root_cert = params
            .self_signed(&root_key_pair)
            .context("Failed to self-sign root CA certificate")?;

        let root_cert_pem = root_cert.pem();
        drop(root_cert);
        let root_issuer = Issuer::new(params, root_key_pair);

        common::log_info!("Root CA certificate generated successfully");

        Ok(Self {
            root_issuer,
            root_cert_pem,
            root_thumbprint: None,
            leaf_certs: HashMap::new(),
            #[cfg(target_os = "linux")]
            linux_cert_path: None,
            #[cfg(target_os = "linux")]
            linux_distro: None,
        })
    }

    #[allow(dead_code)]
    pub fn root_cert_pem(&self) -> &str {
        &self.root_cert_pem
    }

    pub fn generate_leaf_cert(&mut self, domain: &str) -> Result<Arc<CertificateData>> {
        if let Some(cert_data) = self.leaf_certs.get(domain) {
            return Ok(Arc::clone(cert_data));
        }

        common::log_info!("Generating leaf certificate for domain: {}", domain);

        let leaf_key_pair = KeyPair::generate()
            .context("Failed to generate leaf certificate key pair")?;

        let mut params = CertificateParams::default();

        //
        // Set distinguished name.
        //
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, domain);
        params.distinguished_name = dn;

        //
        // Not a CA.
        //
        params.is_ca = IsCa::NoCa;

        //
        // Set key usage for server authentication.
        //
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];

        //
        // Set extended key usage for TLS server authentication.
        //
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
        ];

        //
        // Add Subject Alternative Names.
        //
        params.subject_alt_names = vec![
            SanType::DnsName(domain.try_into().context("Invalid domain name")?),
        ];

        //
        // Also add wildcard if it's not already a wildcard.
        //
        if !domain.starts_with("*.") {
            if let Ok(wildcard) = format!("*.{}", domain).try_into() {
                params.subject_alt_names.push(SanType::DnsName(wildcard));
            }
        }

        //
        // Set validity period (1 year, same as root).
        //
        params.not_before = time::OffsetDateTime::now_utc();
        params.not_after = time::OffsetDateTime::now_utc()
            .checked_add(time::Duration::days(365))
            .unwrap_or_else(|| time::OffsetDateTime::now_utc());

        //
        // Sign with root CA.
        //
        let leaf_cert = params
            .signed_by(&leaf_key_pair, &self.root_issuer)
            .context("Failed to sign leaf certificate with root CA")?;

        let cert_pem = leaf_cert.pem();

        //
        // Log certificate details for debugging.
        //
        common::log_info!("Leaf certificate generated for domain: {}", domain);
        common::log_info!("  Subject: CN={}", domain);
        common::log_info!("  SANs: DNS:{}, DNS:*.{}", domain, domain);
        common::log_info!("  Key Usage: digitalSignature, keyEncipherment");
        common::log_info!("  Extended Key Usage: serverAuth");
        common::log_debug!("Certificate PEM:\n{}", cert_pem);

        let cert_data = Arc::new(CertificateData {
            cert_pem,
            key_pem: leaf_key_pair.serialize_pem(),
        });

        self.leaf_certs.insert(domain.to_string(), Arc::clone(&cert_data));

        Ok(cert_data)
    }

    #[allow(dead_code)]
    pub fn get_leaf_cert(&self, domain: &str) -> Option<Arc<CertificateData>> {
        self.leaf_certs.get(domain).map(Arc::clone)
    }

    #[cfg(target_os = "windows")]
    pub fn install_root_cert(&mut self) -> Result<()> {
        common::log_info!("Installing root CA certificate in Windows certificate store");

        let temp_dir = std::env::temp_dir().join("praxis_certs");
        std::fs::create_dir_all(&temp_dir)?;

        let cert_path = temp_dir.join("praxis_root_ca.cer");
        std::fs::write(&cert_path, &self.root_cert_pem)?;

        //
        // PowerShell script to install the certificate
        // Try LocalMachine first (requires admin), fall back to CurrentUser.
        //
        let ps_script = format!(
            r#"
            $certPath = "{cert_path}"
            $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($certPath)
            $thumbprint = $cert.Thumbprint

            # Try machine store first
            $installed = $false
            try {{
                $store = New-Object System.Security.Cryptography.X509Certificates.X509Store("Root", "LocalMachine")
                $store.Open("ReadWrite")
                $store.Add($cert)
                $store.Close()
                Write-Host "Installed in LocalMachine store"
                $installed = $true
            }} catch {{
                Write-Host "Failed to install in LocalMachine store: $_"
            }}

            # Fall back to user store
            if (-not $installed) {{
                try {{
                    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store("Root", "CurrentUser")
                    $store.Open("ReadWrite")
                    $store.Add($cert)
                    $store.Close()
                    Write-Host "Installed in CurrentUser store"
                }} catch {{
                    Write-Host "Failed to install in CurrentUser store: $_"
                    exit 1
                }}
            }}

            # Output thumbprint for cleanup later
            Write-Output $thumbprint
            "#,
            cert_path = cert_path.display()
        );

        let output = crate::utils::silent_command("powershell")
            .args(["-ExecutionPolicy", "Bypass", "-Command", &ps_script])
            .output()
            .context("Failed to execute PowerShell for certificate installation")?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            //
            // Extract thumbprint from output.
            //
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.len() == 40 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                    self.root_thumbprint = Some(trimmed.to_string());
                    common::log_info!("Root CA installed with thumbprint: {}", trimmed);
                    break;
                }
            }
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            common::log_error!("Failed to install root CA: {}", stderr);
            Err(anyhow::anyhow!("Failed to install root CA certificate: {}", stderr))
        }
    }

    /// Install the root CA certificate on Linux
    ///
    /// If running as root, installs to the system certificate store.
    /// NODE_EXTRA_CA_CERTS is always set via env_vars.rs.
    #[cfg(target_os = "linux")]
    pub fn install_root_cert(&mut self) -> Result<()> {
        common::log_info!("Checking if running as root for system certificate installation");

        //
        // Check if running as root.
        //
        let is_root = unsafe { libc::geteuid() } == 0;

        if !is_root {
            common::log_info!("Not running as root - skipping system certificate store installation");
            common::log_info!("NODE_EXTRA_CA_CERTS will be set for Node.js applications");
            return Ok(());
        }

        //
        // Detect Linux distribution.
        //
        let distro = detect_linux_distro();
        self.linux_distro = Some(distro);
        common::log_info!("Detected Linux distribution: {:?}", distro);

        //
        // Determine certificate path and update command based on distro.
        //
        let (cert_dir, cert_name, update_cmd) = match distro {
            LinuxDistro::DebianBased => (
                std::path::Path::new("/usr/local/share/ca-certificates"),
                "praxis_root_ca.crt",
                vec!["update-ca-certificates"],
            ),
            LinuxDistro::RhelBased => (
                std::path::Path::new("/etc/pki/ca-trust/source/anchors"),
                "praxis_root_ca.pem",
                vec!["update-ca-trust"],
            ),
            LinuxDistro::Arch => (
                std::path::Path::new("/etc/ca-certificates/trust-source/anchors"),
                "praxis_root_ca.crt",
                vec!["trust", "extract-compat"],
            ),
            LinuxDistro::Unknown => {
                common::log_warn!("Unknown Linux distribution - skipping system certificate installation");
                common::log_info!("NODE_EXTRA_CA_CERTS will be set for Node.js applications");
                return Ok(());
            }
        };

        //
        // Create directory if needed.
        //
        if !cert_dir.exists() {
            std::fs::create_dir_all(cert_dir)
                .context("Failed to create certificate directory")?;
        }

        //
        // Write certificate file.
        //
        let cert_path = cert_dir.join(cert_name);
        std::fs::write(&cert_path, &self.root_cert_pem)
            .context("Failed to write certificate to system store")?;
        self.linux_cert_path = Some(cert_path.clone());
        common::log_info!("Installed certificate to: {}", cert_path.display());

        //
        // Run update command.
        //
        let output = std::process::Command::new(&update_cmd[0])
            .args(&update_cmd[1..])
            .output()
            .context("Failed to run certificate update command")?;

        if output.status.success() {
            common::log_info!("System certificate store updated successfully");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            common::log_warn!("Certificate update command returned non-zero: {}", stderr);
        }

        Ok(())
    }

    /// Install the root CA certificate (non-Windows/non-Linux stub)
    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    pub fn install_root_cert(&mut self) -> Result<()> {
        common::log_warn!("Certificate installation not implemented for this platform");
        Ok(())
    }

    /// Uninstall the root CA certificate from the Windows certificate store
    #[cfg(target_os = "windows")]
    pub fn uninstall_root_cert(&self) -> Result<()> {
        let thumbprint = match &self.root_thumbprint {
            Some(t) => t,
            None => {
                common::log_warn!("No thumbprint recorded, cannot uninstall certificate");
                return Ok(());
            }
        };

        common::log_info!("Uninstalling root CA certificate with thumbprint: {}", thumbprint);

        let ps_script = format!(
            r#"
            $thumbprint = "{thumbprint}"

            # Try to remove from both stores
            foreach ($location in @("CurrentUser", "LocalMachine")) {{
                try {{
                    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store("Root", $location)
                    $store.Open("ReadWrite")
                    $cert = $store.Certificates | Where-Object {{ $_.Thumbprint -eq $thumbprint }}
                    if ($cert) {{
                        $store.Remove($cert)
                        Write-Host "Removed from $location store"
                    }}
                    $store.Close()
                }} catch {{
                    Write-Host "Could not access $location store: $_"
                }}
            }}
            "#,
            thumbprint = thumbprint
        );

        let output = crate::utils::silent_command("powershell")
            .args(["-ExecutionPolicy", "Bypass", "-Command", &ps_script])
            .output()
            .context("Failed to execute PowerShell for certificate uninstallation")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            common::log_warn!("Certificate uninstallation may have failed: {}", stderr);
        }

        //
        // Clean up temp files.
        //
        let temp_dir = std::env::temp_dir().join("praxis_certs");
        let _ = std::fs::remove_dir_all(&temp_dir);

        common::log_info!("Root CA certificate uninstalled");
        Ok(())
    }

    /// Uninstall the root CA certificate from Linux system store
    #[cfg(target_os = "linux")]
    pub fn uninstall_root_cert(&self) -> Result<()> {
        //
        // Check if we installed to system cert store.
        //
        let cert_path = match &self.linux_cert_path {
            Some(p) => p,
            None => {
                common::log_info!("No system certificate to uninstall");
                return Ok(());
            }
        };

        //
        // Remove the certificate file.
        //
        if cert_path.exists() {
            std::fs::remove_file(cert_path)
                .context("Failed to remove certificate from system store")?;
            common::log_info!("Removed certificate from: {}", cert_path.display());
        }

        //
        // Run update command to refresh the store.
        //
        let update_cmd = match self.linux_distro {
            Some(LinuxDistro::DebianBased) => vec!["update-ca-certificates"],
            Some(LinuxDistro::RhelBased) => vec!["update-ca-trust"],
            Some(LinuxDistro::Arch) => vec!["trust", "extract-compat"],
            _ => return Ok(()),
        };

        let output = std::process::Command::new(&update_cmd[0])
            .args(&update_cmd[1..])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                common::log_info!("System certificate store updated after removal");
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                common::log_warn!("Certificate update command returned non-zero: {}", stderr);
            }
            Err(e) => {
                common::log_warn!("Failed to run certificate update command: {}", e);
            }
        }

        //
        // Clean up temp files.
        //
        let temp_dir = std::env::temp_dir().join("praxis_certs");
        let _ = std::fs::remove_dir_all(&temp_dir);

        common::log_info!("Root CA certificate uninstalled");
        Ok(())
    }

    /// Uninstall the root CA certificate (non-Windows/non-Linux stub)
    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    pub fn uninstall_root_cert(&self) -> Result<()> {
        common::log_warn!("Certificate uninstallation not implemented for this platform");
        Ok(())
    }

    /// Clear all cached leaf certificates
    #[allow(dead_code)]
    pub fn clear_leaf_certs(&mut self) {
        self.leaf_certs.clear();
    }

    /// Get the certificate thumbprint (Windows only)
    #[cfg(target_os = "windows")]
    pub fn thumbprint(&self) -> Option<&str> {
        self.root_thumbprint.as_deref()
    }

    /// Get the path where the certificate was installed (Linux only)
    #[cfg(target_os = "linux")]
    pub fn cert_path(&self) -> Option<String> {
        self.linux_cert_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    /// Get the Linux distribution type as a string (Linux only)
    #[cfg(target_os = "linux")]
    pub fn linux_distro_name(&self) -> Option<&'static str> {
        self.linux_distro.map(|d| match d {
            LinuxDistro::DebianBased => "debian",
            LinuxDistro::RhelBased => "rhel",
            LinuxDistro::Arch => "arch",
            LinuxDistro::Unknown => "unknown",
        })
    }
}

impl Drop for CertificateAuthority {
    fn drop(&mut self) {
        //
        // Best effort cleanup - don't uninstall automatically on drop
        // as the user might want to keep the cert for debugging.
        //
        self.leaf_certs.clear();
    }
}

/// Detect the Linux distribution family by parsing /etc/os-release
#[cfg(target_os = "linux")]
fn detect_linux_distro() -> LinuxDistro {
    let os_release = match std::fs::read_to_string("/etc/os-release") {
        Ok(content) => content,
        Err(_) => return LinuxDistro::Unknown,
    };

    //
    // Parse ID and ID_LIKE from os-release.
    //
    let mut id = String::new();
    let mut id_like = String::new();

    for line in os_release.lines() {
        if line.starts_with("ID=") {
            id = line[3..].trim_matches('"').to_lowercase();
        } else if line.starts_with("ID_LIKE=") {
            id_like = line[8..].trim_matches('"').to_lowercase();
        }
    }

    //
    // Check for Debian-based distros.
    //
    if id == "debian" || id == "ubuntu" || id == "linuxmint" || id == "pop"
        || id_like.contains("debian") || id_like.contains("ubuntu")
    {
        return LinuxDistro::DebianBased;
    }

    //
    // Check for RHEL-based distros.
    //
    if id == "fedora" || id == "rhel" || id == "centos" || id == "rocky" || id == "almalinux"
        || id_like.contains("fedora") || id_like.contains("rhel")
    {
        return LinuxDistro::RhelBased;
    }

    //
    // Check for Arch-based distros.
    //
    if id == "arch" || id == "manjaro" || id == "endeavouros"
        || id_like.contains("arch")
    {
        return LinuxDistro::Arch;
    }

    LinuxDistro::Unknown
}
