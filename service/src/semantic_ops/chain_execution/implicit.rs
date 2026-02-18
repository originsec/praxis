use chrono::Utc;
use uuid::Uuid;

use crate::database::{ChainConnection, ChainDefinition, ChainElement, TriggerType};

//
// Creates an implicit (transient) chain for running a standalone operation.
// Implicit chains are not persisted to the database - they're created in-memory
// for executing single operations through the unified chain executor.
//

/// Create an implicit chain that wraps a single operation.
///
/// The chain structure is:
/// - Manual Trigger -> Operation -> Raw Termination
///
/// The chain ID starts with "implicit_" to identify it as transient.
#[allow(dead_code)]
pub fn create_implicit_chain(
    operation_name: &str,
    operation_display_name: &str,
    _yolo_mode: bool,
) -> ChainDefinition {
    let chain_id = format!("implicit_{}", Uuid::new_v4());
    let trigger_id = format!("trigger_{}", Uuid::new_v4());
    let op_id = format!("op_{}", Uuid::new_v4());

    let now = Utc::now();

    //
    // Create elements: Trigger -> Operation (terminal).
    //

    let elements = vec![
        ChainElement::Trigger {
            id: trigger_id.clone(),
            trigger_type: TriggerType::Manual,
        },
        ChainElement::Operation {
            id: op_id.clone(),
            operation_name: operation_name.to_string(),
            model_ref: None,
            //
            // No session group - the operation's own YOLO mode will be used.
            //
            session_group: None,
            block_config: None,
        },
    ];

    //
    // Create connections: Trigger -> Operation.
    //

    let connections = vec![ChainConnection {
        id: format!("conn_{}", Uuid::new_v4()),
        from_element: trigger_id,
        to_element: op_id,
        from_port: 0,
        to_port: 0,
        condition: None,
    }];

    ChainDefinition {
        id: chain_id,
        name: format!("[Implicit] {}", operation_display_name),
        description: format!("Implicit chain for standalone operation: {}", operation_name),
        category: "implicit".to_string(),
        elements,
        connections,
        disabled: false,
        //
        // No timeout for implicit chains - the operation has its own timeout.
        //
        timeout: None,
        positions: std::collections::HashMap::new(),
        created_at: now,
        updated_at: now,
    }
}

/// Check if a chain ID represents an implicit chain.
pub fn is_implicit_chain(chain_id: &str) -> bool {
    chain_id.starts_with("implicit_")
}
