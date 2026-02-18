use colored::Colorize;
use std::io::Write;

pub struct Spinner {
    stop: tokio::sync::watch::Sender<bool>,
    handle: tokio::task::JoinHandle<()>,
}

impl Spinner {
    pub fn start(message: &str) -> Self {
        Self::spawn(message, false)
    }

    pub fn start_with_elapsed(message: &str) -> Self {
        Self::spawn(message, true)
    }

    pub fn start_cursor() -> Self {
        let (tx, mut rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(async move {
            let mut visible = true;

            loop {
                if *rx.borrow() {
                    break;
                }

                if visible {
                    print!("\r  {}", "\u{258D}".dimmed());
                } else {
                    print!("\r  \x1B[2K");
                }
                let _ = std::io::stdout().flush();
                visible = !visible;

                tokio::select! {
                    _ = rx.changed() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(530)) => {}
                }
            }

            print!("\r\x1B[2K");
            let _ = std::io::stdout().flush();
        });

        Self { stop: tx, handle }
    }

    fn spawn(message: &str, show_elapsed: bool) -> Self {
        let (tx, mut rx) = tokio::sync::watch::channel(false);
        let msg = message.to_string();

        let handle = tokio::spawn(async move {
            const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut i = 0;
            let start = std::time::Instant::now();

            loop {
                if *rx.borrow() {
                    break;
                }

                let frame = FRAMES[i % FRAMES.len()].dimmed();
                if show_elapsed {
                    let secs = start.elapsed().as_secs();
                    let elapsed = format!("[{}s]", secs).dimmed();
                    print!("\r  {} {} {}", frame, msg.dimmed(), elapsed);
                } else {
                    print!("\r  {} {}", frame, msg.dimmed());
                }
                let _ = std::io::stdout().flush();
                i += 1;

                tokio::select! {
                    _ = rx.changed() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(80)) => {}
                }
            }

            print!("\r\x1B[2K");
            let _ = std::io::stdout().flush();
        });

        Self { stop: tx, handle }
    }

    pub async fn finish(self) {
        let _ = self.stop.send(true);
        let _ = self.handle.await;
    }
}
