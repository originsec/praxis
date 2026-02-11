//! Praxis Web Server
//!
//! A WebSocket server that bridges browser clients to the Praxis service
//! via RabbitMQ. This acts as another client from the service's perspective.

mod messages;
mod rabbitmq;
mod orchestrator;
mod state;
mod websocket;

pub use common::rabbitmq_url;

use axum::{
    body::Body,
    extract::Path,
    http::{header, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::path::PathBuf;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use rabbitmq::RabbitMqClient;
use state::AppState;
use websocket::{ws_handler, WsState};

#[derive(Embed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

async fn static_handler(req: Request<Body>) -> impl IntoResponse {
    let path = req.uri().path().trim_start_matches('/');

    //
    // Try to serve the exact path first.
    //
    if let Some(content) = FrontendAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data.into_owned()))
            .unwrap();
    }

    //
    // For SPA routing, serve index.html for paths that don't exist
    // (client-side router will handle them).
    //
    if let Some(content) = FrontendAssets::get("index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html")
            .body(Body::from(content.data.into_owned()))
            .unwrap();
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("Not Found"))
        .unwrap()
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Print the startup banner
pub fn print_banner(rabbitmq_url: &str, listen_addr: &SocketAddr) {
    let creature = [
        "     ▄▄▄███▄▄▄     ",
        "   ▄█▀▀     ▀▀█▄   ",
        "  ██  ●     ●  ██  ",
        "  ██     ▄     ██  ",
        "   ▀█▄ ▀▀▀▀▀ ▄█▀   ",
        "  ▄▄ ▀▀█████▀▀ ▄▄  ",
        " █▀▀█▄▄     ▄▄█▀▀█ ",
        " █▄▄█▀ ▀▀▀▀▀ ▀█▄▄█ ",
        "      ▀▀▀▀▀▀▀      ",
    ];

    let width = 60;

    println!("\x1b[90m╭{}╮\x1b[0m", "─".repeat(width));
    println!("\x1b[90m│\x1b[0m{}\x1b[90m│\x1b[0m", " ".repeat(width));

    let title = format!("Praxis Web Server v{}", VERSION);
    let padding = (width - title.len()) / 2;
    println!(
        "\x1b[90m│\x1b[0m{}\x1b[1;36m{}\x1b[0m{}\x1b[90m│\x1b[0m",
        " ".repeat(padding),
        title,
        " ".repeat(width - padding - title.len())
    );

    let subtitle = "by [Ø] Origin";
    //
    // 13 visible chars.
    //
    let padding = (width - 13) / 2;
    println!(
        "\x1b[90m│\x1b[0m{}\x1b[35m{}\x1b[0m{}\x1b[90m│\x1b[0m",
        " ".repeat(padding),
        subtitle,
        " ".repeat(width - padding - 13)
    );

    println!("\x1b[90m│\x1b[0m{}\x1b[90m│\x1b[0m", " ".repeat(width));
    println!("\x1b[90m├{}┤\x1b[0m", "─".repeat(width));

    for line in &creature {
        let padding = (width - line.chars().count()) / 2;
        let right_padding = width - padding - line.chars().count();
        println!(
            "\x1b[90m│\x1b[0m{}\x1b[35m{}\x1b[0m{}\x1b[90m│\x1b[0m",
            " ".repeat(padding),
            line,
            " ".repeat(right_padding)
        );
    }

    println!("\x1b[90m├{}┤\x1b[0m", "─".repeat(width));

    let rmq_line = format!("RabbitMQ: {}", rabbitmq_url);
    let rmq_display = if rmq_line.len() > width - 4 {
        format!("{}...", &rmq_line[..width - 7])
    } else {
        rmq_line
    };
    println!(
        "\x1b[90m│\x1b[0m  \x1b[90m{}\x1b[0m{}\x1b[90m│\x1b[0m",
        rmq_display,
        " ".repeat(width - 2 - rmq_display.len())
    );

    let listen_line = format!("Listening: http://{}", listen_addr);
    println!(
        "\x1b[90m│\x1b[0m  \x1b[92m{}\x1b[0m{}\x1b[90m│\x1b[0m",
        listen_line,
        " ".repeat(width - 2 - listen_line.len())
    );

    println!("\x1b[90m│\x1b[0m{}\x1b[90m│\x1b[0m", " ".repeat(width));
    println!("\x1b[90m╰{}╯\x1b[0m", "─".repeat(width));
    println!();
}

