use super::*;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

struct Fixture {
    root: std::path::PathBuf,
    port: u16,
    tasks: tokio::task::JoinSet<()>,
    traffic: mpsc::Receiver<InterceptedTrafficEntry>,
    tls_errors: mpsc::Receiver<String>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.tasks.abort_all();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn response_events() -> [Value; 3] {
    [
        json!({"type":"response.created","response":{"id":"fixture-response"}}),
        json!({"type":"response.output_item.done","item":{
            "type":"message","role":"assistant","id":"fixture-message",
            "content":[{"type":"output_text","text":"praxis-codex-ok"}]
        }}),
        json!({"type":"response.completed","response":{"id":"fixture-response",
            "usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}
        }}),
    ]
}

async fn origin<S>(stream: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(stream);
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.unwrap() == 0 {
            return;
        }
        headers.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    let auth_path = headers.split_whitespace().nth(1).unwrap().to_string();
    let is_login = auth_path.starts_with("/api/accounts/") || auth_path == "/oauth/token";
    assert!(
        is_login
            || headers
                .to_ascii_lowercase()
                .contains("authorization: bearer praxis-fixture-key")
    );
    if headers.to_ascii_lowercase().contains("upgrade: websocket") {
        let stream = PrefixedStream::new(headers.into_bytes(), reader);
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        while let Some(Ok(message)) = ws.next().await {
            if message.is_close() {
                break;
            }
            if !message.is_text() {
                continue;
            }
            for event in response_events() {
                if ws
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        event.to_string().into(),
                    ))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    } else {
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).await.unwrap();
        let (content_type, response) = if is_login {
            let response = match auth_path.as_str() {
                "/api/accounts/deviceauth/usercode" => json!({
                    "device_auth_id":"fixture-device", "user_code":"TEST-CODE", "interval":"0"
                }),
                "/api/accounts/deviceauth/token" => json!({
                    "authorization_code":"fixture-code", "code_challenge":"fixture-challenge",
                    "code_verifier":"fixture-verifier"
                }),
                "/oauth/token" => {
                    use base64::Engine;
                    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                        json!({"email":"fixture@example.invalid", "https://api.openai.com/auth":{
                            "chatgpt_account_id":"fixture-account", "chatgpt_plan_type":"plus"
                        }})
                        .to_string(),
                    );
                    json!({"id_token":format!("e30.{payload}.fixture"),
                        "access_token":"fixture-access", "refresh_token":"fixture-refresh"})
                }
                _ => panic!("Unexpected login path: {auth_path}"),
            };
            ("application/json", response.to_string())
        } else if headers.starts_with("POST ") {
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert!(request.get("input").is_some());
            (
                "text/event-stream",
                response_events()
                    .iter()
                    .map(|event| {
                        format!(
                            "event: {}\ndata: {}\n\n",
                            event["type"].as_str().unwrap(),
                            event
                        )
                    })
                    .collect::<String>(),
            )
        } else {
            ("application/json", "{\"models\":[]}".to_string())
        };
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        );
        reader.get_mut().write_all(reply.as_bytes()).await.unwrap();
        reader.get_mut().shutdown().await.unwrap();
    }
}

