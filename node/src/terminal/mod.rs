mod output;
mod session;

pub use output::TerminalOutputEvent;
pub use session::TerminalSession;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::Read;
use tokio::sync::mpsc;

pub struct TerminalManager {
    sessions: HashMap<String, TerminalSession>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub fn create_session(
        &mut self,
        terminal_id: String,
        client_id: String,
        output_tx: mpsc::UnboundedSender<TerminalOutputEvent>,
    ) -> anyhow::Result<String> {
        let pty_system = native_pty_system();

        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        //
        // Use appropriate shell for the platform.
        //
        #[cfg(windows)]
        let cmd = {
            let mut cmd = CommandBuilder::new("powershell.exe");
            cmd.arg("-NoLogo");
            cmd
        };

        #[cfg(unix)]
        let cmd = {
            //
            // Try to use the user's preferred shell, fallback to /bin/sh.
            //
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let mut cmd = CommandBuilder::new(&shell);
            //
            // Use login shell for proper environment setup.
            //
            cmd.arg("-l");
            cmd
        };

        let _child = pair.slave.spawn_command(cmd)?;

        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        let terminal_id_clone = terminal_id.clone();
        let client_id_clone = client_id.clone();

        std::thread::spawn(move || {
            common::log_info!("Terminal {} reader thread started", terminal_id_clone);
            let mut buf = [0u8; 4096];
            loop {
                if shutdown_rx.try_recv().is_ok() {
                    common::log_info!("Terminal {} reader shutting down", terminal_id_clone);
                    break;
                }

                match reader.read(&mut buf) {
                    Ok(0) => {
                        common::log_info!("Terminal {} EOF", terminal_id_clone);
                        let _ = output_tx.send(TerminalOutputEvent {
                            terminal_id: terminal_id_clone.clone(),
                            client_id: client_id_clone.clone(),
                            data: None,
                            closed: true,
                        });
                        break;
                    }
                    Ok(n) => {
                        common::log_info!("Terminal {} read {} bytes", terminal_id_clone, n);
                        let data = buf[..n].to_vec();
                        if output_tx
                            .send(TerminalOutputEvent {
                                terminal_id: terminal_id_clone.clone(),
                                client_id: client_id_clone.clone(),
                                data: Some(data),
                                closed: false,
                            })
                            .is_err()
                        {
                            common::log_warn!("Failed to send terminal output, channel closed");
                            break;
                        }
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            common::log_error!("Terminal {} read error: {}", terminal_id_clone, e);
                            let _ = output_tx.send(TerminalOutputEvent {
                                terminal_id: terminal_id_clone.clone(),
                                client_id: client_id_clone.clone(),
                                data: None,
                                closed: true,
                            });
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
            }
        });

        let session = TerminalSession {
            terminal_id: terminal_id.clone(),
            client_id,
            master: pair.master,
            writer,
            shutdown_tx: Some(shutdown_tx),
        };

        self.sessions.insert(terminal_id.clone(), session);
        common::log_info!("Created terminal session: {}", terminal_id);

        Ok(terminal_id)
    }

    pub fn write_to_session(&mut self, terminal_id: &str, data: &[u8]) -> anyhow::Result<()> {
        let session = self
            .sessions
            .get_mut(terminal_id)
            .ok_or_else(|| anyhow::anyhow!("Terminal session not found: {}", terminal_id))?;
        session.write_data(data)
    }

    pub fn resize_session(
        &mut self,
        terminal_id: &str,
        rows: u16,
        cols: u16,
    ) -> anyhow::Result<()> {
        let session = self
            .sessions
            .get(terminal_id)
            .ok_or_else(|| anyhow::anyhow!("Terminal session not found: {}", terminal_id))?;
        session.resize(rows, cols)
    }

    pub fn close_session(&mut self, terminal_id: &str) -> anyhow::Result<()> {
        if let Some(mut session) = self.sessions.remove(terminal_id) {
            session.close();
            common::log_info!("Closed terminal session: {}", terminal_id);
        }
        Ok(())
    }

    pub fn get_session_for_client(&self, client_id: &str) -> Option<&String> {
        self.sessions
            .iter()
            .find(|(_, s)| s.client_id == client_id)
            .map(|(id, _)| id)
    }

    #[allow(dead_code)]
    pub fn has_session(&self, terminal_id: &str) -> bool {
        self.sessions.contains_key(terminal_id)
    }

    #[allow(dead_code)]
    pub fn close_all_for_client(&mut self, client_id: &str) {
        let to_close: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.client_id == client_id)
            .map(|(id, _)| id.clone())
            .collect();

        for id in to_close {
            let _ = self.close_session(&id);
        }
    }

    //
    // Get the active terminal ID (first session if any exist).
    //
    pub fn get_active_terminal_id(&self) -> Option<String> {
        self.sessions.keys().next().cloned()
    }
}
