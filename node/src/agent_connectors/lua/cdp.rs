use anyhow::{Result, anyhow};
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::input::InsertTextParams;
use chromiumoxide::page::Page;
use futures::StreamExt;
use mlua::{Lua, LuaSerdeExt, Table};
use once_cell::sync::Lazy;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::Message;

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn request_shutdown() {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

fn check_shutdown() -> Result<()> {
    if SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
        Err(anyhow!("shutdown requested"))
    } else {
        Ok(())
    }
}

//
// Raw WebSocket CDP client for Node.js inspector protocol.
// Sends Runtime.evaluate and returns the result.
//

struct RawCdpWs {
    write: Arc<
        Mutex<
            futures::stream::SplitSink<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                >,
                Message,
            >,
        >,
    >,
    read: Arc<
        tokio::sync::Mutex<
            futures::stream::SplitStream<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                >,
            >,
        >,
    >,
    next_id: AtomicU64,
}

impl RawCdpWs {
    async fn connect(url: &str) -> Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| anyhow!("WebSocket connect failed: {}", e))?;
        let (write, read) = futures::StreamExt::split(ws);
        Ok(Self {
            write: Arc::new(Mutex::new(write)),
            read: Arc::new(tokio::sync::Mutex::new(read)),
            next_id: AtomicU64::new(1),
        })
    }

    async fn evaluate(&self, expression: &str) -> Result<serde_json::Value> {
        use futures::SinkExt;

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({
            "id": id,
            "method": "Runtime.evaluate",
            "params": {
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true,
            }
        });

        {
            let mut write = self.write.lock().unwrap();
            block_on(write.send(Message::Text(msg.to_string().into())))
                .map_err(|e| anyhow!("WS send failed: {}", e))?;
        }

        //
        // Read messages until we find the response matching our id.
        //

        let mut read = self.read.lock().await;
        loop {
            let msg = futures::StreamExt::next(&mut *read)
                .await
                .ok_or_else(|| anyhow!("WebSocket closed"))??;

            if let Message::Text(text) = msg {
                let resp: serde_json::Value = serde_json::from_str(&text)?;
                if resp.get("id").and_then(|v| v.as_u64()) == Some(id) {
                    if let Some(err) = resp.get("error") {
                        return Err(anyhow!("CDP error: {}", err));
                    }
                    let result = resp
                        .get("result")
                        .and_then(|r| r.get("result"))
                        .and_then(|r| r.get("value"))
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    return Ok(result);
                }
            }
        }
    }

    async fn close(&self) -> Result<()> {
        use futures::SinkExt;
        let mut write = self.write.lock().unwrap();
        let _ = block_on(write.close());
        Ok(())
    }
}

enum CdpBackend {
    Chrome(Page),
    NodeInspector(RawCdpWs),
}

struct CdpConnection {
    backend: CdpBackend,
    process_id: Option<u32>,
}

static CDP_CONNECTIONS: Lazy<Mutex<HashMap<String, CdpConnection>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn lua_err(e: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::RuntimeError(e.to_string())
}

//
// Block on an async future from synchronous Lua context.
//

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(f))
}

//
// Spawn a process with DevTools debugging enabled, wait for the endpoint to
// become available, and connect via chromiumoxide.
//
// Supports two modes:
//   - Environment variable (WebView2): set debug_port_env_var + debug_port_format
//   - CLI argument (Electron): set debug_port_cli_arg
//
// Config table fields:
//   process_path        (string)  Path to the executable
//   debug_port_env_var  (string?) Env var name for the debug port argument
//   debug_port_format   (string?) Format string, e.g. "--remote-debugging-port={}"
//   debug_port_cli_arg  (string?) CLI arg format string, e.g. "--remote-debugging-port={}"
//   base_port           (number)  Base port number
//   port_range          (number)  Range for random port selection
//   kill_existing       (bool?)   Whether to kill existing processes first
//   use_hidden_desktop  (bool?)   Spawn on hidden desktop (Windows, default true)
//

