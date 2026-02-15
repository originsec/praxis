impl AgentChatManager {
    pub fn new(
        db: Arc<Database>,
        channel: Channel,
        node_registry: Arc<NodeRegistry>,
        pending_commands: Arc<PendingCommands>,
    ) -> Self {
        Self {
            db,
            channel,
            node_registry,
            pending_commands,
            active_session: RwLock::new(None),
        }
    }
}
