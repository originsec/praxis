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
        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(
            io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen,
        )?;
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
    #[test]
    #[ignore = "interactive: real Windows console and Notepad required; run with --nocapture --test-threads=1"]
    fn windows_notepad_roundtrip_manual() {
        use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
        use std::io::IsTerminal;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        use std::time::{Duration, Instant};

        struct ConsoleRestore;
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
        struct ReaderStop {
            stopped: Arc<AtomicBool>,
            worker: Option<std::thread::JoinHandle<()>>,
        }
        impl Drop for ReaderStop {
            fn drop(&mut self) {
                self.stopped.store(true, Ordering::Release);
                if let Some(worker) = self.worker.take() {
                    let _ = worker.join();
                }
            }
        }

        assert!(
            io::stdin().is_terminal() && io::stdout().is_terminal(),
            "Run this test in a Windows console without redirecting stdin/stdout"
        );
        assert!(
            !crossterm::terminal::is_raw_mode_enabled().unwrap(),
            "Start the manual test from a normal console"
        );
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

        let _console = ConsoleRestore;
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
        let _reader = ReaderStop {
            stopped,
            worker: Some(worker),
        };
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