fn cdp_spawn_and_connect(config: &Table) -> Result<(String, Option<String>)> {
    let process_path: String = config
        .get("process_path")
        .map_err(|e| anyhow!("missing process_path: {}", e))?;
    let debug_port_env_var: Option<String> = config.get("debug_port_env_var").unwrap_or(None);
    let debug_port_format: Option<String> = config.get("debug_port_format").unwrap_or(None);
    let debug_port_cli_arg: Option<String> = config.get("debug_port_cli_arg").unwrap_or(None);
    let base_port: u16 = config
        .get("base_port")
        .map_err(|e| anyhow!("missing base_port: {}", e))?;
    let port_range: u16 = config.get("port_range").unwrap_or(778);
    let kill_existing = config
        .get::<Option<bool>>("kill_existing")
        .unwrap_or(None)
        .unwrap_or(true);
    let use_hidden_desktop = config
        .get::<Option<bool>>("use_hidden_desktop")
        .unwrap_or(None)
        .unwrap_or(true);

    //
    // Determine debug port delivery mode: env var or CLI arg.
    //

    let use_env = debug_port_env_var.is_some() && debug_port_format.is_some();
    let use_cli = debug_port_cli_arg.is_some();

    if !use_env && !use_cli {
        return Err(anyhow!(
            "must provide either debug_port_env_var+debug_port_format or debug_port_cli_arg"
        ));
    }

    //
    // Close all existing CDP connections and terminate their process trees,
    // then kill any remaining processes with the same name.
    //

    if kill_existing {
        close_all_connections();

        if let Some(process_name) = std::path::Path::new(&process_path).file_name() {
            if let Some(name) = process_name.to_str() {
                crate::utils::kill_processes_by_name(name);
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }

    let port = base_port + (rand::random::<u16>() % port_range);
    common::log_info!("CDP: using port {}", port);

    let port_str = port.to_string();

    //
    // Build env var and CLI arg vectors based on the selected mode.
    //

    let env_pair: Option<(String, String)> = if use_env {
        let fmt = debug_port_format.as_ref().unwrap();
        Some((
            debug_port_env_var.as_ref().unwrap().clone(),
            fmt.replace("{}", &port_str),
        ))
    } else {
        None
    };

    let cli_args: Vec<String> = if use_cli {
        let fmt = debug_port_cli_arg.as_ref().unwrap();
        vec![fmt.replace("{}", &port_str)]
    } else {
        vec![]
    };

    let spawn_result = crate::utils::spawn_process_detached(
        &process_path,
        env_pair.as_ref().map(|(k, v)| (k.as_str(), v.as_str())),
        &cli_args.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        use_hidden_desktop,
    )?;

    let pid = spawn_result.pid;

    //
    // Store the hidden desktop handle in the shared registry so Lua can
    // manage its lifetime via the session state.
    //

    #[cfg(windows)]
    let desktop_id: Option<String> = {
        spawn_result.hidden_desktop.map(|d| {
            let id = uuid::Uuid::new_v4().to_string();
            super::runtime::store_desktop_handle(&id, d);
            id
        })
    };

    #[cfg(not(windows))]
    let desktop_id: Option<String> = None;

    let ws_url = block_on(discover_ws_url(port))?;
    let page = block_on(connect_to_devtools_chrome(&ws_url))?;

    let handle = uuid::Uuid::new_v4().to_string();
    let mut map = CDP_CONNECTIONS.lock().unwrap();
    map.insert(
        handle.clone(),
        CdpConnection {
            backend: CdpBackend::Chrome(page),
            process_id: Some(pid),
        },
    );

    common::log_info!("CDP: connected, handle={}", handle);
    Ok((handle, desktop_id))
}

//
// Connect to an already-running DevTools endpoint.
//

fn cdp_connect(port: u16) -> Result<String> {
    let ws_url = block_on(discover_ws_url(port))?;
    common::log_info!("CDP: connecting to {}", ws_url);

    let backend = block_on(async {
        //
        // Try chromiumoxide first (Chrome/WebView2 with pages). If it fails
        // to find pages, fall back to raw WebSocket (Node.js inspector).
        //

        if let Ok(page) = connect_to_devtools_chrome(&ws_url).await {
            return Ok(CdpBackend::Chrome(page));
        }

        common::log_info!("CDP: no pages found, using raw WebSocket for Node.js inspector");
        let raw = RawCdpWs::connect(&ws_url).await?;
        Ok::<_, anyhow::Error>(CdpBackend::NodeInspector(raw))
    })?;

    let handle = uuid::Uuid::new_v4().to_string();
    let mut map = CDP_CONNECTIONS.lock().unwrap();
    map.insert(
        handle.clone(),
        CdpConnection {
            backend,
            process_id: None,
        },
    );

    Ok(handle)
}

//
// Discover the WebSocket debugger URL by polling /json/version (Chrome) and
// /json (Node.js) until the endpoint responds.
//

async fn discover_ws_url(port: u16) -> Result<String> {
    let base_url = format!("http://127.0.0.1:{}", port);
    let max_attempts = 5;

    for attempt in 0..max_attempts {
        check_shutdown()?;
        common::log_debug!(
            "CDP: discovery attempt {}/{} on port {}",
            attempt + 1,
            max_attempts,
            port
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let version_url = format!("{}/json/version", base_url);
        if let Ok(response) = reqwest::get(&version_url).await {
            if let Ok(body) = response.json::<serde_json::Value>().await {
                if let Some(url) = body.get("webSocketDebuggerUrl").and_then(|v| v.as_str()) {
                    return Ok(url.to_string());
                }
            }
        }

        let list_url = format!("{}/json", base_url);
        if let Ok(response) = reqwest::get(&list_url).await {
            if let Ok(body) = response.json::<serde_json::Value>().await {
                if let Some(arr) = body.as_array() {
                    for target in arr {
                        if let Some(url) =
                            target.get("webSocketDebuggerUrl").and_then(|v| v.as_str())
                        {
                            return Ok(url.to_string());
                        }
                    }
                }
            }
        }
    }

    Err(anyhow!(
        "DevTools not available after {} attempts",
        max_attempts
    ))
}

//
// Connect via chromiumoxide (Chrome/WebView2 with pages).
//

async fn connect_to_devtools_chrome(ws_url: &str) -> Result<Page> {
    let (browser, mut handler) = Browser::connect(ws_url).await?;

    tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if let Err(e) = event {
                let err_str = e.to_string();
                if !err_str.contains("ResetWithoutClosingHandshake")
                    && !err_str.contains("Connection reset")
                    && !err_str.contains("did not match any variant")
                {
                    common::log_error!("CDP: browser handler error: {}", e);
                }
            }
        }
    });

    for attempt in 0..3 {
        check_shutdown()?;
        let pages = browser.pages().await?;
        if let Some(page) = pages.into_iter().next() {
            return Ok(page);
        }
        common::log_debug!("CDP: no pages yet, attempt {}/3", attempt + 1);
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    }

    Err(anyhow!("No pages found"))
}

