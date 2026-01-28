use crate::agent_connectors::{Agent, AgentRegistry};
use common::{AgentCommand, AgentCommandResult, NodeCommandResult, ReconResult};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

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

                    let result = agent_clone.perform_recon(false).await;

                    match result {
                        Some(recon_result) => {
                            common::log_info!(
                                "Recon complete: {} MCP servers, {} skills, {} config items",
                                recon_result.tools.mcp_servers.len(),
                                recon_result.tools.skills.len(),
                                recon_result.config.items.len()
                            );
                            NodeCommandResult::Agent(AgentCommandResult::ReconComplete {
                                result: recon_result,
                            })
                        }
                        None => {
                            common::log_warn!("Agent does not support reconnaissance");
                            NodeCommandResult::Agent(AgentCommandResult::ReconComplete {
                                result: ReconResult::default(),
                            })
                        }
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

                    let result = agent_clone.perform_recon(true).await;

                    match result {
                        Some(recon_result) => {
                            common::log_info!(
                                "Semantic recon complete: {} MCP servers, {} skills, {} internal tools, {} config items",
                                recon_result.tools.mcp_servers.len(),
                                recon_result.tools.skills.len(),
                                recon_result.tools.internal_tools.len(),
                                recon_result.config.items.len()
                            );
                            NodeCommandResult::Agent(AgentCommandResult::ReconComplete {
                                result: recon_result,
                            })
                        }
                        None => {
                            common::log_warn!("Agent does not support semantic reconnaissance");
                            NodeCommandResult::Agent(AgentCommandResult::ReconComplete {
                                result: ReconResult::default(),
                            })
                        }
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
            // Validate path is within home directory for security.
            //
            let home_dir = match dirs::home_dir() {
                Some(h) => h,
                None => {
                    return NodeCommandResult::Agent(AgentCommandResult::ConfigFileUpdated {
                        success: false,
                        error: Some("Could not determine home directory".to_string()),
                    });
                }
            };

            let target_path = std::path::Path::new(&path);
            let canonical_path = match target_path.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    //
                    // File might not exist yet, check parent.
                    //
                    match target_path.parent().and_then(|p| p.canonicalize().ok()) {
                        Some(parent) if parent.starts_with(&home_dir) => target_path.to_path_buf(),
                        _ => {
                            return NodeCommandResult::Agent(AgentCommandResult::ConfigFileUpdated {
                                success: false,
                                error: Some("Invalid path or path outside home directory".to_string()),
                            });
                        }
                    }
                }
            };

            if !canonical_path.starts_with(&home_dir) {
                return NodeCommandResult::Agent(AgentCommandResult::ConfigFileUpdated {
                    success: false,
                    error: Some("Path must be within home directory".to_string()),
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
    }
}