impl Fixture {
    async fn start() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let root = std::env::temp_dir().join(format!("praxis-codex-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("home")).unwrap();
        let mut ca = CertificateAuthority::new().unwrap();
        std::fs::write(root.join("ca.pem"), ca.root_cert_pem()).unwrap();
        let cert = ca.generate_leaf_cert("localhost").unwrap();
        let acceptor = create_tls_acceptor_from_pem(&cert.cert_pem, &cert.key_pem).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut std::io::Cursor::new(ca.root_cert_pem())) {
            roots.add(cert.unwrap()).unwrap();
        }
        let connector = tokio_rustls::TlsConnector::from(Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        ));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, traffic) = mpsc::channel(128);
        let (tls_error_tx, tls_errors) = mpsc::channel(16);
        let sink = TrafficSink::new(tx);
        let config = Arc::new(ProxyConfig {
            intercept_domains: Arc::new(RwLock::new(HashSet::from(["localhost".into()]))),
            domain_capture_configs: HashMap::from([(
                "localhost".into(),
                DomainCaptureConfig {
                    agent_rules: vec![AgentCaptureRule {
                        agent_short_name: "codex".into(),
                        url_pattern: None,
                    }],
                },
            )]),
            node_id: "fixture-node".into(),
            intercept_method: InterceptMethod::Proxy,
            domain_to_real_ip: HashMap::new(),
        });
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    result = connections.join_next(), if !connections.is_empty() => {
                        result.unwrap().unwrap();
                    }
                    incoming = listener.accept() => {
                        let (socket, _) = incoming.unwrap();
                        let acceptor = acceptor.clone();
                        let connector = connector.clone();
                        let config = config.clone();
                        let sink = sink.clone();
                        let tls_error_tx = tls_error_tx.clone();
                        connections.spawn(async move {
                            let client_tls = match acceptor.accept(socket).await {
                                Ok(stream) => stream,
                                Err(error) => {
                                    let _ = tls_error_tx.try_send(error.to_string());
                                    return;
                                }
                            };
                            let (outgoing, incoming) = tokio::io::duplex(1024 * 1024);
                            let origin = async {
                                let tls = acceptor.accept(incoming).await.unwrap();
                                origin(tls).await;
                            };
                            let relay = async {
                                let server_tls = connector.connect(
                                    rustls_pki_types::ServerName::try_from("localhost").unwrap(), outgoing
                                ).await.unwrap();
                                let _ = proxy_https_traffic(client_tls, server_tls, "localhost", &config, &sink).await;
                            };
                            tokio::join!(origin, relay);
                        });
                    }
                }
            }
        });
        Self {
            root,
            port,
            tasks,
            traffic,
            tls_errors,
        }
    }

    fn command(&self, websocket: bool) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(
            std::env::var_os("PRAXIS_CODEX_BIN").unwrap_or_else(|| "codex".into()),
        );
        command
            .current_dir(&self.root)
            .kill_on_drop(true)
            .env("CODEX_HOME", self.root.join("home"))
            .env("CODEX_CA_CERTIFICATE", self.root.join("ca.pem"))
            .env("PRAXIS_FIXTURE_KEY", "praxis-fixture-key")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .env_remove("CODEX_ACCESS_TOKEN")
            .env_remove("SSL_CERT_FILE");
        for name in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ] {
            command.env_remove(name);
        }
        for setting in [
            "model_provider=\"fixture\"".to_string(),
            "model=\"gpt-5\"".into(),
            "web_search=\"disabled\"".into(),
            "model_providers.fixture.name=\"fixture\"".into(),
            format!(
                "model_providers.fixture.base_url=\"https://localhost:{}/v1\"",
                self.port
            ),
            "model_providers.fixture.env_key=\"PRAXIS_FIXTURE_KEY\"".into(),
            "model_providers.fixture.wire_api=\"responses\"".into(),
            format!("model_providers.fixture.supports_websockets={websocket}"),
            "model_providers.fixture.request_max_retries=0".into(),
            "model_providers.fixture.stream_max_retries=0".into(),
        ] {
            command.args(["-c", &setting]);
        }
        command
    }

    async fn assert_capture(&mut self, websocket: bool) {
        let entries = timeout(Duration::from_secs(5), async {
            let mut captured = Vec::new();
            loop {
                let entry = self.traffic.recv().await.unwrap();
                let complete =
                    String::from_utf8_lossy(entry.response_body.as_deref().unwrap_or_default())
                        .contains("praxis-codex-ok");
                captured.push(entry);
                if complete {
                    break;
                }
            }
            captured
        })
        .await
        .expect("Codex response should be captured");
        assert!(
            entries
                .iter()
                .all(|entry| entry.agent_short_name == "codex")
        );
        if websocket {
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.response_status == Some(101))
            );
        }
        assert!(!self.tasks.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an installed Codex CLI; uses isolated local TLS fixtures and no live credentials"]
async fn codex_cli_https_and_websocket_capture() {
    for websocket in [false, true] {
        let mut fixture = Fixture::start().await;
        let output = timeout(
            Duration::from_secs(30),
            fixture
                .command(websocket)
                .args([
                    "exec",
                    "--skip-git-repo-check",
                    "--sandbox",
                    "read-only",
                    "Say praxis-codex-ok.",
                ])
                .output(),
        )
        .await
        .expect("Codex CLI timed out")
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("praxis-codex-ok"));
        fixture.assert_capture(websocket).await;
    }
}

