use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use agent_client_protocol as acp;
use acp::schema::{ExtRequest, ExtResponse};
use common::AgentFileType;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::agent_connectors::AgentRegistry;

use super::file_ops;

fn raw_arc<T: Serialize>(v: &T) -> Arc<RawValue> {
    match serde_json::value::to_raw_value(v) {
        Ok(r) => Arc::<RawValue>::from(r),
        Err(_) => Arc::<RawValue>::from(RawValue::from_string("null".to_string()).unwrap()),
    }
}

pub use common::acp_ext::{
    EXT_PRAXIS_RECON,
    EXT_PRAXIS_READ_FILE,
    EXT_PRAXIS_WRITE_FILE,
    EXT_PRAXIS_GREP_FILES,
    EXT_PRAXIS_WRITE_SESSION_CONTENT,
};

#[derive(Debug, Deserialize)]
struct ReconParams {
    agent_short_name: String,
    #[serde(default)]
    is_semantic: bool,
}

#[derive(Debug, Deserialize)]
struct ReadFileParams {
    agent_short_name: String,
    file_type: AgentFileType,
    path: String,
    #[serde(default)]
    line_start: Option<usize>,
    #[serde(default)]
    line_end: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ReadFileResult {
    file_type: AgentFileType,
    path: String,
    content: Option<String>,
    line_start: Option<usize>,
    line_end: Option<usize>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WriteFileParams {
    file_type: AgentFileType,
    path: String,
    contents: String,
}

#[derive(Debug, Serialize)]
struct WriteFileResult {
    file_type: AgentFileType,
    path: String,
    success: bool,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GrepFilesParams {
    agent_short_name: String,
    file_type: AgentFileType,
    paths: Vec<String>,
    pattern: String,
}

#[derive(Debug, Serialize)]
struct GrepFilesResult {
    file_type: AgentFileType,
    pattern: String,
    results: Vec<common::GrepFileEntry>,
    errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WriteSessionContentParams {
    agent_short_name: String,
    path: String,
    contents: String,
}

#[derive(Debug, Serialize)]
struct WriteSessionContentResult {
    path: String,
    success: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ExtError {
    error: String,
}

//
// Dispatch an ExtRequest to the appropriate extension handler. Returns Ok
// with an ExtResponse (carrying either a result or an error object in the
// body) if the method is recognized, or Err(acp::Error) for unknown methods
// so the caller can emit a standard -32601.
//

pub async fn dispatch(
    registry: &Arc<RwLock<AgentRegistry>>,
    req: ExtRequest,
) -> Result<ExtResponse, acp::Error> {
    match req.method.as_ref() {
        EXT_PRAXIS_RECON => handle_recon(registry, req).await,
        EXT_PRAXIS_READ_FILE => handle_read_file(registry, req).await,
        EXT_PRAXIS_WRITE_FILE => handle_write_file(req).await,
        EXT_PRAXIS_GREP_FILES => handle_grep_files(registry, req).await,
        EXT_PRAXIS_WRITE_SESSION_CONTENT => handle_write_session_content(registry, req).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

fn parse_params<T: for<'de> Deserialize<'de>>(req: &ExtRequest) -> Result<T, acp::Error> {
    serde_json::from_str(req.params.get())
        .map_err(|e| acp::Error::invalid_params().data(serde_json::json!(e.to_string())))
}

fn ext_ok<T: Serialize>(v: &T) -> ExtResponse {
    ExtResponse::new(raw_arc(v))
}

fn ext_err(msg: impl Into<String>) -> ExtResponse {
    ExtResponse::new(raw_arc(&ExtError { error: msg.into() }))
}

async fn handle_recon(
    registry: &Arc<RwLock<AgentRegistry>>,
    req: ExtRequest,
) -> Result<ExtResponse, acp::Error> {
    let params: ReconParams = parse_params(&req)?;

    let agent = {
        let reg = registry.read().await;
        reg.find_by_short_name(&params.agent_short_name)
    };

    let Some(agent) = agent else {
        return Ok(ext_err(format!(
            "Unknown agent '{}'",
            params.agent_short_name
        )));
    };

    let Some(recon) = agent.as_recon() else {
        return Ok(ext_err(format!(
            "Agent '{}' does not support recon",
            params.agent_short_name
        )));
    };

    match recon.perform_recon(params.is_semantic).await {
        Some(r) => Ok(ext_ok(&r)),
        None => Ok(ext_err("Recon produced no result (VM busy or failed)")),
    }
}

async fn handle_read_file(
    registry: &Arc<RwLock<AgentRegistry>>,
    req: ExtRequest,
) -> Result<ExtResponse, acp::Error> {
    let params: ReadFileParams = parse_params(&req)?;

    if let Err(error) = file_ops::validate_line_range(params.line_start, params.line_end) {
        return Ok(ext_ok(&ReadFileResult {
            file_type: params.file_type,
            path: params.path,
            content: None,
            line_start: params.line_start,
            line_end: params.line_end,
            error: Some(error),
        }));
    }

    let content_result = match params.file_type {
        AgentFileType::Config => {
            if let Err(error) = file_ops::canonicalize_and_validate_path(&params.path) {
                return Ok(ext_ok(&ReadFileResult {
                    file_type: params.file_type,
                    path: params.path,
                    content: None,
                    line_start: params.line_start,
                    line_end: params.line_end,
                    error: Some(error),
                }));
            }
            file_ops::read_file_range(&params.path, params.line_start, params.line_end)
                .map_err(|e| format!("Failed to read config content: {}", e))
        }
        AgentFileType::Session => {
            let agent = {
                let reg = registry.read().await;
                reg.find_by_short_name(&params.agent_short_name)
            };
            match agent.and_then(|a| a.read_session_content(&params.path)) {
                Some(content) => Ok(file_ops::select_line_range(
                    &content,
                    params.line_start,
                    params.line_end,
                )),
                None => Err("Failed to read session content".to_string()),
            }
        }
    };

    let result = match content_result {
        Ok(content) => {
            let (content, truncated) = file_ops::truncate_content(content);
            ReadFileResult {
                file_type: params.file_type,
                path: params.path,
                content: Some(content),
                line_start: params.line_start,
                line_end: params.line_end,
                error: if truncated {
                    Some("Content truncated due to size limit".to_string())
                } else {
                    None
                },
            }
        }
        Err(e) => ReadFileResult {
            file_type: params.file_type,
            path: params.path,
            content: None,
            line_start: params.line_start,
            line_end: params.line_end,
            error: Some(e),
        },
    };
    Ok(ext_ok(&result))
}

async fn handle_write_file(req: ExtRequest) -> Result<ExtResponse, acp::Error> {
    let params: WriteFileParams = parse_params(&req)?;

    if matches!(params.file_type, AgentFileType::Session) {
        return Ok(ext_ok(&WriteFileResult {
            file_type: params.file_type,
            path: params.path,
            success: false,
            error: Some("Write is not allowed for session content".to_string()),
        }));
    }

    let target_path = Path::new(&params.path);
    let canonical_path = match target_path.canonicalize() {
        Ok(p) => p,
        Err(_) => match target_path.parent().and_then(|p| p.canonicalize().ok()) {
            Some(parent) if file_ops::is_path_in_valid_home(&parent) => target_path.to_path_buf(),
            _ => {
                return Ok(ext_ok(&WriteFileResult {
                    file_type: params.file_type,
                    path: params.path,
                    success: false,
                    error: Some("Invalid path or path outside home directory".to_string()),
                }));
            }
        },
    };

    if !file_ops::is_path_in_valid_home(&canonical_path) {
        return Ok(ext_ok(&WriteFileResult {
            file_type: params.file_type,
            path: params.path,
            success: false,
            error: Some("Path must be within a valid user home directory".to_string()),
        }));
    }

    let result = match std::fs::write(&params.path, &params.contents) {
        Ok(_) => WriteFileResult {
            file_type: params.file_type,
            path: params.path,
            success: true,
            error: None,
        },
        Err(e) => WriteFileResult {
            file_type: params.file_type,
            path: params.path,
            success: false,
            error: Some(format!("Failed to write file: {}", e)),
        },
    };
    Ok(ext_ok(&result))
}

async fn handle_grep_files(
    registry: &Arc<RwLock<AgentRegistry>>,
    req: ExtRequest,
) -> Result<ExtResponse, acp::Error> {
    let params: GrepFilesParams = parse_params(&req)?;

    let re = match Regex::new(&params.pattern) {
        Ok(re) => re,
        Err(e) => {
            return Ok(ext_ok(&GrepFilesResult {
                file_type: params.file_type,
                pattern: params.pattern,
                results: Vec::new(),
                errors: vec![format!("Invalid regex pattern: {}", e)],
            }));
        }
    };

    let mut results = Vec::new();
    let mut errors = Vec::new();

    match params.file_type {
        AgentFileType::Config => {
            let expanded = file_ops::expand_config_paths(&params.paths);
            for path in expanded {
                results.push(file_ops::grep_single_config_file(&path, &re));
            }
            for p in &params.paths {
                if file_ops::has_glob_chars(p) {
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
            let agent = {
                let reg = registry.read().await;
                reg.find_by_short_name(&params.agent_short_name)
            };
            for path in &params.paths {
                match agent
                    .as_ref()
                    .and_then(|a| a.read_session_content(path))
                {
                    Some(content) => {
                        let matches = file_ops::grep_content(&content, &re);
                        results.push(common::GrepFileEntry {
                            path: path.clone(),
                            matches,
                            error: None,
                        });
                    }
                    None => {
                        results.push(common::GrepFileEntry {
                            path: path.clone(),
                            matches: Vec::new(),
                            error: Some("Failed to read session content".to_string()),
                        });
                    }
                }
            }
        }
    }

    Ok(ext_ok(&GrepFilesResult {
        file_type: params.file_type,
        pattern: params.pattern,
        results,
        errors,
    }))
}

async fn handle_write_session_content(
    registry: &Arc<RwLock<AgentRegistry>>,
    req: ExtRequest,
) -> Result<ExtResponse, acp::Error> {
    let params: WriteSessionContentParams = parse_params(&req)?;

    let agent = {
        let reg = registry.read().await;
        reg.find_by_short_name(&params.agent_short_name)
    };

    let result = match agent {
        Some(a) => a.write_session_content(&params.path, &params.contents),
        None => Err(anyhow::anyhow!(
            "Agent '{}' not found",
            params.agent_short_name
        )),
    };

    let response = match result {
        Ok(()) => WriteSessionContentResult {
            path: params.path,
            success: true,
            error: None,
        },
        Err(e) => WriteSessionContentResult {
            path: params.path,
            success: false,
            error: Some(e.to_string()),
        },
    };
    Ok(ext_ok(&response))
}
