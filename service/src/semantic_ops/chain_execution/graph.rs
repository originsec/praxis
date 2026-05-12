use std::collections::HashMap;

use crate::database::{ChainConnection, ChainDefinition, ChainElement, ElementId, SessionGroup};

/// Node in the execution graph
#[derive(Debug, Clone)]
pub struct ExecutionNode {
    pub element: ChainElement,
    /// IDs of elements that must complete before this one can run
    pub dependencies: Vec<ElementId>,
    /// IDs of elements that depend on this one
    pub dependents: Vec<ElementId>,
}

/// Execution graph built from a chain definition
#[derive(Debug)]
pub struct ExecutionGraph {
    /// All nodes indexed by element ID
    pub nodes: HashMap<ElementId, ExecutionNode>,
    /// The trigger element ID (start of execution)
    pub trigger_id: ElementId,
    /// Full set of connections from the chain definition
    pub connections: Vec<ChainConnection>,
    /// Session groups and their member element IDs (group_id -> (SessionGroup, element_ids))
    pub session_groups: HashMap<String, (SessionGroup, Vec<ElementId>)>,
}

impl ExecutionGraph {
    /// Build an execution graph from a chain definition
    pub fn from_chain(chain: &ChainDefinition) -> Result<Self, String> {
        //
        // First, validate the chain.
        //
        chain.validate()?;

        let mut nodes: HashMap<ElementId, ExecutionNode> = HashMap::new();
        let mut trigger_id: Option<ElementId> = None;
        let mut session_groups: HashMap<String, (SessionGroup, Vec<ElementId>)> = HashMap::new();

        //
        // Build nodes from elements.
        //
        for element in &chain.elements {
            let id = element.id().clone();

            //
            // Track special elements and session groups.
            //
            match element {
                ChainElement::Trigger { .. } => {
                    trigger_id = Some(id.clone());
                }
                ChainElement::Memory { .. }
                | ChainElement::Loop { .. }
                | ChainElement::Tool { .. }
                | ChainElement::Payload { .. }
                | ChainElement::Termination { .. } => {}
                ChainElement::Operation { session_group, .. }
                | ChainElement::Transform { session_group, .. }
                | ChainElement::GenericPrompt { session_group, .. } => {
                    if let Some(sg) = session_group {
                        session_groups
                            .entry(sg.id.clone())
                            .or_insert_with(|| (sg.clone(), Vec::new()))
                            .1
                            .push(id.clone());
                    }
                }
            }

            nodes.insert(
                id,
                ExecutionNode {
                    element: element.clone(),
                    dependencies: Vec::new(),
                    dependents: Vec::new(),
                },
            );
        }

        //
        // Build edges from connections.
        //
        for conn in &chain.connections {
            if let Some(from_node) = nodes.get_mut(&conn.from_element) {
                from_node.dependents.push(conn.to_element.clone());
            }
            if let Some(to_node) = nodes.get_mut(&conn.to_element) {
                to_node.dependencies.push(conn.from_element.clone());
            }
        }

        let trigger_id = trigger_id.ok_or("Chain has no trigger element")?;

        //
        // Log session groups for debugging.
        //
        for (group_id, (sg, element_ids)) in &session_groups {
            common::log_info!(
                "Session group '{}' (color: {}, yolo: {}): elements={:?}",
                group_id,
                sg.color,
                sg.yolo_mode,
                element_ids
                    .iter()
                    .map(|id| common::short_id(id))
                    .collect::<Vec<_>>()
            );
        }

        Ok(Self {
            nodes,
            trigger_id,
            connections: chain.connections.clone(),
            session_groups,
        })
    }

    /// Get outgoing connections from an element
    pub fn outgoing_connections(&self, element_id: &ElementId) -> Vec<&ChainConnection> {
        self.connections
            .iter()
            .filter(|c| &c.from_element == element_id)
            .collect()
    }

    /// Get incoming connections to an element
    pub fn incoming_connections(&self, element_id: &ElementId) -> Vec<&ChainConnection> {
        self.connections
            .iter()
            .filter(|c| &c.to_element == element_id)
            .collect()
    }

