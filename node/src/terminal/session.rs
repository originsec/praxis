use portable_pty::{MasterPty, PtySize};
use std::io::Write;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

//
// Maximum scrollback buffer size per terminal (128KB).
//
const MAX_SCROLLBACK: usize = 128 * 1024;

pub struct TerminalSession {
    #[allow(dead_code)]
    pub terminal_id: String,
    pub client_id: String,
    pub(super) master: Box<dyn MasterPty + Send>,
    pub(super) writer: Box<dyn Write + Send>,
    pub(super) shutdown_tx: Option<mpsc::Sender<()>>,
    pub(super) scrollback: Arc<Mutex<Vec<u8>>>,
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

    //
    // Append output data to the scrollback buffer.
    //
    pub fn append_scrollback(scrollback: &Arc<Mutex<Vec<u8>>>, data: &[u8]) {
        if let Ok(mut buf) = scrollback.lock() {
            buf.extend_from_slice(data);

            //
            // If buffer exceeds max, trim from the front.
            //
            if buf.len() > MAX_SCROLLBACK {
                let excess = buf.len() - MAX_SCROLLBACK;
                buf.drain(..excess);
            }
        }
    }

    //
    // Get the current scrollback buffer contents.
    //
    pub fn get_scrollback(&self) -> Vec<u8> {
        self.scrollback
            .lock()
            .map(|buf| buf.clone())
            .unwrap_or_default()
    }
}
