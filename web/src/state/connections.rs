use super::{AppState, ConnectionId, ConnectionInfo};

impl AppState {
    /// Register a new connection
    pub async fn add_connection(&self, id: ConnectionId) {
        let mut connections = self.connections.write().await;
        connections.insert(
            id,
            ConnectionInfo {
                connected_at: chrono::Utc::now(),
            },
        );
    }

    /// Remove a connection
    pub async fn remove_connection(&self, id: &str) {
        let mut connections = self.connections.write().await;
        connections.remove(id);
    }

    /// Get number of active connections
    #[allow(dead_code)]
    pub async fn connection_count(&self) -> usize {
        let connections = self.connections.read().await;
        connections.len()
    }
}
