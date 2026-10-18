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
