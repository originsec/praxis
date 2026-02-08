use anyhow::{anyhow, Result};
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::input::InsertTextParams;
use chromiumoxide::page::Page;
use futures::StreamExt;
use mlua::{Lua, LuaSerdeExt, Table};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

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

struct CdpConnection {
    page: Page,
    process_id: Option<u32>,
    #[cfg(windows)]
    _hidden_desktop: Option<crate::utils::HiddenDesktop>,
}

static CDP_CONNECTIONS: Lazy<Mutex<HashMap<String, CdpConnection>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn lua_err(e: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::RuntimeError(e.to_string())
}

//
// Block on an async future from synchronous Lua context. Uses the same
// pattern as semantic_discover_internal_tools.
//

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(f))
}

//
// Spawn a process with a debug port environment variable, wait for the
// DevTools endpoint to become available, and connect via chromiumoxide.
//
// Config table fields:
//   process_path        (string)  Path to the executable
//   debug_port_env_var  (string)  Env var name for the debug port argument
//   debug_port_format   (string)  Format string, e.g. "--remote-debugging-port={}"
//   base_port           (number)  Base port number
//   port_range          (number)  Range for random port selection
//   kill_existing       (bool?)   Whether to kill existing processes first
//   use_hidden_desktop  (bool?)   Spawn on hidden desktop (Windows, default true)
//

fn cdp_spawn_and_connect(config: &Table) -> Result<String> {
    let process_path: String = config
        .get("process_path")
        .map_err(|e| anyhow!("missing process_path: {}", e))?;
    let debug_port_env_var: String = config
        .get("debug_port_env_var")
        .map_err(|e| anyhow!("missing debug_port_env_var: {}", e))?;
    let debug_port_format: String = config
        .get("debug_port_format")
        .map_err(|e| anyhow!("missing debug_port_format: {}", e))?;
    let base_port: u16 = config
        .get("base_port")
        .map_err(|e| anyhow!("missing base_port: {}", e))?;
    let port_range: u16 = config.get("port_range").unwrap_or(778);
    let kill_existing = config.get::<Option<bool>>("kill_existing").unwrap_or(None).unwrap_or(true);
    let use_hidden_desktop = config.get::<Option<bool>>("use_hidden_desktop").unwrap_or(None).unwrap_or(true);

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

    let debug_arg = debug_port_format.replace("{}", &port.to_string());

    //
    // On Windows, spawn on a hidden desktop if enabled and PRAXIS_NOT_HIDDEN
    // is not set. Otherwise spawn normally and minimize after connect.
    //

    #[cfg(windows)]
    let (pid, should_minimize, hidden_desktop) = {
        let not_hidden = std::env::var("PRAXIS_NOT_HIDDEN")
            .map(|v| v == "1")
            .unwrap_or(cfg!(debug_assertions));
        let want_hidden = use_hidden_desktop && !not_hidden;

        if !want_hidden {
            common::log_debug!(
                "CDP: hidden desktop disabled (use_hidden_desktop={}, not_hidden={})",
                use_hidden_desktop, not_hidden
            );
        }

        if want_hidden {
            let desktop = crate::utils::HiddenDesktop::new();
            if let Some(ref d) = desktop {
                let pid = crate::utils::spawn_on_hidden_desktop(
                    &process_path,
                    &debug_port_env_var,
                    &debug_arg,
                    &d.name,
                )?;
                common::log_info!("CDP: spawned on hidden desktop '{}' with PID {}", d.name, pid);
                (pid, false, desktop)
            } else {
                common::log_warn!("CDP: failed to create hidden desktop, spawning normally");
                let process = std::process::Command::new(&process_path)
                    .env(&debug_port_env_var, &debug_arg)
                    .spawn()
                    .map_err(|e| anyhow!("failed to spawn {}: {}", process_path, e))?;
                (process.id(), true, None)
            }
        } else {
            let process = std::process::Command::new(&process_path)
                .env(&debug_port_env_var, &debug_arg)
                .spawn()
                .map_err(|e| anyhow!("failed to spawn {}: {}", process_path, e))?;
            let pid = process.id();
            common::log_info!("CDP: spawned with PID {} (will minimize after connect)", pid);
            (pid, true, None)
        }
    };

    #[cfg(not(windows))]
    let pid = {
        let _ = use_hidden_desktop;
        let process = std::process::Command::new(&process_path)
            .env(&debug_port_env_var, &debug_arg)
            .spawn()
            .map_err(|e| anyhow!("failed to spawn {}: {}", process_path, e))?;
        let pid = process.id();
        common::log_info!("CDP: spawned with PID {}", pid);
        pid
    };

    let page = block_on(connect_to_devtools(port))?;

    //
    // Minimize the window after DevTools connects if not on a hidden desktop.
    //

    #[cfg(windows)]
    if should_minimize {
        if crate::utils::minimize_process_window(pid) {
            common::log_info!("CDP: minimized process window");
        }
    }

    let handle = uuid::Uuid::new_v4().to_string();
    let mut map = CDP_CONNECTIONS.lock().unwrap();
    map.insert(
        handle.clone(),
        CdpConnection {
            page,
            process_id: Some(pid),
            #[cfg(windows)]
            _hidden_desktop: hidden_desktop,
        },
    );

    common::log_info!("CDP: connected, handle={}", handle);
    Ok(handle)
}

