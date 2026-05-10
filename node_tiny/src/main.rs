#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod acp_state;
mod acp_server;
mod praxis;
mod registration;
mod runtime;
mod utils;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::praxis::{AgentFactory, AgentRegistry};
use crate::registration::register_with_service;

const RECONNECT_DELAY_SECS: u64 = 5;

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
        token_clone.cancel();
    });

    token
}

fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime")
        .block_on(async_main());
}

async fn async_main() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let shutdown_token = setup_shutdown_signal();
    common::log_info!("Starting tiny node...");

    let node_id = utils::get_or_create_node_id();
    common::log_info!("Node ID: {}", node_id);

    let factory = Arc::new(AgentFactory::new(None));
    let registry = Arc::new(RwLock::new({
        let mut r = AgentRegistry::new();
        r.rebuild(&factory);
        r
    }));

    loop {
        if shutdown_token.is_cancelled() {
            break;
        }

        let result = match register_with_service(node_id.clone(), shutdown_token.clone()).await {
            Ok(Some(r)) => {
                common::log_info!("Registered with service. Node ID: {}", r.node_id);
                common::logging::set_event_log_enabled(r.event_logging_enabled);
                r
            }
            Ok(None) => break,
            Err(e) => {
                common::log_error!("Failed to register: {}", e);
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(RECONNECT_DELAY_SECS)) => {}
                    _ = shutdown_token.cancelled() => break,
                }
                continue;
            }
        };

        match runtime::run(
            Arc::new(result.channel),
            result.node_id,
            result.node_queue,
            registry.clone(),
            factory.clone(),
            shutdown_token.clone(),
            result.praxis_agent_enabled,
            result.praxis_agent_config,
        )
        .await
        {
            Ok(runtime::RuntimeExit::Shutdown) => break,
            Ok(runtime::RuntimeExit::Reset) => {
                common::log_info!("Node reset, re-registering...");
                continue;
            }
            Err(e) => common::log_error!("Runtime error: {}", e),
        }

        if shutdown_token.is_cancelled() {
            break;
        }

        common::log_warn!(
            "Connection lost. Reconnecting in {} seconds...",
            RECONNECT_DELAY_SECS
        );
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(RECONNECT_DELAY_SECS)) => {}
            _ = shutdown_token.cancelled() => break,
        }
    }

    common::log_info!("Shutdown complete");
}
