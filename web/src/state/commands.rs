use super::AppState;

impl AppState {
    //
    // --- Command tracking for Orchestrator ---.
    //

    /// Add a pending command ID
    pub async fn add_pending_command(&self, command_id: String) {
        let mut pending = self.pending_commands.write().await;
        pending.insert(command_id);
    }

    /// Remove a pending command ID
    pub async fn remove_pending_command(&self, command_id: &str) {
        let mut pending = self.pending_commands.write().await;
        pending.remove(command_id);
    }

    /// Check if a command is pending
    #[allow(dead_code)]
    pub async fn is_command_pending(&self, command_id: &str) -> bool {
        let pending = self.pending_commands.read().await;
        pending.contains(command_id)
    }

    /// Store a command response
    pub async fn store_command_response(&self, command_id: String, result: common::NodeCommandResult) {
        //
        // Only store if it's a pending command.
        //
        let is_pending = {
            let pending = self.pending_commands.read().await;
            pending.contains(&command_id)
        };
        if is_pending {
            let mut responses = self.command_responses.write().await;
            responses.insert(command_id, result);
        }
    }

    /// Take a command response (removes from storage only if response exists)
    pub async fn take_command_response(&self, command_id: &str) -> Option<common::NodeCommandResult> {
        let mut responses = self.command_responses.write().await;
        if let Some(result) = responses.remove(command_id) {
            //
            // Only remove from pending if we found a response.
            //
            let mut pending = self.pending_commands.write().await;
            pending.remove(command_id);
            Some(result)
        } else {
            None
        }
    }

    //
    // --- Semantic operation request tracking ---.
    //

    /// Add a pending semantic op request ID
    pub async fn add_pending_semantic_op(&self, request_id: String) {
        let mut pending = self.pending_semantic_ops.write().await;
        pending.insert(request_id);
    }

    /// Store a semantic op queued response (request_id -> operation_id)
    pub async fn store_semantic_op_response(&self, request_id: String, operation_id: String) {
        //
        // Only store if it's a pending request.
        //
        let is_pending = {
            let pending = self.pending_semantic_ops.read().await;
            pending.contains(&request_id)
        };
        if is_pending {
            let mut responses = self.semantic_op_responses.write().await;
            responses.insert(request_id, operation_id);
        }
    }

    /// Take a semantic op response (returns operation_id and removes from storage)
    pub async fn take_semantic_op_response(&self, request_id: &str) -> Option<String> {
        let mut responses = self.semantic_op_responses.write().await;
        if let Some(operation_id) = responses.remove(request_id) {
            let mut pending = self.pending_semantic_ops.write().await;
            pending.remove(request_id);
            Some(operation_id)
        } else {
            None
        }
    }
}