//
// Model list API types.
//
#[derive(Deserialize)]
#[allow(dead_code)]
struct ModelListRequest {
    provider: String,
    api_key: String,
}

#[derive(Serialize)]
struct ModelListResponse {
    models: Vec<String>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

//
// Node binary download types.
//

#[derive(Serialize)]
struct NodeDownloadInfo {
    platform: String,
    filename: String,
    available: bool,
    size: Option<u64>,
}

#[derive(Serialize)]
struct DownloadsInfoResponse {
    nodes: Vec<NodeDownloadInfo>,
}

//
// Provider list API types.
//

#[derive(Serialize)]
struct ProviderInfo {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct ProvidersResponse {
    providers: Vec<ProviderInfo>,
}

fn get_nodes_dir() -> PathBuf {
    std::env::var("PRAXIS_NODES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            //
            // Fallback to ~/.praxis/bin/nodes for local development.
            //
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".praxis")
                .join("bin")
                .join("nodes")
        })
}

async fn get_downloads_info() -> Json<DownloadsInfoResponse> {
    let nodes_dir = get_nodes_dir();

    let platforms = vec![
        ("linux", "praxis_node_linux"),
        ("windows", "praxis_node_windows.exe"),
    ];

    let nodes: Vec<NodeDownloadInfo> = platforms
        .into_iter()
        .map(|(platform, filename)| {
            let path = nodes_dir.join(filename);
            let (available, size) = if path.exists() {
                let size = std::fs::metadata(&path).ok().map(|m| m.len());
                let non_empty = size.map(|s| s > 0).unwrap_or(false);
                (non_empty, size)
            } else {
                (false, None)
            };
            NodeDownloadInfo {
                platform: platform.to_string(),
                filename: filename.to_string(),
                available,
                size,
            }
        })
        .collect();

    Json(DownloadsInfoResponse { nodes })
}

async fn download_node(Path(platform): Path<String>) -> impl IntoResponse {
    let nodes_dir = get_nodes_dir();

    let filename = match platform.as_str() {
        "linux" => "praxis_node_linux",
        "windows" => "praxis_node_windows.exe",
        _ => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Invalid platform. Use 'linux' or 'windows'."))
                .unwrap();
        }
    };

    let path = nodes_dir.join(filename);

    match tokio::fs::read(&path).await {
        Ok(data) => {
            let content_type = "application/octet-stream";
            let content_disposition = format!("attachment; filename=\"{}\"", filename);

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CONTENT_DISPOSITION, content_disposition)
                .header(header::CONTENT_LENGTH, data.len())
                .body(Body::from(data))
                .unwrap()
        }
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from(format!(
                "Node binary for '{}' not found. Build with Docker or run install.sh first.",
                platform
            )))
            .unwrap(),
    }
}

//
// API handler for listing available providers.
//
async fn get_providers() -> Json<ProvidersResponse> {
    let providers = common::ai::Provider::all()
        .iter()
        .map(|p| ProviderInfo {
            id: p.as_str().to_string(),
            name: p.display_name().to_string(),
        })
        .collect();

    Json(ProvidersResponse { providers })
}

//
// API handler for listing models - fetches dynamically from provider APIs.
//
async fn list_models(
    Json(request): Json<ModelListRequest>,
) -> Result<Json<ModelListResponse>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    match common::ai::fetch_models_for_provider(&request.provider, &request.api_key).await {
        Ok(models) => Ok(Json(ModelListResponse { models })),
        Err(e) => Err((
            axum::http::StatusCode::BAD_GATEWAY,
            Json(ErrorResponse { error: e }),
        )),
    }
}

const RECONNECT_DELAY_SECS: u64 = 5;