async fn rpc_response(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    id: u64,
) -> Value {
    loop {
        let line = lines.next_line().await.unwrap().expect("app-server exited");
        let message: Value = serde_json::from_str(&line).unwrap();
        if message["id"] == id {
            assert!(message.get("error").is_none(), "{message}");
            return message["result"].clone();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an installed Codex CLI; uses isolated local TLS fixtures and no live credentials"]
async fn codex_app_server_https_and_websocket_capture() {
    for websocket in [false, true] {
        let mut fixture = Fixture::start().await;
        let mut child = fixture
            .command(websocket)
            .arg("app-server")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        timeout(Duration::from_secs(30), async {
            let initialize = json!({"id":1,"method":"initialize","params":{
                "clientInfo":{"name":"praxis-fixture","version":"1.0"}
            }});
            input
                .write_all(format!("{initialize}\n").as_bytes())
                .await
                .unwrap();
            rpc_response(&mut lines, 1).await;
            input
                .write_all(b"{\"method\":\"initialized\"}\n")
                .await
                .unwrap();
            let start = json!({"id":2,"method":"thread/start","params":{
                "cwd":fixture.root,"sandbox":"read-only","approvalPolicy":"never"
            }});
            input
                .write_all(format!("{start}\n").as_bytes())
                .await
                .unwrap();
            let thread = rpc_response(&mut lines, 2).await;
            let turn = json!({"id":3,"method":"turn/start","params":{
                "threadId":thread["thread"]["id"],
                "input":[{"type":"text","text":"Say praxis-codex-ok."}]
            }});
            input
                .write_all(format!("{turn}\n").as_bytes())
                .await
                .unwrap();
            let mut received_message = false;
            loop {
                let line = lines.next_line().await.unwrap().expect("app-server exited");
                let message: Value = serde_json::from_str(&line).unwrap();
                assert!(message.get("error").is_none(), "{message}");
                if message["method"] == "item/completed"
                    && message["params"]["item"]["text"] == "praxis-codex-ok"
                {
                    received_message = true;
                }
                if message["method"] == "turn/completed" {
                    assert_eq!(
                        message["params"]["turn"]["status"], "completed",
                        "{message}"
                    );
                    assert!(received_message);
                    break;
                }
            }
            fixture.assert_capture(websocket).await;
        })
        .await
        .expect("app-server test timed out");
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an installed Codex CLI; uses isolated local TLS fixtures and no live credentials"]
async fn codex_rejects_intercept_ca_without_configuration() {
    let mut fixture = Fixture::start().await;
    let mut child = fixture
        .command(false)
        .env_remove("CODEX_CA_CERTIFICATE")
        .args([
            "exec",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "Say praxis-codex-ok.",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let error = timeout(Duration::from_secs(10), fixture.tls_errors.recv())
        .await
        .expect("Codex did not reject the untrusted certificate")
        .unwrap();
    assert!(
        error.contains("UnknownCA") || error.contains("CertificateUnknown"),
        "{error}"
    );
    assert!(fixture.traffic.try_recv().is_err());
    child.kill().await.unwrap();
    child.wait().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an installed Codex CLI; uses isolated local TLS fixtures and no live credentials"]
async fn codex_device_login_survives_interception() {
    let mut fixture = Fixture::start().await;
    let issuer = format!("https://localhost:{}", fixture.port);
    let output = timeout(
        Duration::from_secs(30),
        fixture
            .command(false)
            .args([
                "-c",
                "cli_auth_credentials_store=\"file\"",
                "login",
                "--device-auth",
                "--experimental_issuer",
                &issuer,
            ])
            .output(),
    )
    .await
    .expect("Codex login timed out")
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let auth: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.root.join("home/auth.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(auth["tokens"]["access_token"], "fixture-access");
    assert_eq!(auth["tokens"]["refresh_token"], "fixture-refresh");
    let mut paths = HashSet::new();
    timeout(Duration::from_secs(5), async {
        while paths.len() < 3 {
            let entry = fixture.traffic.recv().await.unwrap();
            if entry.response_status == Some(200) {
                assert_eq!(entry.agent_short_name, "codex");
                paths.insert(entry.url);
            }
        }
    })
    .await
    .expect("Login exchange should be captured");
    assert!(paths.iter().any(|path| path.ends_with("/oauth/token")));
}
