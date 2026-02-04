use portable_pty::{MasterPty, PtySize};
use std::io::Write;
use tokio::sync::mpsc;

pub struct TerminalSession {
    #[allow(dead_code)]
    pub terminal_id: String,
    pub client_id: String,
    pub(super) master: Box<dyn MasterPty + Send>,
    pub(super) writer: Box<dyn Write + Send>,
    pub(super) shutdown_tx: Option<mpsc::Sender<()>>,
}

impl TerminalSession {
    pub fn write_data(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, rows: u16, cols: u16) -> anyhow::Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn close(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.try_send(());
        }
    }
}