/// Run the Praxis web server
pub async fn run() -> anyhow::Result<()> {
    //
    // Server address.
    //
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));

    //
    // Print banner first (only once).
    //
    print_banner(common::rabbitmq_url(), &addr);

    //
    // Main loop - restarts on RabbitMQ connection loss.
    //
    loop {
        match run_server(addr).await {
            Ok(()) => {
                //
                // Server stopped due to shutdown signal (RabbitMQ connection
                // lost).
                //
                common::log_warn!(
                    "RabbitMQ connection lost. Restarting in {} seconds...",
                    RECONNECT_DELAY_SECS
                );
            }
            Err(e) => {
                common::log_error!(
                    "Server error: {}. Restarting in {} seconds...",
                    e, RECONNECT_DELAY_SECS
                );
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(RECONNECT_DELAY_SECS)).await;
    }
}

/// Run the web server until shutdown signal (RabbitMQ connection lost)
async fn run_server(addr: SocketAddr) -> anyhow::Result<()> {
    //
    // Generate client ID for this web server instance.
    //
    let client_id = format!("web_{}", Uuid::new_v4());

    //
    // Initialize event log channel for web logs.
    //
    let (event_log_tx, mut event_log_rx) = tokio::sync::mpsc::unbounded_channel::<common::ApplicationLogEntry>();
    common::logging::init("web".to_string(), event_log_tx);

    //
    // Create shared state.
    //
    let app_state = AppState::new(client_id.clone());

    //
    // Connect to RabbitMQ (retries until successful).
    //
    let rabbitmq: Arc<RabbitMqClient> = Arc::new(RabbitMqClient::connect(Arc::clone(&app_state)).await);

    //
    // Spawn task to forward web logs to service via RabbitMQ.
    // Also stops when shutdown is signaled (RabbitMQ connection lost).
    //
    let rabbitmq_for_logging = rabbitmq.clone();
    let shutdown_notify_for_logging = Arc::clone(&app_state.shutdown_notify);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                entry = event_log_rx.recv() => {
                    match entry {
                        Some(entry) => {
                            if let Err(_) = rabbitmq_for_logging.send_event_log(entry).await {
                                //
                                // Channel broken, stop task (will restart with
                                // new connection).
                                //
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = shutdown_notify_for_logging.notified() => {
                    break;
                }
            }
        }
    });

    //
    // Start consuming RabbitMQ messages BEFORE registering
    // so we receive the initial state update from the service.
    //
    let rabbitmq_clone: Arc<RabbitMqClient> = Arc::clone(&rabbitmq);
    rabbitmq_clone.start_consuming().await
        .map_err(|e| anyhow::anyhow!("Failed to start RabbitMQ consumer: {}", e))?;

    //
    // Register with the service (will receive state update immediately).
    //
    rabbitmq.register().await
        .map_err(|e| anyhow::anyhow!("Failed to register with service: {}", e))?;

    //
    // Create WebSocket state.
    //
    let ws_state = Arc::new(WsState::new(
        Arc::clone(&app_state),
        Arc::clone(&rabbitmq),
    ));

    //
    // Set up CORS.
    //
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    //
    // Build router with embedded SPA fallback.
    //
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/providers", get(get_providers))
        .route("/api/models", post(list_models))
        .route("/api/downloads/info", get(get_downloads_info))
        .route("/api/downloads/node/{platform}", get(download_node))
        .fallback(static_handler)
        .layer(cors)
        .with_state(ws_state);

    common::log_info!("Starting web server on {}", addr);

    //
    // Bind and run server. Abort immediately on RabbitMQ connection loss
    // (don't wait for graceful shutdown - WebSocket connections are long-lived).
    //
    let listener = tokio::net::TcpListener::bind(addr).await?;

    let shutdown_notify = Arc::clone(&app_state.shutdown_notify);

    tokio::select! {
        result = axum::serve(listener, app) => {
            result?;
        }
        _ = shutdown_notify.notified() => {
            //
            // RabbitMQ connection lost - abort immediately and restart.
            //
        }
    }

    Ok(())
}
