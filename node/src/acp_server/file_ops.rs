//! File-operation helpers shared across ACP extension handlers.
//!
//! The legacy NodeCommand::Agent(ReadFile/WriteFile/GrepFiles/...) path
//! lived in node/src/handlers/agent_handler.rs. These pure helpers were
//! lifted out verbatim so the ACP extension handlers can reuse them.

use std::path::{Path, PathBuf};

use common::{GrepFileEntry, GrepMatch};
use regex::Regex;

//
// Maximum content size for file reads sent over RabbitMQ. Leave headroom
// below the 16MB default message limit for JSON envelope overhead.
//

pub const MAX_CONTENT_SIZE: usize = 14 * 1024 * 1024;

pub fn truncate_content(content: String) -> (String, bool) {
    if content.len() <= MAX_CONTENT_SIZE {
        return (content, false);
    }

    let end = match content[..MAX_CONTENT_SIZE].rfind('\n') {
        Some(pos) => pos + 1,
        None => {
            let mut end = MAX_CONTENT_SIZE;
            while !content.is_char_boundary(end) {
                end -= 1;
            }
            end
        }
    };

    let mut truncated = content;
    truncated.truncate(end);
    (truncated, true)
}

pub fn is_path_in_valid_home(canonical_path: &Path) -> bool {
    let valid_homes = crate::agent_connectors::utils::enumerate_user_homes();
    valid_homes.iter().any(|home| {
        home.canonicalize()
            .map(|h| canonical_path.starts_with(&h))
            .unwrap_or(false)
    })
}

pub fn canonicalize_and_validate_path(path: &str) -> Result<PathBuf, String> {
    let target_path = Path::new(path);
    let canonical_path = target_path
        .canonicalize()
        .map_err(|e| format!("Invalid path: {}", e))?;

    if !is_path_in_valid_home(&canonical_path) {
        return Err("Path must be within a valid user home directory".to_string());
    }

    Ok(canonical_path)
}

//
// Validate a path that is about to be written, snapshotted or deleted. The
// file itself may not exist yet, in which case its parent directory must
// resolve inside a valid user home.
//

pub fn validate_write_path(path: &str) -> Result<(), String> {
    let target_path = Path::new(path);
    let canonical_path = match target_path.canonicalize() {
        Ok(p) => p,
        Err(_) => match target_path.parent().and_then(|p| p.canonicalize().ok()) {
            Some(parent) if is_path_in_valid_home(&parent) => return Ok(()),
            _ => return Err("Invalid path or path outside home directory".to_string()),
        },
    };
    if !is_path_in_valid_home(&canonical_path) {
        return Err("Path must be within a valid user home directory".to_string());
    }
    Ok(())
}

//
// Largest file the change ledger will snapshot. Config files are tiny; the
// cap keeps before/after content (both stored by the service) well inside
// the RabbitMQ message limit.
//

pub const MAX_SNAPSHOT_SIZE: u64 = 4 * 1024 * 1024;

#[derive(Debug, serde::Serialize, PartialEq)]
pub struct FileSnapshot {
    pub path: String,
    pub exists: bool,
    pub sha256: Option<String>,
    pub content: Option<String>,
    pub error: Option<String>,
}

//
// Read a file in full for the change ledger. A missing file is a valid
// snapshot (exists = false); unreadable, oversized or non-UTF-8 files are
// reported as errors because the ledger could not restore them faithfully.
//

pub fn snapshot_file(path: &str) -> FileSnapshot {
    let err = |e: String| FileSnapshot {
        path: path.to_string(),
        exists: false,
        sha256: None,
        content: None,
        error: Some(e),
    };
    if let Err(e) = validate_write_path(path) {
        return err(e);
    }
    let bytes = match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return FileSnapshot {
                path: path.to_string(),
                exists: false,
                sha256: None,
                content: None,
                error: None,
            };
        }
        Err(e) => return err(format!("Failed to stat file: {}", e)),
        Ok(m) if m.is_dir() => return err("Path is a directory".to_string()),
        Ok(m) if m.len() > MAX_SNAPSHOT_SIZE => {
            return err(format!(
                "File is {} bytes; the change ledger handles files up to {} bytes",
                m.len(),
                MAX_SNAPSHOT_SIZE
            ));
        }
        Ok(_) => match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return err(format!("Failed to read file: {}", e)),
        },
    };
    let sha256 = common::config_change::sha256_hex(&bytes);
    match String::from_utf8(bytes) {
        Ok(content) => FileSnapshot {
            path: path.to_string(),
            exists: true,
            sha256: Some(sha256),
            content: Some(content),
            error: None,
        },
        Err(_) => err("File is not valid UTF-8 text".to_string()),
    }
}

