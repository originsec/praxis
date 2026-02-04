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
        database::ChainElement::Operation { id, operation_name, model_ref, session_group } => {
            common::ChainElement::Operation {
                id,
                operation_name,
                model_ref,
                session_group: session_group.map(|sg| common::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        database::ChainElement::Transform { id, prompt, model_ref, session_group } => {
            common::ChainElement::Transform {
                id,
                prompt,
                model_ref,
                session_group: session_group.map(|sg| common::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        database::ChainElement::GenericPrompt { id, prompt, session_group } => {
            common::ChainElement::GenericPrompt {
                id,
                prompt,
                session_group: session_group.map(|sg| common::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        database::ChainElement::Termination { id, termination_type, label } => {
            common::ChainElement::Termination {
                id,
                termination_type: match termination_type {
                    database::TerminationType::Raw => common::ChainTerminationType::Raw,
                    database::TerminationType::Semantic { prompt, model_ref } => {
                        common::ChainTerminationType::Semantic { prompt, model_ref }
                    }
                },
                label,
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
        common::ChainElement::Operation { id, operation_name, model_ref, session_group } => {
            database::ChainElement::Operation {
                id,
                operation_name,
                model_ref,
                session_group: session_group.map(|sg| database::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        common::ChainElement::Transform { id, prompt, model_ref, session_group } => {
            database::ChainElement::Transform {
                id,
                prompt,
                model_ref,
                session_group: session_group.map(|sg| database::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        common::ChainElement::GenericPrompt { id, prompt, session_group } => {
            database::ChainElement::GenericPrompt {
                id,
                prompt,
                session_group: session_group.map(|sg| database::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        common::ChainElement::Termination { id, termination_type, label } => {
            database::ChainElement::Termination {
                id,
                termination_type: match termination_type {
                    common::ChainTerminationType::Raw => database::TerminationType::Raw,
                    common::ChainTerminationType::Semantic { prompt, model_ref } => {
                        database::TerminationType::Semantic { prompt, model_ref }
                    }
                },
                label,
            }
        }
    }
}
