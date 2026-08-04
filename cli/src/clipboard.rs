use std::io::Write;
use std::process::{Command, Stdio};

pub fn copy(text: &str) -> bool {
    #[cfg(windows)]
    {
        let mut child = match Command::new("clip").stdin(Stdio::piped()).spawn() {
            Ok(child) => child,
            Err(_) => return false,
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        child.wait().map(|status| status.success()).unwrap_or(false)
    }

    #[cfg(not(windows))]
    {
        let candidates: &[(&str, &[&str])] = &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
            ("pbcopy", &[]),
        ];
        for (bin, args) in candidates {
            let mut command = Command::new(*bin);
            command.args(*args);
            let mut child = match command.stdin(Stdio::piped()).spawn() {
                Ok(child) => child,
                Err(_) => continue,
            };
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().map(|status| status.success()).unwrap_or(false) {
                return true;
            }
        }
        false
    }
}
