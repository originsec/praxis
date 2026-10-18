use std::io;
use std::path::Path;
use std::process::{Command, ExitStatus};

struct TerminalHandoff {
    _pause: crate::event::TerminalPause,
}

impl TerminalHandoff {
    fn acquire(input: &crate::event::TerminalInput) -> io::Result<Self> {
        let handoff = Self {
            _pause: input.pause()?,
        };

        //
        // On Windows, disabling mouse capture restores the console mode
        // saved while the TUI was raw. Switch to cooked input afterward so
        // the editor and its completion prompt receive normal Enter/Ctrl+C.
        //

        crossterm::execute!(
            io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen,
        )?;
        crossterm::terminal::disable_raw_mode()?;
        Ok(handoff)
    }
}

impl Drop for TerminalHandoff {
    fn drop(&mut self) {
        let _ = crossterm::execute!(
            io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
        );
        let _ = crossterm::terminal::enable_raw_mode();

        //
        // The reader stays paused while we discard buffered editor keys.
        // Bound the drain so continuous input cannot delay resuming the UI.
        //
        for _ in 0..256 {
            if !crossterm::event::poll(std::time::Duration::ZERO).unwrap_or(false) {
                break;
            }
            if crossterm::event::read().is_err() {
                break;
            }
        }
    }
}

pub(crate) fn run(
    editor: &str,
    path: &Path,
    input: &crate::event::TerminalInput,
) -> io::Result<ExitStatus> {
    let _handoff = TerminalHandoff::acquire(input)?;
    let status = Command::new(editor).arg(path).status()?;

    //
    // Windows editors can hand the file to another process and exit before
    // the document closes. Keep the temporary path alive until the user
    // confirms completion, rather than reading and deleting it on handoff.
    //

    #[cfg(windows)]
    if status.success() {
        use std::io::Write;

        println!("Save the file in your editor, then close it.");
        print!("Press Enter here to apply the saved changes (Ctrl+C to quit): ");
        io::stdout().flush()?;
        confirm_completion(&mut io::stdin().lock())?;
    }

    Ok(status)
}

#[cfg(any(windows, test))]
fn confirm_completion(input: &mut impl io::BufRead) -> io::Result<()> {
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "editor completion was not confirmed",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    #[cfg(windows)]
    struct ConsoleRestore;

    #[cfg(windows)]
    impl Drop for ConsoleRestore {
        fn drop(&mut self) {
            let _ = crossterm::execute!(
                io::stdout(),
                crossterm::event::DisableMouseCapture,
                crossterm::terminal::LeaveAlternateScreen,
            );
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }

    #[cfg(windows)]
    struct ReaderStop {
        stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }

    #[cfg(windows)]
    impl Drop for ReaderStop {
        fn drop(&mut self) {
            self.stopped
                .store(true, std::sync::atomic::Ordering::Release);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    #[cfg(windows)]
    fn windows_console() -> (
        ConsoleRestore,
        ReaderStop,
        crate::event::TerminalInput,
        std::sync::mpsc::Receiver<crossterm::event::Event>,
    ) {
        use std::io::IsTerminal;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };

        assert!(
            io::stdin().is_terminal() && io::stdout().is_terminal(),
            "Run this test in a Windows console without redirecting stdin/stdout"
        );
        assert!(
            !crossterm::terminal::is_raw_mode_enabled().unwrap(),
            "Start the console test from a normal console"
        );
        let console = ConsoleRestore;
        crossterm::terminal::enable_raw_mode().unwrap();
        crossterm::execute!(
            io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )
        .unwrap();
        let (forward, events) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let reader_stopped = stopped.clone();
        let (input, worker) = crate::event::TerminalInput::start(
            move |event| forward.send(event).is_ok(),
            move || reader_stopped.load(Ordering::Acquire),
        );
        let reader = ReaderStop {
            stopped,
            worker: Some(worker),
        };
        (console, reader, input, events)
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a Windows console; no interaction needed; run with --nocapture --test-threads=1"]
    fn windows_console_handoff_restores_cooked_input() {
        let (_console, _reader, input, _events) = windows_console();
        assert!(crossterm::terminal::is_raw_mode_enabled().unwrap());
        for _ in 0..2 {
            let handoff = TerminalHandoff::acquire(&input).unwrap();
            assert!(
                !crossterm::terminal::is_raw_mode_enabled().unwrap(),
                "Editor input must be cooked after mouse capture is disabled"
            );
            drop(handoff);
            assert!(
                crossterm::terminal::is_raw_mode_enabled().unwrap(),
                "TUI input must be raw after the editor returns"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "interactive: real Windows console and Notepad required; run with --nocapture --test-threads=1"]
    fn windows_notepad_roundtrip_manual() {
        use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
        use std::time::{Duration, Instant};

        let mut file = tempfile::Builder::new()
            .prefix("praxis_notepad_roundtrip_")
            .suffix(".txt")
            .tempfile()
            .unwrap();
        file.write_all(b"original").unwrap();
        let path = file.into_temp_path();
        io::stdout().write_all(
            b"Notepad roundtrip test:\r\nReplace original with praxis-editor-ok, save, and close Notepad.\r\nThen press Enter ONCE at the terminal confirmation prompt.\r\nFinally press r to verify the TUI reader resumed. Ctrl+C cancels.\r\n"
        ).unwrap();
        io::stdout().flush().unwrap();

        let (_console, _reader, input, events) = windows_console();
        let status = run("notepad.exe", &path, &input).unwrap();
        assert!(status.success(), "Notepad exited with {status}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap().trim(),
            "praxis-editor-ok"
        );

        io::stdout().write_all(b"Saved marker verified. Press r now to confirm terminal input resumed (Esc cancels).\r\n").unwrap();
        io::stdout().flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "Timed out waiting for resumed terminal input"
            );
            let event = events
                .recv_timeout(remaining)
                .expect("No resumed terminal input within 120 seconds");
            if let Event::Key(key) = event {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                assert!(
                    key.code != KeyCode::Esc
                        && !(key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)),
                    "Manual test cancelled"
                );
                if key.code == KeyCode::Char('r') {
                    break;
                }
            }
        }
    }

    #[test]
    fn closed_temp_file_can_be_replaced_before_readback() {
        let mut file = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        file.write_all(b"original").unwrap();
        let path = file.into_temp_path();

        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let mut editor = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .share_mode(0)
                .open(&path)
                .unwrap();
            editor.write_all(b"exclusive editor save").unwrap();
        }

        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "updated").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "updated");
        let saved_path = path.to_path_buf();
        drop(path);
        assert!(!saved_path.exists());
    }

    #[test]
    fn completion_requires_input_and_accepts_windows_enter() {
        assert!(confirm_completion(&mut Cursor::new(b"\r\n")).is_ok());
        assert_eq!(
            confirm_completion(&mut Cursor::new(b""))
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof,
        );
    }
}