//
// Current SHA-256 of a file, None if it does not exist.
//

pub fn current_sha256(path: &str) -> std::io::Result<Option<String>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(common::config_change::sha256_hex(&b))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn validate_line_range(
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<(), String> {
    if let Some(start) = line_start {
        if start == 0 {
            return Err("line_start must be >= 1".to_string());
        }
    }
    if let Some(end) = line_end {
        if end == 0 {
            return Err("line_end must be >= 1".to_string());
        }
    }
    if let (Some(start), Some(end)) = (line_start, line_end) {
        if end < start {
            return Err("line_end must be >= line_start".to_string());
        }
    }
    Ok(())
}

pub fn read_file_range(
    path: &str,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    if line_start.is_none() && line_end.is_none() {
        return Ok(content);
    }
    Ok(select_line_range(&content, line_start, line_end))
}

pub fn select_line_range(
    content: &str,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> String {
    if line_start.is_none() && line_end.is_none() {
        return content.to_string();
    }

    let start = line_start.unwrap_or(1);
    let end = line_end.unwrap_or(usize::MAX);
    let mut selected = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let line_number = idx + 1;
        if line_number < start {
            continue;
        }
        if line_number > end {
            break;
        }
        selected.push(line);
    }
    selected.join("\n")
}

pub fn grep_content(content: &str, re: &Regex) -> Vec<GrepMatch> {
    let mut matches = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if re.is_match(line) {
            matches.push(GrepMatch {
                line_number: idx + 1,
                line_content: line.to_string(),
            });
        }
    }
    matches
}

pub fn has_glob_chars(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[')
}

pub fn expand_config_paths(paths: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for p in paths {
        if has_glob_chars(p) {
            if let Ok(entries) = glob::glob(p) {
                for entry in entries.flatten() {
                    if let Some(s) = entry.to_str() {
                        if canonicalize_and_validate_path(s).is_ok() {
                            out.push(s.to_string());
                        }
                    }
                }
            }
        } else {
            out.push(p.clone());
        }
    }
    out
}

pub fn grep_single_config_file(path: &str, re: &Regex) -> GrepFileEntry {
    if let Err(error) = canonicalize_and_validate_path(path) {
        return GrepFileEntry {
            path: path.to_string(),
            matches: Vec::new(),
            error: Some(error),
        };
    }
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let matches = grep_content(&content, re);
            GrepFileEntry {
                path: path.to_string(),
                matches,
                error: None,
            }
        }
        Err(e) => GrepFileEntry {
            path: path.to_string(),
            matches: Vec::new(),
            error: Some(format!("Failed to read: {}", e)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    //
    // Paths outside every user home are refused before any read, for both
    // existing and not-yet-existing files.
    //

    #[test]
    fn snapshot_refuses_paths_outside_home() {
        for path in ["/etc/hostname", "/nonexistent-dir/praxis.json"] {
            let snap = snapshot_file(path);
            assert!(!snap.exists);
            assert!(snap.content.is_none());
            assert!(snap.error.is_some(), "{} should be rejected", path);
        }
        assert!(validate_write_path("/etc/praxis-test.json").is_err());
    }

    #[test]
    fn current_sha256_reports_missing_and_existing() {
        let dir = std::env::temp_dir().join(format!("praxis_file_ops_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        let path = file.to_string_lossy().into_owned();
        assert_eq!(current_sha256(&path).unwrap(), None);
        std::fs::write(&file, "abc").unwrap();
        assert_eq!(
            current_sha256(&path).unwrap().as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