fn with_connection<F, R>(handle: &str, f: F) -> Result<R>
where
    F: FnOnce(&CdpConnection) -> Result<R>,
{
    let map = CDP_CONNECTIONS.lock().unwrap();
    let conn = map
        .get(handle)
        .ok_or_else(|| anyhow!("CDP handle not found: {}", handle))?;
    f(conn)
}

fn require_page(conn: &CdpConnection) -> Result<&Page> {
    match &conn.backend {
        CdpBackend::Chrome(page) => Ok(page),
        CdpBackend::NodeInspector(_) => Err(anyhow!(
            "operation not supported on Node.js inspector connection"
        )),
    }
}

fn cdp_evaluate(handle: &str, js: &str) -> Result<serde_json::Value> {
    with_connection(handle, |conn| {
        let js = js.to_string();
        match &conn.backend {
            CdpBackend::Chrome(page) => block_on(async {
                let result = page.evaluate(js).await?;
                match result.into_value::<serde_json::Value>() {
                    Ok(v) => Ok(v),
                    Err(_) => Ok(serde_json::Value::Null),
                }
            }),
            CdpBackend::NodeInspector(raw) => block_on(raw.evaluate(&js)),
        }
    })
}

fn cdp_find_elements(handle: &str, selector: &str) -> Result<usize> {
    with_connection(handle, |conn| {
        let page = require_page(conn)?;
        let selector = selector.to_string();
        block_on(async {
            let elements = page.find_elements(selector).await.unwrap_or_default();
            Ok(elements.len())
        })
    })
}

fn cdp_click(handle: &str, selector: &str) -> Result<()> {
    with_connection(handle, |conn| {
        let page = require_page(conn)?;
        let selector = selector.to_string();
        block_on(async {
            let element = page
                .find_element(selector)
                .await
                .map_err(|e| anyhow!("element not found: {}", e))?;
            element
                .click()
                .await
                .map_err(|e| anyhow!("click failed: {}", e))?;
            Ok(())
        })
    })
}

fn cdp_type_text(handle: &str, text: &str) -> Result<()> {
    with_connection(handle, |conn| {
        let page = require_page(conn)?;
        let text = text.to_string();
        block_on(async {
            page.execute(InsertTextParams::new(text))
                .await
                .map_err(|e| anyhow!("InsertText failed: {}", e))?;
            Ok(())
        })
    })
}

fn cdp_press_key(handle: &str, selector: &str, key: &str) -> Result<()> {
    with_connection(handle, |conn| {
        let page = require_page(conn)?;
        let selector = selector.to_string();
        let key = key.to_string();
        block_on(async {
            let element = page
                .find_element(selector)
                .await
                .map_err(|e| anyhow!("element not found: {}", e))?;
            element
                .press_key(&key)
                .await
                .map_err(|e| anyhow!("press_key failed: {}", e))?;
            Ok(())
        })
    })
}

