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
use tokio_util::sync::CancellationToken;

const RECONNECT_DELAY_SECS: u64 = 5;

//
// Creates a cancellation token that gets cancelled on SIGINT/SIGTERM.
// This allows Ctrl+C to work at any point in the application.
//
fn setup_shutdown_signal() -> CancellationToken {
    let token = CancellationToken::new();
    let token_clone = token.clone();

    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm =
                signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
            let mut sigint =
                signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");
            tokio::select! {
                _ = sigterm.recv() => common::log_info!("Received SIGTERM"),
                _ = sigint.recv() => common::log_info!("Received SIGINT"),
            }
        }
        #[cfg(windows)]
        {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to register Ctrl+C handler");
            common::log_info!("Received Ctrl+C");
        }
        agent_connectors::lua::cdp::request_shutdown();
        token_clone.cancel();
    });

    token
}

fn main() {
    let worker_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(20)
        .max(20);

    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime")
        .block_on(async_main());
}

async fn async_main() {
    use tracing_subscriber::EnvFilter;

    //
    // Initialize tracing in both debug and release builds.
    // Filter out noisy chromiumoxide deserialization errors.
    // Respects RUST_LOG environment variable (defaults to "info").
    //
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"))
        .add_directive("chromiumoxide::conn=off".parse().unwrap())
        .add_directive("chromiumoxide::handler=off".parse().unwrap());

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    //
    // Set up global shutdown signal handler. This spawns a task that waits for
    // SIGINT/SIGTERM and cancels the token, allowing Ctrl+C to work at any
    // point in the application.
    //

    let shutdown_token = setup_shutdown_signal();

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
    // All supported agent targets are held in a registry. The initial registry
    // contains native agents plus embedded and user-dir Lua agents. Service-
    // managed Lua scripts are pushed via AgentRegistryUpdate after the node
    // registers.
    //

    let factory = Arc::new(AgentFactory::new());
    let mut initial_registry = AgentRegistry::new();
    initial_registry.rebuild(&factory, &[]);
    let registry = Arc::new(RwLock::new(initial_registry));

    //
    // Main reconnection loop.
    //
    loop {
        if shutdown_token.is_cancelled() {
            common::log_info!("Shutdown requested, exiting...");
            break;
        }

        let selected_agent: Arc<Mutex<Option<Arc<dyn Agent>>>> = Arc::new(Mutex::new(None));

        let result = match register_with_service(node_id.clone(), shutdown_token.clone()).await {
            Ok(Some(result)) => {
                common::log_info!(
                    "Successfully registered with service. Node ID: {}",
                    result.node_id
                );

                common::logging::set_event_log_enabled(result.event_logging_enabled);
                common::log_info!(
                    "Event logging {} from registration ack",
                    if result.event_logging_enabled { "enabled" } else { "disabled" }
                );

                result
            }
            Ok(None) => {
                common::log_info!("Shutdown requested during registration");
                break;
            }
            Err(e) => {
                common::log_error!("Failed to register with service: {}", e);
                common::log_warn!(
                    "Will retry registration in {} seconds...",
                    RECONNECT_DELAY_SECS
                );

                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(RECONNECT_DELAY_SECS)) => {}
                    _ = shutdown_token.cancelled() => {
                        common::log_info!("Shutdown requested during reconnection delay");
                        break;
                    }
                }
                continue;
            }
        };

        match runtime::run(
            Arc::new(result.channel),
            result.node_id,
            result.node_queue,
            registry.clone(),
            selected_agent,
            factory.clone(),
            shutdown_token.clone(),
            result.lua_scripts,
        )
        .await
        {
            Ok(runtime::RuntimeExit::Shutdown) => {
                common::log_info!("Runtime exited cleanly");
                break;
            }
            Ok(runtime::RuntimeExit::Reset) => {
                crate::agent_connectors::lua::runtime::clear_reset();
                common::log_info!("Node reset, re-registering immediately...");
                continue;
            }
            Err(e) => {
                common::log_error!("Runtime error: {}", e);
            }
        }

        if shutdown_token.is_cancelled() {
            common::log_info!("Shutdown requested, exiting...");
            break;
        }

        common::log_warn!(
            "Connection lost. Reconnecting in {} seconds...",
            RECONNECT_DELAY_SECS
        );

        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(RECONNECT_DELAY_SECS)) => {}
            _ = shutdown_token.cancelled() => {
                common::log_info!("Shutdown requested during reconnection delay");
                break;
            }
        }
    }
}
