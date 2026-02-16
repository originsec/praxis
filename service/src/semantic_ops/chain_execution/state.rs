use chrono::{DateTime, Utc};
use common::{
    ChainExecutionStatus, ChainExecutionUpdate, ElementConfig, ElementContext,
    ElementExecution, ElementExecutionStatus,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Execution state for a single chain execution
pub struct ChainExecutionState {
    pub execution_id: String,
    pub chain_id: String,
    pub chain_name: String,
    pub node_id: String,
    pub agent_short_name: String,
    pub status: ChainExecutionStatus,
    pub elements: HashMap<String, ElementExecution>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Final outputs from termination elements (element_id -> output)
    pub outputs: HashMap<String, String>,
}

impl ChainExecutionState {
    /// Create a new execution state
    pub fn new(
        execution_id: String,
        chain_id: String,
        chain_name: String,
        node_id: String,
        agent_short_name: String,
        element_ids: Vec<String>,
    ) -> Self {
        let mut elements = HashMap::new();
        for id in element_ids {
            elements.insert(
                id.clone(),
                ElementExecution {
                    element_id: id,
                    status: ElementExecutionStatus::Pending,
                    config: None,
                    context: None,
                    started_at: None,
                    completed_at: None,
                },
            );
        }

        Self {
            execution_id,
            chain_id,
            chain_name,
            node_id,
            agent_short_name,
            status: ChainExecutionStatus::Queued,
            elements,
            started_at: Utc::now(),
            ended_at: None,
            outputs: HashMap::new(),
        }
    }

    /// Mark the execution as running (transition from Queued)
    pub fn mark_running(&mut self) {
        self.status = ChainExecutionStatus::Running;
    }

    /// Set element configuration (from chain definition)
    #[allow(dead_code)]
    pub fn set_element_config(&mut self, element_id: &str, config: ElementConfig) {
        if let Some(elem) = self.elements.get_mut(element_id) {
            elem.config = Some(config);
        }
    }

    /// Set element runtime context (input data, session info)
    #[allow(dead_code)]
    pub fn set_element_context(&mut self, element_id: &str, context: ElementContext) {
        if let Some(elem) = self.elements.get_mut(element_id) {
            elem.context = Some(context);
        }
    }

    /// Mark an element as waiting for inputs
    #[allow(dead_code)]
    pub fn set_element_waiting(&mut self, element_id: &str) {
        if let Some(elem) = self.elements.get_mut(element_id) {
            elem.status = ElementExecutionStatus::WaitingForInputs;
        }
    }

    /// Mark an element as running with config and context
    pub fn set_element_running_with_context(
        &mut self,
        element_id: &str,
        config: ElementConfig,
        context: ElementContext,
    ) {
        if let Some(elem) = self.elements.get_mut(element_id) {
            elem.status = ElementExecutionStatus::Running;
            elem.config = Some(config);
            elem.context = Some(context);
            elem.started_at = Some(Utc::now());
        }
    }

    /// Mark an element as running (legacy, without config/context)
    #[allow(dead_code)]
    pub fn set_element_running(&mut self, element_id: &str) {
        if let Some(elem) = self.elements.get_mut(element_id) {
            elem.status = ElementExecutionStatus::Running;
            elem.started_at = Some(Utc::now());
        }
    }

    /// Mark an element as completed with output and optional success flag
    pub fn set_element_completed(
        &mut self,
        element_id: &str,
        output: String,
        success: Option<bool>,
    ) {
        if let Some(elem) = self.elements.get_mut(element_id) {
            elem.status = ElementExecutionStatus::Completed { output, success };
            elem.completed_at = Some(Utc::now());
        }
    }

    /// Mark an element as failed with error
    pub fn set_element_failed(&mut self, element_id: &str, error: String) {
        if let Some(elem) = self.elements.get_mut(element_id) {
            elem.status = ElementExecutionStatus::Failed { error };
            elem.completed_at = Some(Utc::now());
        }
    }

    /// Mark an element as skipped
    pub fn set_element_skipped(&mut self, element_id: &str) {
        if let Some(elem) = self.elements.get_mut(element_id) {
            elem.status = ElementExecutionStatus::Skipped;
            elem.completed_at = Some(Utc::now());
        }
    }

    /// Add output from a termination element
    pub fn add_output(&mut self, label: String, output: String) {
        self.outputs.insert(label, output);
    }

    /// Mark the entire execution as completed
    pub fn mark_completed(&mut self) {
        self.status = ChainExecutionStatus::Completed;
        self.ended_at = Some(Utc::now());
    }

    /// Mark the entire execution as failed
    pub fn mark_failed(&mut self) {
        self.status = ChainExecutionStatus::Failed;
        self.ended_at = Some(Utc::now());
    }

    /// Mark the entire execution as cancelled
    pub fn mark_cancelled(&mut self) {
        self.status = ChainExecutionStatus::Cancelled;
        self.ended_at = Some(Utc::now());
    }

    /// Convert to update message for broadcasting
    pub fn to_update(&self) -> ChainExecutionUpdate {
        ChainExecutionUpdate {
            execution_id: self.execution_id.clone(),
            chain_id: self.chain_id.clone(),
            chain_name: self.chain_name.clone(),
            node_id: self.node_id.clone(),
            agent_short_name: self.agent_short_name.clone(),
            status: self.status.clone(),
            elements: self.elements.clone(),
            started_at: self.started_at,
            ended_at: self.ended_at,
            outputs: self.outputs.clone(),
        }
    }
}

/// Registry for tracking active chain executions
pub struct ChainExecutionRegistry {
    executions: RwLock<HashMap<String, Arc<RwLock<ChainExecutionState>>>>,
}

impl ChainExecutionRegistry {
    pub fn new() -> Self {
        Self {
            executions: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new execution
    pub fn register(&self, state: ChainExecutionState) -> Arc<RwLock<ChainExecutionState>> {
        let execution_id = state.execution_id.clone();
        let arc = Arc::new(RwLock::new(state));
        self.executions.write().unwrap().insert(execution_id, arc.clone());
        arc
    }

    /// Get an execution by ID
    #[allow(dead_code)]
    pub fn get(&self, execution_id: &str) -> Option<Arc<RwLock<ChainExecutionState>>> {
        self.executions.read().unwrap().get(execution_id).cloned()
    }

    /// Remove a completed execution
    pub fn remove(&self, execution_id: &str) {
        self.executions.write().unwrap().remove(execution_id);
    }

    /// List all active executions
    pub fn list(&self) -> Vec<ChainExecutionUpdate> {
        self.executions
            .read()
            .unwrap()
            .values()
            .map(|e| e.read().unwrap().to_update())
            .collect()
    }
}
