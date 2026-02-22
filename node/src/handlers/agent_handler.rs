use crate::agent_connectors::{Agent, AgentRegistry};
use anyhow::anyhow;
use common::{AgentCommand, AgentCommandResult, AgentFileType, GrepFileEntry, GrepMatch, NodeCommandResult, ReconResult};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

//
// Maximum content size for file reads sent over RabbitMQ. Leave headroom
// below the 16MB default message limit for JSON envelope overhead.
//

const MAX_CONTENT_SIZE: usize = 14 * 1024 * 1024;

fn truncate_content(content: String) -> (String, bool) {
    if content.len() <= MAX_CONTENT_SIZE {
        return (content, false);
    }

    //
    // Truncate at the last newline before the limit to avoid splitting
    // lines (important for JSONL session files).
    //

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

//
// Check if a path is within any valid user home directory. Uses the same
// enumeration logic as recon to ensure consistency.
//
fn is_path_in_valid_home(canonical_path: &Path) -> bool {
    let valid_homes = crate::agent_connectors::utils::enumerate_user_homes();
    valid_homes.iter().any(|home| {
        home.canonicalize()
            .map(|h| canonical_path.starts_with(&h))
            .unwrap_or(false)
    })
}

fn canonicalize_and_validate_path(path: &str) -> Result<PathBuf, String> {
    let target_path = Path::new(path);
    let canonical_path = target_path
        .canonicalize()
        .map_err(|e| format!("Invalid path: {}", e))?;

    if !is_path_in_valid_home(&canonical_path) {
        return Err("Path must be within a valid user home directory".to_string());
    }

    Ok(canonical_path)
}

fn validate_line_range(line_start: Option<usize>, line_end: Option<usize>) -> Result<(), String> {
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

fn read_file_range(
    path: &str,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(path)?;

    if line_start.is_none() && line_end.is_none() {
        return Ok(content);
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

    Ok(selected.join("\n"))
}

fn select_line_range(content: &str, line_start: Option<usize>, line_end: Option<usize>) -> String {
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

fn grep_content(content: &str, re: &Regex) -> Vec<GrepMatch> {
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

fn has_glob_chars(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[')
}

//
// Expand a list of paths that may contain glob patterns into concrete file
// paths. Non-glob paths are passed through as-is. Each expanded path is
// validated with canonicalize_and_validate_path.
//

fn expand_config_paths(paths: &[String]) -> Vec<String> {
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

fn grep_single_config_file(path: &str, re: &Regex) -> GrepFileEntry {
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

pub async fn handle_agent_command(
    cmd: AgentCommand,
    registry: &Arc<RwLock<AgentRegistry>>,
    selected_agent: &Arc<Mutex<Option<Arc<dyn Agent>>>>,
) -> NodeCommandResult {
    match cmd {
        AgentCommand::Update => {
            //
            // Just acknowledge - the actual update is sent periodically.
            //
            NodeCommandResult::Agent(AgentCommandResult::UpdateSent)
        }
        AgentCommand::Recon => {
            //
            // Perform reconnaissance on the selected agent (static discovery).
            //
            let locked = selected_agent.lock().unwrap();
            match locked.as_ref() {
                Some(agent) => {
                    common::log_info!("Starting recon for agent {}", agent.short_name());
                    let agent_clone = agent.clone();
                    drop(locked);

                    if let Some(recon) = agent_clone.as_recon() {
                        let result = recon.perform_recon(false).await;

                        match result {
                            Some(recon_result) => {
                                common::log_info!(
                                    "Recon complete: {} MCP servers, {} skills, {} config items",
                                    recon_result.tools.mcp_servers.len(),
                                    recon_result.tools.skills.len(),
                                    recon_result.config.len()
                                );
                                NodeCommandResult::Agent(AgentCommandResult::ReconComplete {
                                    result: recon_result,
                                })
                            }
                            None => {
                                common::log_warn!("Reconnaissance returned no results");
                                NodeCommandResult::Agent(AgentCommandResult::ReconComplete {
                                    result: ReconResult::default(),
                                })
                            }
                        }
                    } else {
                        common::log_warn!("Agent does not support reconnaissance");
                        NodeCommandResult::Agent(AgentCommandResult::ReconComplete {
                            result: ReconResult::default(),
                        })
                    }
                }
                None => NodeCommandResult::Error {
                    message: "No agent selected for recon".to_string(),
                },
            }
        }
        AgentCommand::ReconSemantic => {
            //
            // Perform semantic reconnaissance on the selected agent (includes
            // internal tools).
            //
            let locked = selected_agent.lock().unwrap();
            match locked.as_ref() {
                Some(agent) => {
                    common::log_info!("Starting semantic recon for agent {}", agent.short_name());
                    let agent_clone = agent.clone();
                    drop(locked);

                    if let Some(recon) = agent_clone.as_recon() {
                        let result = recon.perform_recon(true).await;

                        match result {
                            Some(recon_result) => {
                                common::log_info!(
                                    "Semantic recon complete: {} MCP servers, {} skills, {} internal tools, {} config items",
                                    recon_result.tools.mcp_servers.len(),
                                    recon_result.tools.skills.len(),
                                    recon_result.tools.internal_tools.len(),
                                    recon_result.config.len()
                                );
                                NodeCommandResult::Agent(AgentCommandResult::ReconComplete {
                                    result: recon_result,
                                })
                            }
                            None => {
                                common::log_warn!("Semantic reconnaissance returned no results");
                                NodeCommandResult::Agent(AgentCommandResult::ReconComplete {
                                    result: ReconResult::default(),
                                })
                            }
                        }
                    } else {
                        common::log_warn!("Agent does not support semantic reconnaissance");
                        NodeCommandResult::Agent(AgentCommandResult::ReconComplete {
                            result: ReconResult::default(),
                        })
                    }
                }
                None => NodeCommandResult::Error {
                    message: "No agent selected for semantic recon".to_string(),
                },
            }
        }
        AgentCommand::Select { short_name } => {
            //
            // Check if the requested agent is already selected - if so, just
            // return success.
            //
            {
                let locked = selected_agent.lock().unwrap();
                if let Some(current) = locked.as_ref() {
                    if current.short_name() == short_name {
                        return NodeCommandResult::Agent(AgentCommandResult::Selected {
                            short_name,
                        });
                    }
                }
            }

            let agents = registry.read().await.get_all();
            let agent = agents.iter().find(|a| a.short_name() == short_name);

            match agent {
                Some(agent) => {
                    //
                    // Check if agent is available.
                    //
                    if !agent.do_fingerprint().await {
                        return NodeCommandResult::Error {
                            message: format!("Agent '{}' is not available", short_name),
                        };
                    }

                    //
                    // Close any existing session on the previously selected
                    // agent.
                    //
                    {
                        let mut locked = selected_agent.lock().unwrap();
                        if let Some(prev_agent) = locked.as_ref() {
                            prev_agent.close_session();
                        }
                        *locked = Some(agent.clone());
                    }

                    common::log_info!("Selected agent: {}", short_name);
                    NodeCommandResult::Agent(AgentCommandResult::Selected { short_name })
                }
                None => NodeCommandResult::Error {
                    message: format!("Agent '{}' not found", short_name),
                },
            }
        }
        AgentCommand::ReadFile {
            file_type,
            path,
            line_start,
            line_end,
        } => {
            if let Err(error) = validate_line_range(line_start, line_end) {
                return NodeCommandResult::Agent(AgentCommandResult::ReadFileResult {
                    file_type,
                    path,
                    content: None,
                    line_start,
                    line_end,
                    error: Some(error),
                });
            }

            let content_result = match file_type {
                AgentFileType::Config => {
                    if let Err(error) = canonicalize_and_validate_path(&path) {
                        return NodeCommandResult::Agent(AgentCommandResult::ReadFileResult {
                            file_type,
                            path,
                            content: None,
                            line_start,
                            line_end,
                            error: Some(error),
                        });
                    }
                    read_file_range(&path, line_start, line_end).map_err(|e| format!("Failed to read config content: {}. Make sure the relevant agent is selected and the path is correct.", e))
                }
                AgentFileType::Session => {
                    let locked = selected_agent.lock().unwrap();
                    let agent = locked.as_ref();
                    match agent.and_then(|a| a.read_session_content(&path)) {
                        Some(content) => Ok(select_line_range(&content, line_start, line_end)),
                        None => Err("Failed to read session content. Make sure the relevant agent is selected and the path is correct.".to_string()),
                    }
                }
            };

            match content_result {
                Ok(content) => {
                    let (content, truncated) = truncate_content(content);
                    if truncated {
                        common::log_warn!("File {} truncated to {} bytes", path, content.len());
                    }
                    common::log_info!(
                        "Read file: {} (line_start={:?}, line_end={:?})",
                        path,
                        line_start,
                        line_end
                    );
                    NodeCommandResult::Agent(AgentCommandResult::ReadFileResult {
                        file_type,
                        path,
                        content: Some(content),
                        line_start,
                        line_end,
                        error: if truncated {
                            Some("Content truncated due to size limit".to_string())
                        } else {
                            None
                        },
                    })
                }
                Err(e) => {
                    common::log_warn!("Failed to read file {}: {}", path, e);
                    NodeCommandResult::Agent(AgentCommandResult::ReadFileResult {
                        file_type,
                        path,
                        content: None,
                        line_start,
                        line_end,
                        error: Some(e),
                    })
                }
            }
        }
        AgentCommand::WriteFile {
            file_type,
            path,
            contents,
        } => {
            if matches!(file_type, AgentFileType::Session) {
                return NodeCommandResult::Agent(AgentCommandResult::WriteFileResult {
                    file_type,
                    path,
                    success: false,
                    error: Some("Write is not allowed for session content".to_string()),
                });
            }

            let target_path = Path::new(&path);
            let canonical_path = match target_path.canonicalize() {
                Ok(p) => p,
                Err(_) => match target_path.parent().and_then(|p| p.canonicalize().ok()) {
                    Some(parent) if is_path_in_valid_home(&parent) => target_path.to_path_buf(),
                    _ => {
                        return NodeCommandResult::Agent(AgentCommandResult::WriteFileResult {
                            file_type,
                            path,
                            success: false,
                            error: Some("Invalid path or path outside home directory".to_string()),
                        });
                    }
                },
            };

            if !is_path_in_valid_home(&canonical_path) {
                return NodeCommandResult::Agent(AgentCommandResult::WriteFileResult {
                    file_type,
                    path,
                    success: false,
                    error: Some("Path must be within a valid user home directory".to_string()),
                });
            }

            match std::fs::write(&path, &contents) {
                Ok(_) => {
                    common::log_info!("Updated config file: {}", path);
                    NodeCommandResult::Agent(AgentCommandResult::WriteFileResult {
                        file_type,
                        path,
                        success: true,
                        error: None,
                    })
                }
                Err(e) => {
                    common::log_warn!("Failed to write config file {}: {}", path, e);
                    NodeCommandResult::Agent(AgentCommandResult::WriteFileResult {
                        file_type,
                        path,
                        success: false,
                        error: Some(format!("Failed to write file: {}", e)),
                    })
                }
            }
        }
        AgentCommand::GrepFiles {
            file_type,
            paths,
            pattern,
        } => {
            let re = match Regex::new(&pattern) {
                Ok(re) => re,
                Err(e) => {
                    return NodeCommandResult::Agent(AgentCommandResult::GrepFilesResult {
                        file_type,
                        pattern,
                        results: Vec::new(),
                        errors: vec![format!("Invalid regex pattern: {}", e)],
                    });
                }
            };

            let mut results = Vec::new();
            let mut errors = Vec::new();

            match file_type {
                AgentFileType::Config => {
                    let expanded = expand_config_paths(&paths);
                    for path in expanded {
                        results.push(grep_single_config_file(&path, &re));
                    }
                    // Report glob patterns that matched nothing
                    for p in &paths {
                        if has_glob_chars(p) {
                            let expanded: Vec<_> = glob::glob(p)
                                .map(|iter| iter.filter_map(|r| r.ok()).collect())
                                .unwrap_or_default();
                            if expanded.is_empty() {
                                errors.push(format!("Glob pattern '{}' matched no files", p));
                            }
                        }
                    }
                }
                AgentFileType::Session => {
                    let locked = selected_agent.lock().unwrap();
                    let agent = locked.as_ref();
                    for path in &paths {
                        match agent.and_then(|a| a.read_session_content(path)) {
                            Some(content) => {
                                let matches = grep_content(&content, &re);
                                results.push(GrepFileEntry {
                                    path: path.clone(),
                                    matches,
                                    error: None,
                                });
                            }
                            None => {
                                results.push(GrepFileEntry {
                                    path: path.clone(),
                                    matches: Vec::new(),
                                    error: Some("Failed to read session content".to_string()),
                                });
                            }
                        }
                    }
                }
            }

            let total_matches: usize = results.iter().map(|r| r.matches.len()).sum();
            common::log_info!(
                "Grep {} files: pattern='{}' total_matches={}",
                results.len(),
                pattern,
                total_matches
            );
            NodeCommandResult::Agent(AgentCommandResult::GrepFilesResult {
                file_type,
                pattern,
                results,
                errors,
            })
        }
        AgentCommand::WriteSessionContent { path, contents } => {
            let locked = selected_agent.lock().unwrap();
            let agent = locked.as_ref();
            let result = match agent {
                Some(a) => a.write_session_content(&path, &contents),
                None => Err(anyhow!("No selected agent")),
            };

            match result {
                Ok(()) => {
                    common::log_info!("Wrote session content: {}", path);
                    NodeCommandResult::Agent(AgentCommandResult::WriteSessionContentResult {
                        path,
                        success: true,
                        error: None,
                    })
                }
                Err(e) => {
                    common::log_warn!("Failed to write session content {}: {}", path, e);
                    NodeCommandResult::Agent(AgentCommandResult::WriteSessionContentResult {
                        path,
                        success: false,
                        error: Some(e.to_string()),
                    })
                }
            }
        }
    }
}