fn cdp_wait_for_element(handle: &str, selector: &str, retries: u32, delay_ms: u64) -> Result<bool> {
    with_connection(handle, |conn| {
        let page = require_page(conn)?;
        let selector = selector.to_string();
        block_on(async {
            for _ in 0..retries {
                check_shutdown()?;
                if page.find_element(&selector).await.is_ok() {
                    return Ok(true);
                }
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            Ok(false)
        })
    })
}

fn close_all_connections() {
    let mut map = CDP_CONNECTIONS.lock().unwrap();
    let handles: Vec<String> = map.keys().cloned().collect();
    for handle in &handles {
        if let Some(conn) = map.remove(handle) {
            if let CdpBackend::NodeInspector(raw) = &conn.backend {
                let _ = block_on(raw.close());
            }
            if let Some(pid) = conn.process_id {
                common::log_info!("CDP: closing previous connection, terminating PID {}", pid);
                crate::utils::terminate_process_tree(pid);
            }
        }
    }
}

fn cdp_close(handle: &str) -> Result<()> {
    cleanup_connection(handle);
    Ok(())
}

//
// Remove a CDP connection from the map and terminate its process tree.
// Callable from Rust (session close/drop safety net) without going through Lua.
//

pub fn cleanup_connection(handle: &str) {
    let mut map = CDP_CONNECTIONS.lock().unwrap();
    if let Some(conn) = map.remove(handle) {
        if let CdpBackend::NodeInspector(raw) = &conn.backend {
            let _ = block_on(raw.close());
        }
        if let Some(pid) = conn.process_id {
            common::log_info!("CDP: terminating process tree for PID {}", pid);
            crate::utils::terminate_process_tree(pid);
        }
    }
}

fn cdp_process_id(handle: &str) -> Result<Option<u32>> {
    let map = CDP_CONNECTIONS.lock().unwrap();
    Ok(map.get(handle).and_then(|c| c.process_id))
}

//
// Register all CDP functions on the praxis global table.
//

pub fn install_cdp_api(lua: &Lua, praxis: &Table) -> Result<()> {
    praxis
        .set(
            "cdp_spawn_and_connect",
            lua.create_function(|lua, config: Table| {
                let (handle, desktop_id) = cdp_spawn_and_connect(&config).map_err(lua_err)?;
                let tbl = lua.create_table().map_err(lua_err)?;
                tbl.set("handle", handle).map_err(lua_err)?;
                tbl.set("desktop", desktop_id).map_err(lua_err)?;
                Ok(tbl)
            })
            .map_err(|e| anyhow!(e.to_string()))?,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

    praxis
        .set(
            "cdp_connect",
            lua.create_function(|_, port: u16| cdp_connect(port).map_err(lua_err))
                .map_err(|e| anyhow!(e.to_string()))?,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

    praxis
        .set(
            "cdp_evaluate",
            lua.create_function(|lua, (handle, js): (String, String)| {
                let result = cdp_evaluate(&handle, &js).map_err(lua_err)?;
                lua.to_value(&result)
            })
            .map_err(|e| anyhow!(e.to_string()))?,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

    praxis
        .set(
            "cdp_find_elements",
            lua.create_function(|_, (handle, selector): (String, String)| {
                cdp_find_elements(&handle, &selector).map_err(lua_err)
            })
            .map_err(|e| anyhow!(e.to_string()))?,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

    praxis
        .set(
            "cdp_click",
            lua.create_function(|_, (handle, selector): (String, String)| {
                cdp_click(&handle, &selector).map_err(lua_err)
            })
            .map_err(|e| anyhow!(e.to_string()))?,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

    praxis
        .set(
            "cdp_type_text",
            lua.create_function(|_, (handle, text): (String, String)| {
                cdp_type_text(&handle, &text).map_err(lua_err)
            })
            .map_err(|e| anyhow!(e.to_string()))?,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

    praxis
        .set(
            "cdp_press_key",
            lua.create_function(|_, (handle, selector, key): (String, String, String)| {
                cdp_press_key(&handle, &selector, &key).map_err(lua_err)
            })
            .map_err(|e| anyhow!(e.to_string()))?,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

    praxis
        .set(
            "cdp_wait_for_element",
            lua.create_function(
                |_, (handle, selector, retries, delay_ms): (String, String, u32, u64)| {
                    cdp_wait_for_element(&handle, &selector, retries, delay_ms).map_err(lua_err)
                },
            )
            .map_err(|e| anyhow!(e.to_string()))?,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

    praxis
        .set(
            "cdp_close",
            lua.create_function(|_, handle: String| cdp_close(&handle).map_err(lua_err))
                .map_err(|e| anyhow!(e.to_string()))?,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

    praxis
        .set(
            "cdp_process_id",
            lua.create_function(|_, handle: String| cdp_process_id(&handle).map_err(lua_err))
                .map_err(|e| anyhow!(e.to_string()))?,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

    Ok(())
}