    /// Get the session group for an element (if any)
    pub fn get_session_group(&self, element_id: &ElementId) -> Option<&SessionGroup> {
        for (_group_id, (sg, element_ids)) in &self.session_groups {
            if element_ids.contains(element_id) {
                return Some(sg);
            }
        }
        None
    }

    /// Get the session group ID for an element (if any)
    pub fn get_session_group_id(&self, element_id: &ElementId) -> Option<String> {
        for (group_id, (_sg, element_ids)) in &self.session_groups {
            if element_ids.contains(element_id) {
                return Some(group_id.clone());
            }
        }
        None
    }

    /// Check if `to` is reachable from `from` via outgoing connections (BFS).
    pub fn is_reachable(&self, from: &str, to: &str) -> bool {
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(from.to_string());
        while let Some(current) = queue.pop_front() {
            if current == to {
                return true;
            }
            if !visited.insert(current.clone()) {
                continue;
            }
            for conn in self.outgoing_connections(&current) {
                queue.push_back(conn.to_element.clone());
            }
        }
        false
    }

    /// Get elements with no outgoing connections (terminal elements)
    #[allow(dead_code)]
    pub fn terminal_elements(&self) -> Vec<ElementId> {
        self.nodes
            .iter()
            .filter(|(_, node)| node.dependents.is_empty())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get the number of incoming edges (for merge detection)
    #[allow(dead_code)]
    pub fn get_input_count(&self, element_id: &ElementId) -> usize {
        self.nodes
            .get(element_id)
            .map(|n| n.dependencies.len())
            .unwrap_or(0)
    }

    /// Get the number of outgoing edges (for branch detection)
    #[allow(dead_code)]
    pub fn get_output_count(&self, element_id: &ElementId) -> usize {
        self.nodes
            .get(element_id)
            .map(|n| n.dependents.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TriggerType;
    use std::collections::HashMap;

    #[test]
    fn test_simple_chain() {
        let chain = ChainDefinition {
            id: "test".to_string(),
            name: "Test Chain".to_string(),
            description: "".to_string(),
            category: "test".to_string(),
            elements: vec![
                ChainElement::Trigger {
                    id: "trigger1".to_string(),
                    trigger_type: TriggerType::Manual,
                },
                ChainElement::Operation {
                    id: "op1".to_string(),
                    operation_name: "test::op".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Termination {
                    id: "end1".to_string(),
                    block_config: None,
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "op1".to_string(),
                    to_element: "end1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
            ],
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let graph = ExecutionGraph::from_chain(&chain).unwrap();
        assert_eq!(graph.trigger_id, "trigger1");
        assert_eq!(graph.terminal_elements(), vec!["end1".to_string()]);
        assert_eq!(graph.outgoing_connections(&"trigger1".to_string()).len(), 1);
        assert_eq!(graph.incoming_connections(&"op1".to_string()).len(), 1);
    }

    #[test]
    fn test_branching_chain() {
        let chain = ChainDefinition {
            id: "test".to_string(),
            name: "Test Chain".to_string(),
            description: "".to_string(),
            category: "test".to_string(),
            elements: vec![
                ChainElement::Trigger {
                    id: "trigger1".to_string(),
                    trigger_type: TriggerType::Manual,
                },
                ChainElement::Operation {
                    id: "op1".to_string(),
                    operation_name: "test::op1".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Operation {
                    id: "op2".to_string(),
                    operation_name: "test::op2".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Termination {
                    id: "end1".to_string(),
                    block_config: None,
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op2".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c3".to_string(),
                    from_element: "op1".to_string(),
                    to_element: "end1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c4".to_string(),
                    from_element: "op2".to_string(),
                    to_element: "end1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
            ],
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let graph = ExecutionGraph::from_chain(&chain).unwrap();

        //
        // Trigger should have 2 outgoing connections.
        //
        assert_eq!(graph.get_output_count(&"trigger1".to_string()), 2);

        //
        // Termination is the only terminal element.
        //
        let terminals = graph.terminal_elements();
        assert_eq!(terminals, vec!["end1".to_string()]);
    }

    #[test]
    fn test_session_groups() {
        let session_group = SessionGroup {
            id: "sg1".to_string(),
            color: "#8B5CF6".to_string(),
            yolo_mode: false,
            working_dir: None,
        };

        let chain = ChainDefinition {
            id: "test".to_string(),
            name: "Test Chain".to_string(),
            description: "".to_string(),
            category: "test".to_string(),
            elements: vec![
                ChainElement::Trigger {
                    id: "trigger1".to_string(),
                    trigger_type: TriggerType::Manual,
                },
                ChainElement::Operation {
                    id: "op1".to_string(),
                    operation_name: "test::op1".to_string(),
                    model_ref: None,
                    session_group: Some(session_group.clone()),
                    block_config: None,
                },
                ChainElement::Operation {
                    id: "op2".to_string(),
                    operation_name: "test::op2".to_string(),
                    model_ref: None,
                    session_group: Some(session_group.clone()),
                    block_config: None,
                },
                ChainElement::Termination {
                    id: "end1".to_string(),
                    block_config: None,
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "op1".to_string(),
                    to_element: "op2".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c3".to_string(),
                    from_element: "op2".to_string(),
                    to_element: "end1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
            ],
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let graph = ExecutionGraph::from_chain(&chain).unwrap();

        //
        // Check session group tracking.
        //
        assert!(graph.session_groups.contains_key("sg1"));
        let (sg, elements) = graph.session_groups.get("sg1").unwrap();
        assert_eq!(sg.color, "#8B5CF6");
        assert!(elements.contains(&"op1".to_string()));
        assert!(elements.contains(&"op2".to_string()));

        //
        // Check get_session_group.
        //
        assert!(graph.get_session_group(&"op1".to_string()).is_some());
        assert!(graph.get_session_group(&"trigger1".to_string()).is_none());

        //
        // Termination is the terminal element.
        //
        assert_eq!(graph.terminal_elements(), vec!["end1".to_string()]);
    }

    #[test]
    fn test_memory_elements() {
        use crate::database::MemoryMode;

        let chain = ChainDefinition {
            id: "test".to_string(),
            name: "Test Chain".to_string(),
            description: "".to_string(),
            category: "test".to_string(),
            elements: vec![
                ChainElement::Trigger {
                    id: "trigger1".to_string(),
                    trigger_type: TriggerType::Manual,
                },
                ChainElement::Memory {
                    id: "ms1".to_string(),
                    key: "data_key".to_string(),
                    mode: MemoryMode::Store,
                },
                ChainElement::Memory {
                    id: "mr1".to_string(),
                    key: "data_key".to_string(),
                    mode: MemoryMode::Retrieve,
                },
                ChainElement::Termination {
                    id: "end1".to_string(),
                    block_config: None,
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "ms1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "ms1".to_string(),
                    to_element: "mr1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c3".to_string(),
                    from_element: "mr1".to_string(),
                    to_element: "end1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
            ],
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let graph = ExecutionGraph::from_chain(&chain).unwrap();
        assert_eq!(graph.trigger_id, "trigger1");

        //
        // Memory elements have no session groups.
        //
        assert!(graph.get_session_group(&"ms1".to_string()).is_none());
        assert!(graph.get_session_group(&"mr1".to_string()).is_none());

        //
        // Termination is the terminal element.
        //
        let terminals = graph.terminal_elements();
        assert_eq!(terminals.len(), 1);
        assert!(terminals.contains(&"end1".to_string()));
    }

    #[test]
    fn test_loop_element_valid() {
        //
        // Valid cycle: Op -> Loop -> Op (via port 0), Loop -> Op2 (via
        // port 1) -> Termination.
        //
        let chain = ChainDefinition {
            id: "test".to_string(),
            name: "Test Chain".to_string(),
            description: "".to_string(),
            category: "test".to_string(),
            elements: vec![
                ChainElement::Trigger {
                    id: "trigger1".to_string(),
                    trigger_type: TriggerType::Manual,
                },
                ChainElement::Operation {
                    id: "op1".to_string(),
                    operation_name: "test::op1".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Loop {
                    id: "loop1".to_string(),
                    max_iterations: 3,
                },
                ChainElement::Operation {
                    id: "op_done".to_string(),
                    operation_name: "test::done".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Termination {
                    id: "end1".to_string(),
                    block_config: None,
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "op1".to_string(),
                    to_element: "loop1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                //
                // Loop port 0 (retry) -> back to op1.
                //
                ChainConnection {
                    id: "c3".to_string(),
                    from_element: "loop1".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                //
                // Loop port 1 (exhausted) -> op_done.
                //
                ChainConnection {
                    id: "c4".to_string(),
                    from_element: "loop1".to_string(),
                    to_element: "op_done".to_string(),
                    from_port: 1,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c5".to_string(),
                    from_element: "op_done".to_string(),
                    to_element: "end1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
            ],
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        //
        // Should validate successfully — cycle contains a Loop element.
        //
        let result = ExecutionGraph::from_chain(&chain);
        assert!(result.is_ok());

        let graph = result.unwrap();
        assert_eq!(graph.outgoing_connections(&"loop1".to_string()).len(), 2);
        assert_eq!(graph.incoming_connections(&"loop1".to_string()).len(), 1);
    }

    #[test]
    fn test_cycle_without_loop_rejected() {
        //
        // Invalid cycle: Op1 -> Op2 -> Op1 (no Loop element). Termination
        // is present but unreachable — the cycle check should still catch
        // this.
        //
        let chain = ChainDefinition {
            id: "test".to_string(),
            name: "Test Chain".to_string(),
            description: "".to_string(),
            category: "test".to_string(),
            elements: vec![
                ChainElement::Trigger {
                    id: "trigger1".to_string(),
                    trigger_type: TriggerType::Manual,
                },
                ChainElement::Operation {
                    id: "op1".to_string(),
                    operation_name: "test::op1".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Operation {
                    id: "op2".to_string(),
                    operation_name: "test::op2".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Termination {
                    id: "end1".to_string(),
                    block_config: None,
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "op1".to_string(),
                    to_element: "op2".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                //
                // Creates cycle: op2 -> op1.
                //
                ChainConnection {
                    id: "c3".to_string(),
                    from_element: "op2".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
            ],
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let result = ExecutionGraph::from_chain(&chain);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("cycle without a Loop element"),
            "Expected cycle error, got: {}",
            err
        );
    }

    #[test]
    fn test_conditional_connections() {
        use crate::database::ConnectionCondition;

        let chain = ChainDefinition {
            id: "test".to_string(),
            name: "Test Chain".to_string(),
            description: "".to_string(),
            category: "test".to_string(),
            elements: vec![
                ChainElement::Trigger {
                    id: "trigger1".to_string(),
                    trigger_type: TriggerType::Manual,
                },
                ChainElement::Operation {
                    id: "op1".to_string(),
                    operation_name: "test::op1".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Operation {
                    id: "op_success".to_string(),
                    operation_name: "test::success".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Operation {
                    id: "op_failure".to_string(),
                    operation_name: "test::failure".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Termination {
                    id: "end1".to_string(),
                    block_config: None,
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "op1".to_string(),
                    to_element: "op_success".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: Some(ConnectionCondition::OnSuccess),
                },
                ChainConnection {
                    id: "c3".to_string(),
                    from_element: "op1".to_string(),
                    to_element: "op_failure".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: Some(ConnectionCondition::OnFailure),
                },
                ChainConnection {
                    id: "c4".to_string(),
                    from_element: "op_success".to_string(),
                    to_element: "end1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c5".to_string(),
                    from_element: "op_failure".to_string(),
                    to_element: "end1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
            ],
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let graph = ExecutionGraph::from_chain(&chain).unwrap();

        //
        // op1 has two outgoing connections (success + failure).
        //
        let outgoing = graph.outgoing_connections(&"op1".to_string());
        assert_eq!(outgoing.len(), 2);

        //
        // Termination is the only terminal element.
        //
        let terminals = graph.terminal_elements();
        assert_eq!(terminals, vec!["end1".to_string()]);

        //
        // Verify conditions are preserved on connections.
        //
        let success_conn = outgoing
            .iter()
            .find(|c| c.to_element == "op_success")
            .unwrap();
        assert!(matches!(
            success_conn.condition,
            Some(ConnectionCondition::OnSuccess)
        ));

        let failure_conn = outgoing
            .iter()
            .find(|c| c.to_element == "op_failure")
            .unwrap();
        assert!(matches!(
            failure_conn.condition,
            Some(ConnectionCondition::OnFailure)
        ));
    }

    #[test]
    fn test_missing_termination_rejected() {
        //
        // Chain with no Termination element should fail validation.
        //
        let chain = ChainDefinition {
            id: "test".to_string(),
            name: "Test Chain".to_string(),
            description: "".to_string(),
            category: "test".to_string(),
            elements: vec![
                ChainElement::Trigger {
                    id: "trigger1".to_string(),
                    trigger_type: TriggerType::Manual,
                },
                ChainElement::Operation {
                    id: "op1".to_string(),
                    operation_name: "test::op".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
            ],
            connections: vec![ChainConnection {
                id: "c1".to_string(),
                from_element: "trigger1".to_string(),
                to_element: "op1".to_string(),
                from_port: 0,
                to_port: 0,
                condition: None,
            }],
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let result = ExecutionGraph::from_chain(&chain);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("must have exactly one termination element")
        );
    }

    #[test]
    fn test_loop_max_iterations_validation() {
        let chain = ChainDefinition {
            id: "test".to_string(),
            name: "Test Chain".to_string(),
            description: "".to_string(),
            category: "test".to_string(),
            elements: vec![
                ChainElement::Trigger {
                    id: "trigger1".to_string(),
                    trigger_type: TriggerType::Manual,
                },
                ChainElement::Loop {
                    id: "loop1".to_string(),
                    max_iterations: 0,
                },
                ChainElement::Termination {
                    id: "end1".to_string(),
                    block_config: None,
                },
            ],
            connections: vec![ChainConnection {
                id: "c1".to_string(),
                from_element: "trigger1".to_string(),
                to_element: "loop1".to_string(),
                from_port: 0,
                to_port: 0,
                condition: None,
            }],
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let result = ExecutionGraph::from_chain(&chain);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max_iterations must be >= 1"));
    }

    #[test]
    fn test_connection_query_methods() {
        let chain = ChainDefinition {
            id: "test".to_string(),
            name: "Test Chain".to_string(),
            description: "".to_string(),
            category: "test".to_string(),
            elements: vec![
                ChainElement::Trigger {
                    id: "t".to_string(),
                    trigger_type: TriggerType::Manual,
                },
                ChainElement::Operation {
                    id: "a".to_string(),
                    operation_name: "test::a".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Operation {
                    id: "b".to_string(),
                    operation_name: "test::b".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Transform {
                    id: "c".to_string(),
                    prompt: "merge".to_string(),
                    model_ref: None,
                    session_group: None,
                    block_config: None,
                },
                ChainElement::Termination {
                    id: "end1".to_string(),
                    block_config: None,
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "t".to_string(),
                    to_element: "a".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "t".to_string(),
                    to_element: "b".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c3".to_string(),
                    from_element: "a".to_string(),
                    to_element: "c".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c4".to_string(),
                    from_element: "b".to_string(),
                    to_element: "c".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
                ChainConnection {
                    id: "c5".to_string(),
                    from_element: "c".to_string(),
                    to_element: "end1".to_string(),
                    from_port: 0,
                    to_port: 0,
                    condition: None,
                },
            ],
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let graph = ExecutionGraph::from_chain(&chain).unwrap();

        //
        // Trigger has 2 outgoing, 0 incoming.
        //
        assert_eq!(graph.outgoing_connections(&"t".to_string()).len(), 2);
        assert_eq!(graph.incoming_connections(&"t".to_string()).len(), 0);

        //
        // 'c' (merge point) has 2 incoming, 1 outgoing (to termination).
        //
        assert_eq!(graph.incoming_connections(&"c".to_string()).len(), 2);
        assert_eq!(graph.outgoing_connections(&"c".to_string()).len(), 1);

        //
        // 'a' has 1 incoming, 1 outgoing.
        //
        assert_eq!(graph.incoming_connections(&"a".to_string()).len(), 1);
        assert_eq!(graph.outgoing_connections(&"a".to_string()).len(), 1);

        //
        // Only Termination is terminal.
        //
        let terminals = graph.terminal_elements();
        assert_eq!(terminals.len(), 1);
        assert!(terminals.contains(&"end1".to_string()));
    }
}
