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
