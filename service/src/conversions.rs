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
                }),
            }
        }
        database::ChainElement::MemoryStore { id, key } => {
            common::ChainElement::MemoryStore { id, key }
        }
        database::ChainElement::MemoryRetrieve { id, key } => {
            common::ChainElement::MemoryRetrieve { id, key }
        }
        database::ChainElement::Loop { id, max_iterations } => {
            common::ChainElement::Loop { id, max_iterations }
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
                }),
            }
        }
        common::ChainElement::MemoryStore { id, key } => {
            database::ChainElement::MemoryStore { id, key }
        }
        common::ChainElement::MemoryRetrieve { id, key } => {
            database::ChainElement::MemoryRetrieve { id, key }
        }
        common::ChainElement::Loop { id, max_iterations } => {
            database::ChainElement::Loop { id, max_iterations }
        }
    }
}
