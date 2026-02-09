use crate::agent_connectors::{Agent, AgentRegistry};
use common::{AgentCommand, AgentCommandResult, NodeCommandResult, ReconResult};
use std::path::Path;
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
                    common::log_info!(
                        "Starting recon for agent {}",
                        agent.short_name()
                    );
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
                    common::log_info!(
                        "Starting semantic recon for agent {}",
                        agent.short_name()
                    );
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
        AgentCommand::UpdateConfigFile { path, contents } => {
            //
            // Validate path is within a valid user home directory for security.
            //
            let target_path = Path::new(&path);
            let canonical_path = match target_path.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    //
                    // File might not exist yet, check parent.
                    //
                    match target_path.parent().and_then(|p| p.canonicalize().ok()) {
                        Some(parent) if is_path_in_valid_home(&parent) => target_path.to_path_buf(),
                        _ => {
                            return NodeCommandResult::Agent(AgentCommandResult::ConfigFileUpdated {
                                success: false,
                                error: Some("Invalid path or path outside home directory".to_string()),
                            });
                        }
                    }
                }
            };

            if !is_path_in_valid_home(&canonical_path) {
                return NodeCommandResult::Agent(AgentCommandResult::ConfigFileUpdated {
                    success: false,
                    error: Some("Path must be within a valid user home directory".to_string()),
                });
            }

            //
            // Write the file.
            //
            match std::fs::write(&path, &contents) {
                Ok(_) => {
                    common::log_info!("Updated config file: {}", path);
                    NodeCommandResult::Agent(AgentCommandResult::ConfigFileUpdated {
                        success: true,
                        error: None,
                    })
                }
                Err(e) => {
                    common::log_warn!("Failed to write config file {}: {}", path, e);
                    NodeCommandResult::Agent(AgentCommandResult::ConfigFileUpdated {
                        success: false,
                        error: Some(format!("Failed to write file: {}", e)),
                    })
                }
            }
        }
        AgentCommand::GetSessionContent { session_file } => {
            //
            // Delegate to the selected agent's read_session_content, which
            // handles virtual paths (e.g. SQLite-backed sessions) as well as
            // plain files. Path validation is the agent's responsibility for
            // virtual paths; for real files the default impl reads directly.
            //

            let locked = selected_agent.lock().unwrap();
            let agent = locked.as_ref();

            let content = agent.and_then(|a| a.read_session_content(&session_file));

            match content {
                Some(content) => {
                    let (content, truncated) = truncate_content(content);
                    if truncated {
                        common::log_warn!(
                            "Session file {} truncated to {} bytes",
                            session_file, content.len()
                        );
                    }
                    common::log_info!("Read session file: {}", session_file);
                    NodeCommandResult::Agent(AgentCommandResult::SessionContent {
                        session_file,
                        content: Some(content),
                        error: if truncated {
                            Some("Content truncated due to size limit".to_string())
                        } else {
                            None
                        },
                    })
                }
                None => {
                    common::log_warn!("Failed to read session file: {}", session_file);
                    NodeCommandResult::Agent(AgentCommandResult::SessionContent {
                        session_file,
                        content: None,
                        error: Some("Failed to read session content".to_string()),
                    })
                }
            }
        }
        AgentCommand::GetConfigContent { config_path } => {
            //
            // Validate path is within a valid user home directory for security.
            //
            let target_path = Path::new(&config_path);
            let canonical_path = match target_path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    return NodeCommandResult::Agent(AgentCommandResult::ConfigContent {
                        config_path,
                        content: None,
                        error: Some(format!("Invalid path: {}", e)),
                    });
                }
            };

            if !is_path_in_valid_home(&canonical_path) {
                return NodeCommandResult::Agent(AgentCommandResult::ConfigContent {
                    config_path,
                    content: None,
                    error: Some("Path must be within a valid user home directory".to_string()),
                });
            }

            //
            // Read the config file.
            //
            match std::fs::read_to_string(&config_path) {
                Ok(content) => {
                    let (content, truncated) = truncate_content(content);
                    if truncated {
                        common::log_warn!(
                            "Config file {} truncated to {} bytes",
                            config_path, content.len()
                        );
                    }
                    common::log_info!("Read config file: {}", config_path);
                    NodeCommandResult::Agent(AgentCommandResult::ConfigContent {
                        config_path,
                        content: Some(content),
                        error: if truncated {
                            Some("Content truncated due to size limit".to_string())
                        } else {
                            None
                        },
                    })
                }
                Err(e) => {
                    common::log_warn!("Failed to read config file {}: {}", config_path, e);
                    NodeCommandResult::Agent(AgentCommandResult::ConfigContent {
                        config_path,
                        content: None,
                        error: Some(format!("Failed to read file: {}", e)),
                    })
                }
            }
        }
    }
}
