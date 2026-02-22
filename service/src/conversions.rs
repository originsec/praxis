//! Chain element type conversions between database and messaging formats.

use crate::database;

/// Convert database chain element to messaging chain element
pub fn to_common(e: database::ChainElement) -> common::ChainElement {
    match e {
        database::ChainElement::Trigger { id, trigger_type } => {
            common::ChainElement::Trigger {
                id,
                trigger_type: match trigger_type {
                    database::TriggerType::Manual => common::ChainTriggerType::Manual,
                },
            }
        }
        database::ChainElement::Operation { id, operation_name, model_ref, session_group, block_config } => {
            common::ChainElement::Operation {
                id,
                operation_name,
                model_ref,
                session_group: session_group.map(|sg| common::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                    working_dir: sg.working_dir,
                }),
                block_config: block_config.map(|bc| common::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
        database::ChainElement::Transform { id, prompt, model_ref, session_group, block_config } => {
            common::ChainElement::Transform {
                id,
                prompt,
                model_ref,
                session_group: session_group.map(|sg| common::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                    working_dir: sg.working_dir,
                }),
                block_config: block_config.map(|bc| common::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
        database::ChainElement::GenericPrompt { id, prompt, session_group, block_config } => {
            common::ChainElement::GenericPrompt {
                id,
                prompt,
                session_group: session_group.map(|sg| common::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                    working_dir: sg.working_dir,
                }),
                block_config: block_config.map(|bc| common::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
        database::ChainElement::Memory { id, key, mode } => {
            common::ChainElement::Memory {
                id,
                key,
                mode: match mode {
                    database::MemoryMode::Store => common::MemoryMode::Store,
                    database::MemoryMode::Retrieve => common::MemoryMode::Retrieve,
                },
            }
        }
        database::ChainElement::Loop { id, max_iterations } => {
            common::ChainElement::Loop { id, max_iterations }
        }
        database::ChainElement::Tool { id, tool_name, tool_params, block_config } => {
            common::ChainElement::Tool {
                id,
                tool_name,
                tool_params,
                block_config: block_config.map(|bc| common::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
        database::ChainElement::Payload { id, payload_id, block_config } => {
            common::ChainElement::Payload {
                id,
                payload_id,
                block_config: block_config.map(|bc| common::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
        database::ChainElement::Termination { id, block_config } => {
            common::ChainElement::Termination {
                id,
                block_config: block_config.map(|bc| common::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
    }
}

/// Convert messaging chain element to database chain element
pub fn to_database(e: common::ChainElement) -> database::ChainElement {
    match e {
        common::ChainElement::Trigger { id, trigger_type } => {
            database::ChainElement::Trigger {
                id,
                trigger_type: match trigger_type {
                    common::ChainTriggerType::Manual => database::TriggerType::Manual,
                },
            }
        }
        common::ChainElement::Operation { id, operation_name, model_ref, session_group, block_config } => {
            database::ChainElement::Operation {
                id,
                operation_name,
                model_ref,
                session_group: session_group.map(|sg| database::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                    working_dir: sg.working_dir,
                }),
                block_config: block_config.map(|bc| database::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
        common::ChainElement::Transform { id, prompt, model_ref, session_group, block_config } => {
            database::ChainElement::Transform {
                id,
                prompt,
                model_ref,
                session_group: session_group.map(|sg| database::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                    working_dir: sg.working_dir,
                }),
                block_config: block_config.map(|bc| database::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
        common::ChainElement::GenericPrompt { id, prompt, session_group, block_config } => {
            database::ChainElement::GenericPrompt {
                id,
                prompt,
                session_group: session_group.map(|sg| database::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                    working_dir: sg.working_dir,
                }),
                block_config: block_config.map(|bc| database::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
        common::ChainElement::Memory { id, key, mode } => {
            database::ChainElement::Memory {
                id,
                key,
                mode: match mode {
                    common::MemoryMode::Store => database::MemoryMode::Store,
                    common::MemoryMode::Retrieve => database::MemoryMode::Retrieve,
                },
            }
        }
        common::ChainElement::Loop { id, max_iterations } => {
            database::ChainElement::Loop { id, max_iterations }
        }
        common::ChainElement::Tool { id, tool_name, tool_params, block_config } => {
            database::ChainElement::Tool {
                id,
                tool_name,
                tool_params,
                block_config: block_config.map(|bc| database::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
        common::ChainElement::Payload { id, payload_id, block_config } => {
            database::ChainElement::Payload {
                id,
                payload_id,
                block_config: block_config.map(|bc| database::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
        common::ChainElement::Termination { id, block_config } => {
            database::ChainElement::Termination {
                id,
                block_config: block_config.map(|bc| database::BlockConfig {
                    max_runtime: bc.max_runtime,
                    yolo_mode: bc.yolo_mode,
                    working_dir: bc.working_dir,
                    require_all_inputs: bc.require_all_inputs,
                }),
            }
        }
    }
}
