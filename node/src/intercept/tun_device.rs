use anyhow::Result;
use std::sync::Arc;

/// Abstraction for TUN device operations across platforms.
///
/// This trait provides a unified interface for TUN device packet I/O,
/// allowing the packet engine to work with both Windows (Wintun) and
/// Linux (native TUN) implementations.
pub trait TunDevice: Send + Sync {
    /// Receive a packet from the TUN device (blocking).
    ///
    /// Returns the raw packet bytes. This call blocks until a packet is
    /// available or the device is shut down.
    fn receive_blocking(&self) -> Result<Vec<u8>>;

    /// Send a packet through the TUN device.
    fn send(&self, packet: &[u8]) -> Result<()>;

    /// Signal the device to shut down.
    ///
    /// This should unblock any pending receive_blocking() calls.
    fn shutdown(&self);
}

/// Arc wrapper for TunDevice to allow sharing across threads.
pub type SharedTunDevice = Arc<dyn TunDevice>;

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Windows TUN device implementation using Wintun.
    pub struct WintunDevice {
        session: Arc<wintun::Session>,
        shutdown_flag: AtomicBool,
    }

    impl WintunDevice {
        pub fn new(session: Arc<wintun::Session>) -> Self {
            Self {
                session,
                shutdown_flag: AtomicBool::new(false),
            }
        }
    }

    impl TunDevice for WintunDevice {
        fn receive_blocking(&self) -> Result<Vec<u8>> {
            if self.shutdown_flag.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!("Device is shut down"));
            }

            match self.session.receive_blocking() {
                Ok(packet) => {
                    let bytes = packet.bytes().to_vec();
                    common::log_trace!("Received {} bytes from wintun", bytes.len());
                    Ok(bytes)
                }
                Err(e) => {
                    if self.shutdown_flag.load(Ordering::Relaxed) {
                        Err(anyhow::anyhow!("Device is shut down"))
                    } else {
                        Err(anyhow::anyhow!("Wintun receive error: {}", e))
                    }
                }
            }
        }

        fn send(&self, packet: &[u8]) -> Result<()> {
            let mut wintun_packet = self.session.allocate_send_packet(packet.len() as u16)?;
            wintun_packet.bytes_mut().copy_from_slice(packet);
            self.session.send_packet(wintun_packet);
            common::log_trace!("Sent {} bytes to wintun", packet.len());
            Ok(())
        }

        fn shutdown(&self) {
            common::log_debug!("Shutting down wintun device");
            self.shutdown_flag.store(true, Ordering::Relaxed);
            let _ = self.session.shutdown();
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows::WintunDevice;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, RawFd};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Linux TUN device implementation using the tun crate.
    pub struct LinuxTunDevice {
        device: Mutex<tun::Device>,
        shutdown_flag: AtomicBool,
        fd: RawFd,
    }

    impl LinuxTunDevice {
        pub fn new(device: tun::Device) -> Self {
            let fd = device.as_raw_fd();
            Self {
                device: Mutex::new(device),
                shutdown_flag: AtomicBool::new(false),
                fd,
            }
        }
    }

    impl TunDevice for LinuxTunDevice {
        fn receive_blocking(&self) -> Result<Vec<u8>> {
            let mut buf = vec![0u8; 65535];

            loop {
                if self.shutdown_flag.load(Ordering::Relaxed) {
                    return Err(anyhow::anyhow!("Device is shut down"));
                }

                //
                // Use poll() with a 100ms timeout so we can check the shutdown
                // flag periodically.
                //
                let mut pollfd = libc::pollfd {
                    fd: self.fd,
                    events: libc::POLLIN,
                    revents: 0,
                };

                let poll_result = unsafe { libc::poll(&mut pollfd, 1, 100) };

                if poll_result < 0 {
                    let err = std::io::Error::last_os_error();
                    if err.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(anyhow::anyhow!("poll error: {}", err));
                }

                if poll_result == 0 {
                    //
                    // Timeout - check shutdown flag and try again.
                    //
                    continue;
                }

                if pollfd.revents & libc::POLLIN != 0 {
                    //
                    // Data available - read it.
                    //
                    let mut device = self.device.lock().unwrap();
                    match device.read(&mut buf) {
                        Ok(n) => {
                            buf.truncate(n);
                            common::log_trace!("Received {} bytes from TUN", n);
                            return Ok(buf);
                        }
                        Err(e) => {
                            if self.shutdown_flag.load(Ordering::Relaxed) {
                                return Err(anyhow::anyhow!("Device is shut down"));
                            }
                            if e.kind() == std::io::ErrorKind::WouldBlock {
                                continue;
                            }
                            return Err(anyhow::anyhow!("TUN read error: {}", e));
                        }
                    }
                }

                if pollfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                    return Err(anyhow::anyhow!("TUN device error or closed"));
                }
            }
        }

        fn send(&self, packet: &[u8]) -> Result<()> {
            let mut device = self.device.lock().unwrap();
            device
                .write_all(packet)
                .map_err(|e| anyhow::anyhow!("TUN write error: {}", e))?;
            common::log_trace!("Sent {} bytes to TUN", packet.len());
            Ok(())
        }

        fn shutdown(&self) {
            common::log_debug!("Shutting down Linux TUN device");
            self.shutdown_flag.store(true, Ordering::Relaxed);
            //
            // The receive_blocking loop will exit on the next poll timeout
            // when it sees the shutdown flag.
            //
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::LinuxTunDevice;

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
mod stub {
    use super::*;

    pub struct StubTunDevice;

    impl TunDevice for StubTunDevice {
        fn receive_blocking(&self) -> Result<Vec<u8>> {
            Err(anyhow::anyhow!("TUN device not supported on this platform"))
        }

        fn send(&self, _packet: &[u8]) -> Result<()> {
            Err(anyhow::anyhow!("TUN device not supported on this platform"))
        }

        fn shutdown(&self) {}
    }
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub use stub::StubTunDevice;
