use anyhow::{Context, Result};
use std::sync::Arc;

use super::tun_device::{LinuxTunDevice, SharedTunDevice};

/// TUN adapter name on Linux.
pub const ADAPTER_NAME: &str = "praxis0";

/// Linux TUN manager for VPN-based interception.
///
/// Manages the lifecycle of a TUN device on Linux using the tun crate.
pub struct LinuxTunManager {
    /// The TUN device (created when started)
    device: Option<SharedTunDevice>,
    /// Whether the manager is currently active
    is_active: bool,
}

impl LinuxTunManager {
    pub fn new() -> Self {
        Self {
            device: None,
            is_active: false,
        }
    }

    /// Start the TUN adapter and create a packet session.
    ///
    /// This creates a TUN device named "praxis0" that can be used for
    /// packet-level interception.
    pub fn start(&mut self) -> Result<()> {
        if self.is_active {
            common::log_info!("TUN adapter already active");
            return Ok(());
        }

        common::log_info!("Creating Linux TUN device: {}", ADAPTER_NAME);

        //
        // Configure the TUN device.
        //
        let mut config = tun::Configuration::default();
        config
            .tun_name(ADAPTER_NAME)
            .layer(tun::Layer::L3)  // Layer 3 (IP packets only, no ethernet header)
            .up();

        //
        // Create the TUN device.
        //
        let tun_device = tun::create(&config)
            .context("Failed to create TUN device. Make sure you have CAP_NET_ADMIN or are running as root.")?;

        common::log_info!("TUN device {} created successfully", ADAPTER_NAME);

        let device = Arc::new(LinuxTunDevice::new(tun_device));
        self.device = Some(device);
        self.is_active = true;

        common::log_info!("Linux TUN adapter started successfully");
        Ok(())
    }

    /// Stop the TUN adapter.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_active {
            return Ok(());
        }

        common::log_debug!("Stopping Linux TUN adapter");

        //
        // Shutdown the device first.
        //
        if let Some(device) = &self.device {
            device.shutdown();
        }

        //
        // Drop the device.
        //
        self.device = None;
        self.is_active = false;

        common::log_info!("Linux TUN adapter stopped");
        Ok(())
    }

    /// Shutdown the device to unblock any blocking reads.
    ///
    /// This should be called before waiting for the packet engine to stop.
    #[allow(dead_code)]
    pub fn shutdown_device(&self) {
        if let Some(device) = &self.device {
            common::log_debug!("Shutting down TUN device to unblock readers");
            device.shutdown();
        }
    }

    /// Get the TUN device for reading/writing packets.
    ///
    /// Returns None if the adapter hasn't been started.
    pub fn device(&self) -> Option<SharedTunDevice> {
        self.device.clone()
    }

    /// Check if the TUN adapter is active.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }
}

impl Drop for LinuxTunManager {
    fn drop(&mut self) {
        if self.is_active {
            if let Err(e) = self.stop() {
                common::log_error!("Failed to stop TUN adapter on drop: {}", e);
            }
        }
    }
}

impl Default for LinuxTunManager {
    fn default() -> Self {
        Self::new()
    }
}
