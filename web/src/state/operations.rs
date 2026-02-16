use common::SemanticOpStatus;

use super::AppState;

impl AppState {
    pub async fn update_operation(&self, op: common::SemanticOpUpdate) {
        let mut ops = self.operations.write().await;
        ops.insert(op.operation_id.clone(), op);
    }

    #[allow(dead_code)]
    pub async fn remove_operation(&self, operation_id: &str) {
        let mut ops = self.operations.write().await;
        ops.remove(operation_id);
    }

    #[allow(dead_code)]
    pub async fn get_operations(&self) -> Vec<common::SemanticOpUpdate> {
        let ops = self.operations.read().await;
        ops.values().cloned().collect()
    }

    #[allow(dead_code)]
    pub async fn clear_finished_operations(&self) {
        let mut ops = self.operations.write().await;
        ops.retain(|_, op| matches!(op.status, SemanticOpStatus::Running | SemanticOpStatus::Queued));
    }

    pub async fn update_operation_definitions(&self, defs: Vec<common::OperationDefinitionInfo>) {
        let mut cached = self.operation_definitions.write().await;
        *cached = defs;
    }

    #[allow(dead_code)]
    pub async fn get_operation_definitions(&self) -> Vec<common::OperationDefinitionInfo> {
        let cached = self.operation_definitions.read().await;
        cached.clone()
    }
}
