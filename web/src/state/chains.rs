use common::ChainExecutionStatus;

use super::AppState;

impl AppState {
    //
    // --- Chain Definitions ---.
    //

    /// Update chain definitions
    pub async fn update_chain_definitions(&self, defs: Vec<common::ChainDefinitionInfo>) {
        let mut cached = self.chain_definitions.write().await;
        *cached = defs;
    }

    /// Get chain definitions
    pub async fn get_chain_definitions(&self) -> Vec<common::ChainDefinitionInfo> {
        let cached = self.chain_definitions.read().await;
        cached.clone()
    }

    //
    // --- Chain Executions ---.
    //

    /// Update a chain execution
    pub async fn update_chain_execution(&self, exec: common::ChainExecutionUpdate) {
        let mut execs = self.chain_executions.write().await;
        execs.insert(exec.execution_id.clone(), exec);
    }

    /// Get all chain executions
    pub async fn get_chain_executions(&self) -> Vec<common::ChainExecutionUpdate> {
        let execs = self.chain_executions.read().await;
        execs.values().cloned().collect()
    }

    /// Remove a chain execution
    #[allow(dead_code)]
    pub async fn remove_chain_execution(&self, execution_id: &str) {
        let mut execs = self.chain_executions.write().await;
        execs.remove(execution_id);
    }

    /// Clear all finished chain executions
    #[allow(dead_code)]
    pub async fn clear_finished_chain_executions(&self) {
        let mut execs = self.chain_executions.write().await;
        execs.retain(|_, exec| {
            matches!(exec.status, ChainExecutionStatus::Running | ChainExecutionStatus::Queued)
        });
    }
}
