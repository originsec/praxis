#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod agent_connectors;
mod app;
mod handlers;
mod intercept;
mod runtime;
mod terminal;
mod utils;

use agent_connectors::{Agent, AgentFactory, AgentRegistry};
use app::register_with_service;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

const RECONNECT_DELAY_SECS: u64 = 5;

#[tokio::main]
async fn main() {
    #[cfg(debug_assertions)]
    {
        use tracing_subscriber::EnvFilter;

        //
        // Filter out noisy chromiumoxide deserialization errors.
        //

        let filter = EnvFilter::new("info")
            .add_directive("chromiumoxide::conn=off".parse().unwrap())
            .add_directive("chromiumoxide::handler=off".parse().unwrap());

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .init();
    }

    //
    // Install the ring crypto provider for rustls.
    //
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    common::log_info!("Starting node...");

    //
    // Clean up any stale intercept state from a previous run that crashed.
    //

    intercept::cleanup_stale_state();

    //
    // Load or create a persistent node ID that survives restarts.
    //

    let node_id = utils::get_or_create_node_id();
    common::log_info!("Node ID: {}", node_id);

    //
    // All supported agent targets are held in a registry.
    // Each agent is a self-contained implementation.
    //
    let factory = AgentFactory::new();
    let registry = Arc::new(RwLock::new(AgentRegistry::load_from_factory(&factory)));

    //
    // Main reconnection loop.
    //
    loop {
        let selected_agent: Arc<Mutex<Option<Arc<dyn Agent>>>> = Arc::new(Mutex::new(None));

        //
        // Register with the service via RabbitMQ.
        //
        let result = match register_with_service(node_id.clone()).await {
            Ok(result) => {
                common::log_info!(
                    "Successfully registered with service. Node ID: {}",
                    result.node_id
                );
                result
            }
            Err(e) => {
                common::log_error!("Failed to register with service: {}", e);
                common::log_warn!(
                    "Will retry registration in {} seconds...",
                    RECONNECT_DELAY_SECS
                );
                tokio::time::sleep(std::time::Duration::from_secs(RECONNECT_DELAY_SECS)).await;
                continue;
            }
        };

        //
        // Run the main event loop - listen to queues.
        //
        match runtime::run(
            Arc::new(result.channel),
            result.node_id,
            result.node_queue,
            registry.clone(),
            selected_agent,
        )
        .await
        {
            Ok(()) => {
                //
                // Clean shutdown (e.g., SIGTERM).
                //
                common::log_info!("Runtime exited cleanly");
                break;
            }
            Err(e) => {
                common::log_error!("Runtime error: {}", e);
            }
        }

        //
        // Connection lost - reconnect.
        //
        common::log_warn!(
            "Connection lost. Reconnecting in {} seconds...",
            RECONNECT_DELAY_SECS
        );
        tokio::time::sleep(std::time::Duration::from_secs(RECONNECT_DELAY_SECS)).await;
    }
}
