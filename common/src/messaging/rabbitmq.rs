/// Node signal queue - nodes send messages here
pub const NODE_SIGNAL_QUEUE: &str = "NodeSignal";

/// Event log queue - service publishes event logs here (deprecated, use specific queues)
pub const EVENT_LOG_QUEUE: &str = "EventLog";

/// Node event log queue - nodes send event logs here
pub const NODE_EVENT_LOG_QUEUE: &str = "NodeEventLog";

/// Web event log queue - web sends event logs here
pub const WEB_EVENT_LOG_QUEUE: &str = "WebEventLog";

/// Service event log queue - service writes its own event logs here
pub const SERVICE_EVENT_LOG_QUEUE: &str = "ServiceEventLog";

/// Node broadcast exchange (fanout) - service broadcasts to all nodes
pub const NODE_BROADCAST_EXCHANGE: &str = "NodeBroadcast";

/// Client signal queue - clients send messages here
pub const CLIENT_SIGNAL_QUEUE: &str = "ClientSignal";

/// Client broadcast exchange (fanout) - service broadcasts to all clients
pub const CLIENT_BROADCAST_EXCHANGE: &str = "ClientBroadcast";

/// Default RabbitMQ URL if PRAXIS_RABBITMQ_URL environment variable is not set
const DEFAULT_RABBITMQ_URL: &str = "amqp://praxis:praxis@localhost:5672";

static RABBITMQ_URL_CELL: OnceLock<String> = OnceLock::new();

/// Returns the RabbitMQ URL from the PRAXIS_RABBITMQ_URL environment variable,
/// or the default value if the environment variable is not set.
pub fn rabbitmq_url() -> &'static str {
    RABBITMQ_URL_CELL.get_or_init(|| {
        std::env::var("PRAXIS_RABBITMQ_URL").unwrap_or_else(|_| DEFAULT_RABBITMQ_URL.to_string())
    })
}

pub async fn publish_json<T: Serialize>(
    channel: &Channel,
    routing_key: &str,
    message: &T,
) -> anyhow::Result<PublisherConfirm> {
    let payload = serde_json::to_vec(message)?;
    let confirm = channel
        .basic_publish(
            "",
            routing_key,
            BasicPublishOptions::default(),
            &payload,
            BasicProperties::default(),
        )
        .await?;
    Ok(confirm)
}

/// Publish a JSON message to a fanout exchange.
pub async fn publish_json_exchange<T: Serialize>(
    channel: &Channel,
    exchange: &str,
    message: &T,
) -> anyhow::Result<PublisherConfirm> {
    let payload = serde_json::to_vec(message)?;
    let confirm = channel
        .basic_publish(
            exchange,
            "",
            BasicPublishOptions::default(),
            &payload,
            BasicProperties::default(),
        )
        .await?;
    Ok(confirm)
}

pub fn client_queue_name(client_id: &str) -> String {
    format!("Client_{}", client_id)
}

/// Generate a node-specific queue name
pub fn node_queue_name(node_id: &str) -> String {
    format!("Node_{}", node_id)
}

/// Generate a node-specific semantic parser queue name
/// This separate queue is used for semantic parser responses to avoid
/// deadlocks when command handlers are waiting for responses
pub fn node_semantic_queue_name(node_id: &str) -> String {
    format!("Node_{}_semantic", node_id)
}

/// Macro for logging events
#[macro_export]
macro_rules! log_event {
    ($logger:expr, $name:expr, $($arg:tt)*) => {
        $logger.log($name, &format!($($arg)*)).await?
    };
}

