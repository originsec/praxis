#[cfg(target_os = "windows")]
use anyhow::Context;
use anyhow::Result;
#[cfg(target_os = "windows")]
use std::fs;
#[cfg(target_os = "windows")]
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::sync::Arc;

/// Embedded wintun.dll for AMD64 Windows
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const WINTUN_DLL: &[u8] = include_bytes!("../../assets/wintun_amd64.dll");

/// Wintun adapter name
#[cfg(target_os = "windows")]
pub const ADAPTER_NAME: &str = "Praxis VPN";
/// Wintun tunnel type
#[cfg(target_os = "windows")]
const TUNNEL_TYPE: &str = "Praxis";

/// Path where wintun.dll will be extracted
#[cfg(target_os = "windows")]
fn wintun_dll_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("praxis_wintun.dll");
    path
}

/// Wintun manager for VPN-based interception
#[cfg(target_os = "windows")]
pub struct WintunManager {
    /// The wintun adapter (created when started)
    adapter: Option<Arc<wintun::Adapter>>,
    /// Packet I/O session
    session: Option<Arc<wintun::Session>>,
    /// Whether the manager is currently active
    is_active: bool,
}

#[cfg(target_os = "windows")]
impl WintunManager {
    /// Create a new WintunManager
    pub fn new() -> Self {
        Self {
            adapter: None,
            session: None,
            is_active: false,
        }
    }

    /// Extract the embedded wintun.dll to a temp location
    fn extract_wintun_dll() -> Result<PathBuf> {
        let dll_path = wintun_dll_path();

        //
        // Check if DLL already exists and is valid.
        //
        if dll_path.exists() {
            //
            // Verify size matches embedded version.
            //
            if let Ok(metadata) = fs::metadata(&dll_path) {
                if metadata.len() == WINTUN_DLL.len() as u64 {
                    common::log_info!("Using existing wintun.dll at {:?}", dll_path);
                    return Ok(dll_path);
                }
            }
        }

        //
        // Extract the embedded DLL.
        //
        common::log_info!("Extracting wintun.dll to {:?}", dll_path);
        fs::write(&dll_path, WINTUN_DLL).context("Failed to write wintun.dll to temp directory")?;

        Ok(dll_path)
    }

    /// Start the VPN adapter and create a packet session
    ///
    /// This creates a virtual network adapter named "Praxis VPN" that
    /// appears in Windows Network Connections, and opens a session
    /// for reading/writing packets.
    pub fn start(&mut self) -> Result<()> {
        if self.is_active {
            common::log_info!("Wintun adapter already active");
            return Ok(());
        }

        //
        // Extract wintun.dll.
        //
        let dll_path = Self::extract_wintun_dll()?;

        //
        // Load wintun library.
        //
        let wintun =
            unsafe { wintun::load_from_path(&dll_path).context("Failed to load wintun.dll")? };

        //
        // Always create a new adapter. The wintun crate logs errors when open()
        // fails to find an existing adapter, which is noisy. Creating always
        // works and will reuse an existing adapter with the same name if present.
        //
        common::log_info!("Creating Praxis VPN adapter");
        let adapter = wintun::Adapter::create(&wintun, ADAPTER_NAME, TUNNEL_TYPE, None)
            .context("Failed to create wintun adapter")?;

        //
        // Start a packet session with maximum ring buffer capacity.
        //
        common::log_debug!(
            "Starting wintun session with ring capacity: {}",
            wintun::MAX_RING_CAPACITY
        );
        let session = adapter
            .start_session(wintun::MAX_RING_CAPACITY)
            .context("Failed to start wintun session")?;

        self.adapter = Some(adapter);
        self.session = Some(Arc::new(session));
        self.is_active = true;

        common::log_info!("Praxis VPN adapter started successfully with packet session");
        Ok(())
    }

    /// Stop the VPN adapter and close the session
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_active {
            return Ok(());
        }

        //
        // Shutdown the session first (this unblocks any blocking reads).
        //
        if let Some(session) = self.session.take() {
            common::log_debug!("Shutting down wintun session");
            let _ = session.shutdown();
            //
            // Drop the session.
            //
            drop(session);
        }

        //
        // Drop the adapter (this will close it).
        //
        if let Some(adapter) = self.adapter.take() {
            drop(adapter);
            common::log_info!("Praxis VPN adapter stopped");
        }

        self.is_active = false;
        Ok(())
    }

    #[allow(dead_code)]
    /// Shutdown the session to unblock any blocking reads
    /// This should be called before waiting for the packet engine to stop
    pub fn shutdown_session(&self) {
        if let Some(session) = &self.session {
            common::log_debug!("Shutting down wintun session to unblock readers");
            let _ = session.shutdown();
        }
    }

    /// Get the packet session for reading/writing packets
    ///
    /// Returns None if the adapter hasn't been started
    pub fn session(&self) -> Option<Arc<wintun::Session>> {
        self.session.clone()
    }

    #[allow(dead_code)]
    /// Get a reference to the adapter
    pub fn adapter(&self) -> Option<&Arc<wintun::Adapter>> {
        self.adapter.as_ref()
    }

    #[allow(dead_code)]
    /// Check if the VPN adapter is active
    pub fn is_active(&self) -> bool {
        self.is_active
    }
}

#[cfg(target_os = "windows")]
impl Drop for WintunManager {
    fn drop(&mut self) {
        if self.is_active {
            if let Err(e) = self.stop() {
                common::log_error!("Failed to stop wintun adapter on drop: {}", e);
            }
        }
    }
}

//
// Non-Windows stub implementation.
//
#[allow(dead_code)]
#[cfg(not(target_os = "windows"))]
pub struct WintunManager {
    is_active: bool,
}

#[cfg(not(target_os = "windows"))]
impl WintunManager {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self { is_active: false }
    }

    #[allow(dead_code)]
    pub fn start(&mut self) -> Result<()> {
        common::log_warn!("Wintun VPN mode is only supported on Windows");
        Err(anyhow::anyhow!(
            "Wintun VPN mode is only supported on Windows"
        ))
    }

    #[allow(dead_code)]
    pub fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    #[allow(dead_code)]
    pub fn shutdown_session(&self) {
        //
        // No-op on non-Windows.
        //
    }

    //
    // Stub methods for non-Windows - these won't be called.
    //
    #[allow(dead_code)]
    pub fn session(&self) -> Option<std::sync::Arc<()>> {
        None
    }
}

impl Default for WintunManager {
    fn default() -> Self {
        Self::new()
    }
}
