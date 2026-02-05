use std::collections::{HashMap, HashSet, VecDeque};

use crate::database::{ChainDefinition, ChainElement, ElementId, SessionGroup};

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
pub struct ExecutionGraph {
    /// All nodes indexed by element ID
    pub nodes: HashMap<ElementId, ExecutionNode>,
    /// The trigger element ID (start of execution)
    #[allow(dead_code)]
    pub trigger_id: ElementId,
    /// All termination element IDs
    #[allow(dead_code)]
    pub termination_ids: Vec<ElementId>,
    /// Session groups and their member element IDs (group_id -> element_ids)
    pub session_groups: HashMap<String, (SessionGroup, Vec<ElementId>)>,
    /// Elements that are first in their session group (and should include input context)
    pub first_in_session: HashSet<ElementId>,
    /// Topologically sorted execution order
    pub execution_order: Vec<ElementId>,
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
        let mut termination_ids: Vec<ElementId> = Vec::new();
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
                ChainElement::Termination { .. } => {
                    termination_ids.push(id.clone());
                }
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
            //
            // Add dependent link.
            //
            if let Some(from_node) = nodes.get_mut(&conn.from_element) {
                from_node.dependents.push(conn.to_element.clone());
            }
            //
            // Add dependency link.
            //
            if let Some(to_node) = nodes.get_mut(&conn.to_element) {
                to_node.dependencies.push(conn.from_element.clone());
            }
        }

        let trigger_id = trigger_id.ok_or("Chain has no trigger element")?;

        //
        // Perform topological sort.
        //
        let execution_order = Self::topological_sort(&nodes, &trigger_id)?;

        //
        // Determine first element in each session group (based on execution
        // order).
        //
        let mut first_in_session: HashSet<ElementId> = HashSet::new();
        for (_group_id, (_sg, element_ids)) in &session_groups {
            //
            // Find the first element in this group that appears in execution
            // order.
            //
            for order_id in &execution_order {
                if element_ids.contains(order_id) {
                    first_in_session.insert(order_id.clone());
                    break;
                }
            }
        }

        //
        // Log session groups for debugging.
        //
        for (group_id, (sg, element_ids)) in &session_groups {
            common::log_info!(
                "Session group '{}' (color: {}, yolo: {}): elements={:?}",
                group_id,
                sg.color,
                sg.yolo_mode,
                element_ids.iter().map(|id| &id[..8.min(id.len())]).collect::<Vec<_>>()
            );
        }
        common::log_info!(
            "First in session elements: {:?}",
            first_in_session.iter().map(|id| &id[..8.min(id.len())]).collect::<Vec<_>>()
        );

        Ok(Self {
            nodes,
            trigger_id,
            termination_ids,
            session_groups,
            first_in_session,
            execution_order,
        })
    }

    /// Perform topological sort using Kahn's algorithm
    fn topological_sort(
        nodes: &HashMap<ElementId, ExecutionNode>,
        trigger_id: &ElementId,
    ) -> Result<Vec<ElementId>, String> {
        let mut in_degree: HashMap<ElementId, usize> = HashMap::new();
        let mut result: Vec<ElementId> = Vec::new();
        let mut queue: VecDeque<ElementId> = VecDeque::new();

        //
        // Calculate in-degrees.
        //
        for (id, node) in nodes {
            in_degree.insert(id.clone(), node.dependencies.len());
        }

        //
        // Start with the trigger (which has no dependencies).
        //
        queue.push_back(trigger_id.clone());

        while let Some(id) = queue.pop_front() {
            result.push(id.clone());

            if let Some(node) = nodes.get(&id) {
                for dependent_id in &node.dependents {
                    if let Some(deg) = in_degree.get_mut(dependent_id) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(dependent_id.clone());
                        }
                    }
                }
            }
        }

        //
        // Check for cycles.
        //
        if result.len() != nodes.len() {
            return Err("Chain contains a cycle".to_string());
        }

        Ok(result)
    }

    /// Get element IDs that can execute in parallel at a given point
    /// (elements with satisfied dependencies that aren't yet executed)
    #[allow(dead_code)]
    pub fn get_ready_elements(&self, completed: &HashSet<ElementId>) -> Vec<ElementId> {
        self.execution_order
            .iter()
            .filter(|id| {
                if completed.contains(*id) {
                    return false;
                }
                let node = match self.nodes.get(*id) {
                    Some(n) => n,
                    None => return false,
                };
                node.dependencies.iter().all(|dep| completed.contains(dep))
            })
            .cloned()
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

    /// Check if an element is the first in its session group
    pub fn is_first_in_session(&self, element_id: &ElementId) -> bool {
        self.first_in_session.contains(element_id)
    }

    /// Get all elements in a session group in execution order
    #[allow(dead_code)]
    pub fn get_session_group_elements(&self, group_id: &str) -> Vec<ElementId> {
        let element_ids = match self.session_groups.get(group_id) {
            Some((_, ids)) => ids,
            None => return Vec::new(),
        };

        //
        // Return in execution order.
        //
        self.execution_order
            .iter()
            .filter(|id| element_ids.contains(id))
            .cloned()
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
    use crate::database::{ChainConnection, TerminationType, TriggerType};

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
                },
                ChainElement::Termination {
                    id: "term1".to_string(),
                    termination_type: TerminationType::Raw,
                    label: "Output".to_string(),
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "op1".to_string(),
                    to_element: "term1".to_string(),
                    from_port: 0,
                    to_port: 0,
                },
            ],
            disabled: false,
            timeout: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let graph = ExecutionGraph::from_chain(&chain).unwrap();
        assert_eq!(graph.execution_order, vec!["trigger1", "op1", "term1"]);
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
                },
                ChainElement::Operation {
                    id: "op2".to_string(),
                    operation_name: "test::op2".to_string(),
                    model_ref: None,
                    session_group: None,
                },
                ChainElement::Termination {
                    id: "term1".to_string(),
                    termination_type: TerminationType::Raw,
                    label: "Output 1".to_string(),
                },
                ChainElement::Termination {
                    id: "term2".to_string(),
                    termination_type: TerminationType::Raw,
                    label: "Output 2".to_string(),
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op2".to_string(),
                    from_port: 0,
                    to_port: 0,
                },
                ChainConnection {
                    id: "c3".to_string(),
                    from_element: "op1".to_string(),
                    to_element: "term1".to_string(),
                    from_port: 0,
                    to_port: 0,
                },
                ChainConnection {
                    id: "c4".to_string(),
                    from_element: "op2".to_string(),
                    to_element: "term2".to_string(),
                    from_port: 0,
                    to_port: 0,
                },
            ],
            disabled: false,
            timeout: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let graph = ExecutionGraph::from_chain(&chain).unwrap();

        //
        // Trigger should be first.
        //
        assert_eq!(graph.execution_order[0], "trigger1");

        //
        // Both branches should be detected.
        //
        assert_eq!(graph.get_output_count(&"trigger1".to_string()), 2);
    }

    #[test]
    fn test_session_groups() {
        let session_group = SessionGroup {
            id: "sg1".to_string(),
            color: "#8B5CF6".to_string(),
            yolo_mode: false,
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
                },
                ChainElement::Operation {
                    id: "op2".to_string(),
                    operation_name: "test::op2".to_string(),
                    model_ref: None,
                    session_group: Some(session_group.clone()),
                },
                ChainElement::Termination {
                    id: "term1".to_string(),
                    termination_type: TerminationType::Raw,
                    label: "Output".to_string(),
                },
            ],
            connections: vec![
                ChainConnection {
                    id: "c1".to_string(),
                    from_element: "trigger1".to_string(),
                    to_element: "op1".to_string(),
                    from_port: 0,
                    to_port: 0,
                },
                ChainConnection {
                    id: "c2".to_string(),
                    from_element: "op1".to_string(),
                    to_element: "op2".to_string(),
                    from_port: 0,
                    to_port: 0,
                },
                ChainConnection {
                    id: "c3".to_string(),
                    from_element: "op2".to_string(),
                    to_element: "term1".to_string(),
                    from_port: 0,
                    to_port: 0,
                },
            ],
            disabled: false,
            timeout: None,
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
        // Check first in session (op1 comes before op2 in execution order).
        //
        assert!(graph.is_first_in_session(&"op1".to_string()));
        assert!(!graph.is_first_in_session(&"op2".to_string()));

        //
        // Check get_session_group.
        //
        assert!(graph.get_session_group(&"op1".to_string()).is_some());
        assert!(graph.get_session_group(&"trigger1".to_string()).is_none());
    }
}
