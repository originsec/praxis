use anyhow::Result;
use async_trait::async_trait;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

//
// Webhook listener for chain triggers. `POST /hooks/<trigger_id>` with
// `Authorization: Bearer <secret>` fires the webhook trigger with the JSON
// body (capped at WEBHOOK_MAX_BODY_BYTES) as trigger context. The service
// stores only the SHA-256 of each secret and compares digests in constant
// time. Secrets and request headers are never logged.
//

/// Maximum accepted request body size.
pub const WEBHOOK_MAX_BODY_BYTES: usize = 64 * 1024;

/// Prefix that makes leaked webhook secrets easy to recognise and grep for.
const SECRET_PREFIX: &str = "pxwh_";

pub fn generate_secret() -> String {
    //
    // Two v4 UUIDs give 244 bits from the OS CSPRNG (getrandom).
    //
    format!(
        "{}{}{}",
        SECRET_PREFIX,
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub fn hash_secret(secret: &str) -> String {
    Sha256::digest(secret.as_bytes())
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

//
// Check the bearer token against the stored hash. Both sides are hex
// SHA-256 digests of equal length, so the comparison time does not depend
// on the presented token.
//
fn bearer_matches(headers: &HeaderMap, stored_hash: &str) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(token) = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
    else {
        return false;
    };
    constant_time_eq(hash_secret(token.trim()).as_bytes(), stored_hash.as_bytes())
}

pub struct WebhookTarget {
    pub secret_hash: String,
    pub enabled: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WebhookFireOutcome {
    Accepted,
    RateLimited,
    NotFound,
}

//
// What the HTTP layer needs from the trigger engine. A trait so the handler
// can be tested without a database or chain executor.
//
#[async_trait]
pub trait WebhookBackend: Send + Sync {
    async fn lookup(&self, trigger_id: &str) -> Result<Option<WebhookTarget>>;
    async fn fire(&self, trigger_id: &str, payload: Value) -> WebhookFireOutcome;
}

type Backend = Arc<dyn WebhookBackend>;

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

async fn handle_hook(
    State(backend): State<Backend>,
    Path(trigger_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.len() > WEBHOOK_MAX_BODY_BYTES {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "body too large");
    }

    let target = match backend.lookup(&trigger_id).await {
        Ok(Some(target)) => target,
        Ok(None) => return error(StatusCode::NOT_FOUND, "unknown webhook"),
        Err(e) => {
            common::log_error!("Webhook trigger lookup failed: {}", e);
            return error(StatusCode::INTERNAL_SERVER_ERROR, "lookup failed");
        }
    };

    if !bearer_matches(&headers, &target.secret_hash) {
        common::log_warn!(
            "Rejected webhook call for trigger {}: missing or invalid bearer token",
            common::short_id(&trigger_id)
        );
        let mut response = error(StatusCode::UNAUTHORIZED, "invalid or missing bearer token");
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        return response;
    }

    if !target.enabled {
        return error(StatusCode::CONFLICT, "trigger is disabled");
    }

    let payload = if body.iter().all(u8::is_ascii_whitespace) {
        Value::Null
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(v) => v,
            Err(_) => return error(StatusCode::BAD_REQUEST, "body must be JSON"),
        }
    };

    match backend.fire(&trigger_id, payload).await {
        WebhookFireOutcome::Accepted => (
            StatusCode::ACCEPTED,
            Json(json!({ "status": "accepted", "trigger_id": trigger_id })),
        )
            .into_response(),
        WebhookFireOutcome::RateLimited => {
            error(StatusCode::TOO_MANY_REQUESTS, "trigger fired too recently")
        }
        WebhookFireOutcome::NotFound => error(StatusCode::NOT_FOUND, "unknown webhook"),
    }
}

pub fn router(backend: Backend) -> axum::Router {
    axum::Router::new()
        .route("/hooks/{trigger_id}", axum::routing::post(handle_hook))
        .layer(DefaultBodyLimit::max(WEBHOOK_MAX_BODY_BYTES))
        .with_state(backend)
}

struct RunningWebhookServer {
    addr: SocketAddr,
    cancel: CancellationToken,
    serve_task: tokio::task::JoinHandle<()>,
}

//
// Start/stop the listener as settings change, mirroring McpServerManager:
// restarting on the same address is a no-op and stop() waits (bounded) for
// the socket to be released before a rebind.
//
pub struct WebhookServerManager {
    running: RwLock<Option<RunningWebhookServer>>,
}

impl WebhookServerManager {
    pub fn new() -> Self {
        Self {
            running: RwLock::new(None),
        }
    }

    pub async fn start(&self, bind_address: &str, port: u16, backend: Backend) -> Result<()> {
        let ip: IpAddr = bind_address.trim().parse().map_err(|e| {
            anyhow::anyhow!("invalid webhook bind address '{}': {}", bind_address, e)
        })?;
        let addr = SocketAddr::new(ip, port);

        if let Some(r) = self.running.read().await.as_ref()
            && r.addr == addr
        {
            return Ok(());
        }
        self.stop().await;

        let listener = tokio::net::TcpListener::bind(addr).await?;
        let cancel = CancellationToken::new();
        let shutdown = cancel.clone();
        let app = router(backend);
        let serve_task = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
                .await;
        });

        *self.running.write().await = Some(RunningWebhookServer {
            addr,
            cancel,
            serve_task,
        });
        common::log_info!(
            "Webhook listener started on http://{}/hooks/<trigger_id>",
            addr
        );
        Ok(())
    }

    //
    // Start, restart or stop the listener to match the current service
    // config. Used at startup and whenever a webhook_server_* key changes.
    //
    pub async fn apply_config(
        &self,
        config: &crate::config::ServiceConfig,
        engine: &Arc<crate::trigger_engine::TriggerEngine>,
    ) {
        if !config.is_webhook_server_enabled() {
            self.stop().await;
            return;
        }
        let bind = config.get_webhook_server_bind_address();
        let port = config.get_webhook_server_port();
        let backend: Backend = Arc::new(engine.clone());
        if let Err(e) = self.start(&bind, port, backend).await {
            common::log_error!(
                "Failed to start webhook listener on {}:{}: {}",
                bind,
                port,
                e
            );
        }
    }

    pub async fn stop(&self) {
        let running = self.running.write().await.take();
        let Some(running) = running else {
            return;
        };
        common::log_info!("Stopping webhook listener on {}", running.addr);
        running.cancel.cancel();
        let abort = running.serve_task.abort_handle();
        if tokio::time::timeout(Duration::from_secs(5), running.serve_task)
            .await
            .is_err()
        {
            abort.abort();
        }
    }
}