//
// Connect to an already-running DevTools endpoint.
//

fn cdp_connect(port: u16) -> Result<String> {
    let page = block_on(connect_to_devtools(port))?;

    let handle = uuid::Uuid::new_v4().to_string();
    let mut map = CDP_CONNECTIONS.lock().unwrap();
    map.insert(
        handle.clone(),
        CdpConnection {
            page,
            process_id: None,
            #[cfg(windows)]
            _hidden_desktop: None,
        },
    );

    Ok(handle)
}

//
// Shared DevTools connection logic. Polls /json/version then connects via
// chromiumoxide. Same retry logic as GenericDevToolsSession::connect_to_devtools.
//

async fn connect_to_devtools(port: u16) -> Result<Page> {
    let ws_url = format!("http://127.0.0.1:{}", port);

    let max_attempts = 5;
    let mut connected = false;
    for attempt in 0..max_attempts {
        check_shutdown()?;
        common::log_debug!("CDP: connection attempt {}/{} to {}", attempt + 1, max_attempts, ws_url);
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let version_url = format!("http://127.0.0.1:{}/json/version", port);
        if let Ok(response) = reqwest::get(&version_url).await {
            if response.status().is_success() {
                connected = true;
                break;
            }
        }
    }

    if !connected {
        return Err(anyhow!(
            "DevTools endpoint not available after {} attempts", max_attempts
        ));
    }

    let (browser, mut handler) = Browser::connect(&ws_url).await?;

    tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if let Err(e) = event {
                let err_str = e.to_string();
                if err_str.contains("ResetWithoutClosingHandshake")
                    || err_str.contains("Connection reset")
                {
                    common::log_debug!("CDP: browser connection closed");
                } else if err_str.contains("did not match any variant") {
                    // Silently ignore — chromiumoxide doesn't recognize some CDP messages
                } else {
                    common::log_error!("CDP: browser handler error: {}", e);
                }
            }
        }
    });

    for attempt in 0..max_attempts {
        check_shutdown()?;
        let pages = browser.pages().await?;
        if let Some(page) = pages.into_iter().next() {
            return Ok(page);
        }
        common::log_debug!("CDP: no pages yet, attempt {}/{}", attempt + 1, max_attempts);
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    Err(anyhow!("No pages found in browser after {} attempts", max_attempts))
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

fn cdp_evaluate(handle: &str, js: &str) -> Result<serde_json::Value> {
    with_connection(handle, |conn| {
        let js = js.to_string();
        block_on(async {
            let result = conn.page.evaluate(js).await?;
            Ok(result.into_value()?)
        })
    })
}

fn cdp_find_elements(handle: &str, selector: &str) -> Result<usize> {
    with_connection(handle, |conn| {
        let selector = selector.to_string();
        block_on(async {
            let elements = conn.page.find_elements(selector).await.unwrap_or_default();
            Ok(elements.len())
        })
    })
}

fn cdp_click(handle: &str, selector: &str) -> Result<()> {
    with_connection(handle, |conn| {
        let selector = selector.to_string();
        block_on(async {
            let element = conn
                .page
                .find_element(selector)
                .await
                .map_err(|e| anyhow!("element not found: {}", e))?;
            element.click().await.map_err(|e| anyhow!("click failed: {}", e))?;
            Ok(())
        })
    })
}

fn cdp_type_text(handle: &str, text: &str) -> Result<()> {
    with_connection(handle, |conn| {
        let text = text.to_string();
        block_on(async {
            conn.page
                .execute(InsertTextParams::new(text))
                .await
                .map_err(|e| anyhow!("InsertText failed: {}", e))?;
            Ok(())
        })
    })
}

fn cdp_press_key(handle: &str, selector: &str, key: &str) -> Result<()> {
    with_connection(handle, |conn| {
        let selector = selector.to_string();
        let key = key.to_string();
        block_on(async {
            let element = conn
                .page
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
        let selector = selector.to_string();
        block_on(async {
            for _ in 0..retries {
                check_shutdown()?;
                if conn.page.find_element(&selector).await.is_ok() {
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
            lua.create_function(|_, config: Table| {
                cdp_spawn_and_connect(&config).map_err(lua_err)
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