impl Default for WebhookServerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const SECRET: &str = "pxwh_test_secret";

    struct MockBackend {
        enabled: bool,
        fired: Mutex<Vec<(String, Value)>>,
    }

    #[async_trait]
    impl WebhookBackend for MockBackend {
        async fn lookup(&self, trigger_id: &str) -> Result<Option<WebhookTarget>> {
            Ok((trigger_id == "t1").then(|| WebhookTarget {
                secret_hash: hash_secret(SECRET),
                enabled: self.enabled,
            }))
        }

        async fn fire(&self, trigger_id: &str, payload: Value) -> WebhookFireOutcome {
            self.fired
                .lock()
                .unwrap()
                .push((trigger_id.to_string(), payload));
            WebhookFireOutcome::Accepted
        }
    }

    fn backend(enabled: bool) -> Arc<MockBackend> {
        Arc::new(MockBackend {
            enabled,
            fired: Mutex::new(Vec::new()),
        })
    }

    fn headers(auth: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(a) = auth {
            h.insert(header::AUTHORIZATION, HeaderValue::from_str(a).unwrap());
        }
        h
    }

    async fn call(b: &Arc<MockBackend>, id: &str, auth: Option<&str>, body: &[u8]) -> StatusCode {
        let backend: Backend = b.clone();
        handle_hook(
            State(backend),
            Path(id.to_string()),
            headers(auth),
            Bytes::copy_from_slice(body),
        )
        .await
        .status()
    }

    #[tokio::test]
    async fn good_secret_fires_with_payload() {
        let b = backend(true);
        let auth = format!("Bearer {}", SECRET);
        let status = call(&b, "t1", Some(&auth), br#"{"a":1}"#).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let fired = b.fired.lock().unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].1, json!({"a": 1}));
    }

    #[tokio::test]
    async fn bad_and_missing_secret_are_rejected() {
        let b = backend(true);
        assert_eq!(
            call(&b, "t1", Some("Bearer wrong"), b"{}").await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(call(&b, "t1", None, b"{}").await, StatusCode::UNAUTHORIZED);
        assert_eq!(
            call(&b, "t1", Some(SECRET), b"{}").await,
            StatusCode::UNAUTHORIZED,
            "token without Bearer scheme must be rejected"
        );
        assert!(b.fired.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unknown_trigger_is_not_found() {
        let b = backend(true);
        let auth = format!("Bearer {}", SECRET);
        let status = call(&b, "nope", Some(&auth), b"{}").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn oversized_body_is_rejected() {
        let b = backend(true);
        let big = vec![b' '; WEBHOOK_MAX_BODY_BYTES + 1];
        let auth = format!("Bearer {}", SECRET);
        let status = call(&b, "t1", Some(&auth), &big).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(b.fired.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn disabled_trigger_and_invalid_json() {
        let auth = format!("Bearer {}", SECRET);
        let auth = auth.as_str();
        assert_eq!(
            call(&backend(false), "t1", Some(auth), b"{}").await,
            StatusCode::CONFLICT
        );
        let b = backend(true);
        assert_eq!(
            call(&b, "t1", Some(auth), b"not json").await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(call(&b, "t1", Some(auth), b"").await, StatusCode::ACCEPTED);
        assert_eq!(b.fired.lock().unwrap()[0].1, Value::Null);
    }

    #[tokio::test]
    async fn router_enforces_body_limit_over_http() {
        let b = backend(true);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cancel = CancellationToken::new();
        let shutdown = cancel.clone();
        let backend: Backend = b.clone();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router(backend))
                .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
                .await;
        });

        let client = reqwest::Client::new();
        let url = format!("http://{}/hooks/t1", addr);
        let ok = client
            .post(&url)
            .bearer_auth(SECRET)
            .body(r#"{"event":"push"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status().as_u16(), 202);
        let too_big = client
            .post(&url)
            .bearer_auth(SECRET)
            .body(vec![b'a'; WEBHOOK_MAX_BODY_BYTES + 10])
            .send()
            .await
            .unwrap();
        assert_eq!(too_big.status().as_u16(), 413);

        cancel.cancel();
        let _ = server.await;
        assert_eq!(b.fired.lock().unwrap().len(), 1);
    }

    #[test]
    fn secrets_are_unique_and_hash_is_stable() {
        let a = generate_secret();
        let b = generate_secret();
        assert_ne!(a, b);
        assert!(a.starts_with(SECRET_PREFIX));
        assert_eq!(hash_secret(&a), hash_secret(&a));
        assert_eq!(hash_secret(&a).len(), 64);
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
