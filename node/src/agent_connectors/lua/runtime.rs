use anyhow::{Result, anyhow};
use mlua::{ChunkMode, Function, Lua, LuaSerdeExt, MultiValue, Table, Value};
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::utils::LockExt;
use common::{ReconResult, SessionContext};

static COMMAND_HANDLES: Lazy<std::sync::Mutex<HashMap<String, Arc<AtomicU32>>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

//
// Global reset flag. When set, all Lua VM execution is interrupted via
// set_hook and all blocking command waits are aborted.
//

static RESET_FLAG: AtomicBool = AtomicBool::new(false);

pub fn signal_reset() {
    RESET_FLAG.store(true, Ordering::SeqCst);
    abort_all_commands();
}

pub fn clear_reset() {
    RESET_FLAG.store(false, Ordering::SeqCst);
}

fn is_reset() -> bool {
    RESET_FLAG.load(Ordering::SeqCst)
}

fn abort_all_commands() {
    let map = COMMAND_HANDLES.lock_safe();
    for cell in map.values() {
        let pid = cell.load(Ordering::SeqCst);
        if pid != 0 {
            crate::utils::terminate_process_tree(pid);
            cell.store(0, Ordering::SeqCst);
        }
    }
}

//
// Cancellation registry for long-running Lua operations (e.g. transact poll
// loops). Keyed by an arbitrary string (typically cdp_handle). Set from Rust
// (abort_transaction/close), checked from Lua via praxis.is_cancelled(key).
//

static CANCEL_FLAGS: Lazy<std::sync::Mutex<HashMap<String, Arc<AtomicBool>>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

pub fn set_cancelled(key: &str) {
    let map = CANCEL_FLAGS.lock_safe();
    if let Some(flag) = map.get(key) {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn is_cancelled(key: &str) -> bool {
    let map = CANCEL_FLAGS.lock_safe();
    map.get(key)
        .map(|f| f.load(std::sync::atomic::Ordering::SeqCst))
        .unwrap_or(false)
}

fn register_cancel_flag(key: &str) {
    let mut map = CANCEL_FLAGS.lock_safe();
    map.insert(key.to_string(), Arc::new(AtomicBool::new(false)));
}

fn remove_cancel_flag(key: &str) {
    let mut map = CANCEL_FLAGS.lock_safe();
    map.remove(key);
}

fn lua_error<E: std::fmt::Display>(e: E) -> anyhow::Error {
    anyhow!(e.to_string())
}

#[derive(Debug, Default, Deserialize)]
struct CommandSpec {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    stdin: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    timeout_secs: Option<u64>,
}

//
// Create a Lua VM pre-initialized with the shared API, helper libraries, and
// the connector script loaded. The connector table is stored as the
// `_connector` global for subsequent calls.
//

pub fn create_vm(script: &str) -> Result<Lua> {
    let lua = init_vm_shell()?;
    let value: Value = lua.load(script).eval().map_err(lua_error)?;
    set_connector(&lua, value)?;
    Ok(lua)
}

//
// Initialize a fresh Lua VM with the reset hook, shared host API, and shared
// libraries but without a loaded connector script. Used by create_vm (which
// then evaluates source) and make_session_vm (which then evaluates bytecode).
//

fn init_vm_shell() -> Result<Lua> {
    let lua = Lua::new();

    //
    // Install a hook that fires every 1000 Lua instructions. When the global
    // reset flag is set, the hook returns an error which unwinds the Lua call
    // stack and returns control to Rust.
    //

    lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(1000),
        |_lua, _debug| {
            if is_reset() {
                Err(mlua::Error::RuntimeError("reset signal received".into()))
            } else {
                Ok(mlua::VmState::Continue)
            }
        },
    )
    .map_err(lua_error)?;

    install_shared_api(&lua)?;
    install_shared_libraries(&lua)?;
    Ok(lua)
}

fn set_connector(lua: &Lua, value: Value) -> Result<()> {
    match value {
        Value::Table(t) => {
            lua.globals().set("_connector", t).map_err(lua_error)?;
            Ok(())
        }
        _ => Err(anyhow!("Lua connector script must return a table")),
    }
}

//
// Compile a connector script to portable Lua bytecode. Done once per
// connector at load time. The returned bytes can be loaded repeatedly via
// make_session_vm to instantiate per-session VMs without re-parsing source.
//

pub fn compile_bytecode(script: &str) -> Result<Vec<u8>> {
    let lua = Lua::new();
    let func: Function = lua.load(script).into_function().map_err(lua_error)?;
    Ok(func.dump(true))
}

//
// Instantiate a fresh per-session Lua VM from precompiled bytecode. The
// returned VM has its own heap, host API bindings, and an evaluated
// _connector table independent of every other session's VM.
//

pub fn make_session_vm(bytecode: &[u8]) -> Result<Lua> {
    let lua = init_vm_shell()?;
    let value: Value = lua
        .load(bytecode)
        .set_mode(ChunkMode::Binary)
        .eval()
        .map_err(lua_error)?;
    set_connector(&lua, value)?;
    Ok(lua)
}

fn connector_table(lua: &Lua) -> Result<Table> {
    lua.globals().get("_connector").map_err(lua_error)
}

pub struct LuaManifest {
    pub name: String,
    pub short_name: String,
    pub has_recon: bool,
    pub has_fingerprint: bool,
    pub has_read_session_content: bool,
}

pub fn vm_parse_manifest(lua: &Lua) -> Result<LuaManifest> {
    let table = connector_table(lua)?;

    let name: String = table
        .get("name")
        .map_err(lua_error)
        .map_err(|_| anyhow!("Lua connector manifest missing required field 'name'"))?;
    let short_name: String = table
        .get("short_name")
        .map_err(lua_error)
        .map_err(|_| anyhow!("Lua connector manifest missing required field 'short_name'"))?;

    let has_recon = table.contains_key("recon").map_err(lua_error)?;
    let has_fingerprint = table.contains_key("fingerprint").map_err(lua_error)?;
    let has_read_session_content = table
        .contains_key("read_session_content")
        .map_err(lua_error)?;

    Ok(LuaManifest {
        name,
        short_name,
        has_recon,
        has_fingerprint,
        has_read_session_content,
    })
}

pub struct FingerprintDetails {
    pub available: bool,
    pub process_path: Option<String>,
    pub version: Option<String>,
}

pub fn vm_fingerprint_details(lua: &Lua) -> Result<FingerprintDetails> {
    let table = connector_table(lua)?;
    let func: Function = table
        .get("fingerprint")
        .map_err(lua_error)
        .map_err(|_| anyhow!("Lua connector missing required function 'fingerprint'"))?;

    let ctx = lua.to_value(&json!({})).map_err(lua_error)?;
    let value: Value = func.call(ctx).map_err(lua_error)?;
    parse_fingerprint_details(value)
}

pub fn vm_recon(lua: &Lua, is_semantic: bool, process_path: Option<&str>) -> Result<ReconResult> {
    let table = connector_table(lua)?;
    let func: Function = table
        .get("recon")
        .map_err(lua_error)
        .map_err(|_| anyhow!("Lua connector missing function 'recon'"))?;
    let ctx = lua
        .to_value(&json!({
            "is_semantic": is_semantic,
            "process_path": process_path,
        }))
        .map_err(lua_error)?;
    let value: Value = func.call(ctx).map_err(lua_error)?;
    let recon: ReconResult = lua.from_value(value).map_err(lua_error)?;
    Ok(recon)
}

pub fn vm_create_session(
    lua: &Lua,
    context: &SessionContext,
    process_path: Option<String>,
) -> Result<JsonValue> {
    let table = connector_table(lua)?;
    let func: Function = table
        .get("create_session")
        .map_err(lua_error)
        .map_err(|_| anyhow!("Lua connector missing required function 'create_session'"))?;

    let ctx_json = json!({
        "working_dir": context.working_dir,
        "yolo_mode": context.yolo_mode,
        "prompt_timeout_secs": context.prompt_timeout_secs,
        "interactive": context.interactive,
        "process_path": process_path,
    });
    let ctx = lua.to_value(&ctx_json).map_err(lua_error)?;
    let value: Value = func.call(ctx).map_err(lua_error)?;
    let state: JsonValue = lua.from_value(value).map_err(lua_error)?;

    Ok(state)
}

pub fn vm_session_transact(
    lua: &Lua,
    context: &SessionContext,
    state: &JsonValue,
    prompt: &str,
) -> Result<(String, JsonValue)> {
    let table = connector_table(lua)?;
    let func: Function = table
        .get("session_transact")
        .map_err(lua_error)
        .map_err(|_| anyhow!("Lua connector missing required function 'session_transact'"))?;

    let ctx = lua.to_value(context).map_err(lua_error)?;
    let lua_state = lua.to_value(state).map_err(lua_error)?;
    let result: Value = func.call((ctx, lua_state, prompt)).map_err(lua_error)?;
    parse_transact_result(lua, result)
}

pub fn vm_session_close(lua: &Lua, context: &SessionContext, state: &JsonValue) -> Result<()> {
    let table = connector_table(lua)?;
    let func: Function = table
        .get("session_close")
        .map_err(lua_error)
        .map_err(|_| anyhow!("Lua connector missing required function 'session_close'"))?;

    let ctx = lua.to_value(context).map_err(lua_error)?;
    let lua_state = lua.to_value(state).map_err(lua_error)?;
    let _: MultiValue = func.call((ctx, lua_state)).map_err(lua_error)?;
    Ok(())
}

pub fn vm_read_session_content(lua: &Lua, session_file: &str) -> Result<Option<String>> {
    let table = connector_table(lua)?;
    let func: Function = table.get("read_session_content").map_err(lua_error)?;
    let result: Value = func.call(session_file.to_string()).map_err(lua_error)?;
    match result {
        Value::Nil => Ok(None),
        Value::String(s) => Ok(Some(s.to_str().map_err(lua_error)?.to_string())),
        _ => Ok(None),
    }
}

fn install_shared_api(lua: &Lua) -> Result<()> {
    let praxis = lua.create_table().map_err(lua_error)?;

    praxis
        .set(
            "os_name",
            lua.create_function(|_, ()| {
                #[cfg(windows)]
                {
                    Ok("windows".to_string())
                }
                #[cfg(target_os = "linux")]
                {
                    Ok("linux".to_string())
                }
                #[cfg(target_os = "macos")]
                {
                    Ok("macos".to_string())
                }
                #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
                {
                    Ok("unknown".to_string())
                }
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "uuid_v4",
            lua.create_function(|_, ()| Ok(uuid::Uuid::new_v4().to_string()))
                .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "now_unix",
            lua.create_function(|_, ()| Ok(chrono::Utc::now().timestamp()))
                .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "sha256_hex",
            lua.create_function(|_, input: String| {
                let mut hasher = Sha256::new();
                hasher.update(input.as_bytes());
                let digest = hasher.finalize();
                let mut hex = String::with_capacity(digest.len() * 2);
                for byte in digest.as_slice() {
                    use std::fmt::Write;
                    let _ = write!(&mut hex, "{:02x}", byte);
                }
                Ok(hex)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "json_decode",
            lua.create_function(|lua, input: String| {
                let value: JsonValue = serde_json::from_str(&input)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                lua.to_value(&value)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "json_encode",
            lua.create_function(|lua, value: Value| {
                let json: JsonValue = lua
                    .from_value(value)
                    .map_err(lua_error)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                serde_json::to_string(&json).map_err(|e| mlua::Error::RuntimeError(e.to_string()))
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "path_join",
            lua.create_function(|_, parts: Vec<String>| {
                let mut buf = PathBuf::new();
                for part in parts {
                    if part.is_empty() {
                        continue;
                    }
                    buf.push(part);
                }
                let joined = buf.to_string_lossy().to_string().replace('\\', "/");
                Ok(joined)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "path_exists",
            lua.create_function(|_, path: String| Ok(Path::new(&path).exists()))
                .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "path_is_dir",
            lua.create_function(|_, path: String| Ok(Path::new(&path).is_dir()))
                .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "path_parent",
            lua.create_function(|_, path: String| {
                Ok(Path::new(&path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string().replace('\\', "/")))
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "expand_path",
            lua.create_function(|_, template: String| {
                Ok(crate::agent_connectors::utils::expand_path(&template))
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "env_get",
            lua.create_function(|_, (key, home): (String, Option<String>)| {
                Ok(env_get_for_home(&key, home.as_deref()))
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "user_homes",
            lua.create_function(|_, ()| {
                Ok(crate::agent_connectors::utils::enumerate_user_homes()
                    .into_iter()
                    .map(|p| p.to_string_lossy().to_string().replace('\\', "/"))
                    .collect::<Vec<_>>())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "extract_user_home",
            lua.create_function(|_, path: String| {
                Ok(
                    crate::agent_connectors::utils::extract_user_home_from_path(&path)
                        .map(|p| p.to_string_lossy().to_string().replace('\\', "/")),
                )
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "find_executables",
            lua.create_function(|_, name: String| {
                Ok(crate::utils::find_all_executables_in_path(&name))
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "read_file",
            lua.create_function(|_, path: String| Ok(std::fs::read_to_string(path).ok()))
                .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "write_file",
            lua.create_function(|_, (path, content): (String, String)| {
                std::fs::write(&path, &content).map_err(|e| {
                    mlua::Error::RuntimeError(format!("write_file '{}': {}", path, e))
                })?;
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "read_dir",
            lua.create_function(|lua, path: String| {
                let mut entries = Vec::<JsonValue>::new();
                let iter = std::fs::read_dir(&path)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                for entry in iter.flatten() {
                    let p = entry.path();
                    let md = entry.metadata().ok();
                    let modified = md
                        .as_ref()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64);
                    entries.push(json!({
                        "path": p.to_string_lossy().to_string().replace('\\', "/"),
                        "name": p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string(),
                        "is_dir": p.is_dir(),
                        "is_file": p.is_file(),
                        "modified_unix": modified
                    }));
                }
                lua.to_value(&entries)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "walk_files",
            lua.create_function(|lua, (base, max_depth): (String, usize)| {
                use crate::agent_connectors::utils::SKIP_DIRS;

                let mut out = Vec::<String>::new();
                let walker = walkdir::WalkDir::new(base)
                    .max_depth(max_depth)
                    .into_iter()
                    .filter_entry(|e| {
                        if !e.file_type().is_dir() {
                            return true;
                        }
                        let name = e.file_name().to_string_lossy();
                        !SKIP_DIRS.contains(&name.as_ref())
                    });
                for entry in walker.flatten() {
                    if entry.file_type().is_file() {
                        out.push(
                            entry
                                .path()
                                .to_string_lossy()
                                .to_string()
                                .replace('\\', "/"),
                        );
                    }
                }
                lua.to_value(&out)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "remove_dir",
            lua.create_function(|_, path: String| {
                std::fs::remove_dir_all(&path)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                Ok(true)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "glob_files",
            lua.create_function(|lua, pattern: String| {
                let mut paths = Vec::<String>::new();
                if let Ok(entries) = glob::glob(&pattern) {
                    for entry in entries.flatten() {
                        paths.push(entry.to_string_lossy().to_string().replace('\\', "/"));
                    }
                }
                lua.to_value(&paths)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "count_file_lines",
            lua.create_function(|_, path: String| {
                let count = match std::fs::File::open(&path) {
                    Ok(file) => {
                        use std::io::BufRead;
                        std::io::BufReader::new(file)
                            .lines()
                            .filter_map(|l| l.ok())
                            .filter(|l| !l.trim().is_empty())
                            .count()
                    }
                    Err(_) => 0,
                };
                Ok(count)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "command_run",
            lua.create_function(|lua, spec: Value| {
                let spec_json: JsonValue = lua
                    .from_value(spec)
                    .map_err(lua_error)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                let result = run_command(&spec_json, None)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                lua.to_value(&result)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "command_run_handle",
            lua.create_function(|lua, (spec, handle): (Value, String)| {
                let spec_json: JsonValue = lua
                    .from_value(spec)
                    .map_err(lua_error)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                let result = run_command(&spec_json, Some(handle))
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                lua.to_value(&result)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "command_abort_handle",
            lua.create_function(|_, handle: String| Ok(abort_handle(&handle)))
                .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    //
    // ACP (Agent Client Protocol) functions for long-lived agent subprocesses.
    //

    praxis
        .set(
            "acp_start",
            lua.create_function(|lua, spec: mlua::Value| {
                let spec_json: JsonValue = lua.from_value(spec).map_err(|e| {
                    mlua::Error::RuntimeError(format!("Invalid acp_start spec: {}", e))
                })?;
                let program = spec_json["program"]
                    .as_str()
                    .ok_or_else(|| mlua::Error::RuntimeError("Missing 'program'".into()))?;
                let args: Vec<String> = spec_json["args"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let cwd = spec_json["cwd"].as_str().unwrap_or("");

                let client = crate::acp::client::spawn_acp_client(program, &args, cwd)
                    .map_err(|e| mlua::Error::RuntimeError(format!("ACP start failed: {}", e)))?;

                let handle = uuid::Uuid::new_v4().to_string();
                crate::acp::register_client(&handle, client);
                Ok(handle)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "acp_create_session",
            lua.create_function(|_, (handle, cwd): (String, String)| {
                crate::acp::with_client(&handle, |client| client.create_session(&cwd))
                    .ok_or_else(|| {
                        mlua::Error::RuntimeError(format!("ACP handle '{}' not found", handle))
                    })?
                    .map_err(|e| {
                        mlua::Error::RuntimeError(format!("ACP session/new failed: {}", e))
                    })
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "acp_prompt",
            lua.create_function(
                |_, (handle, prompt, yolo, interactive): (String, String, bool, bool)| {
                    //
                    // Retrieve channels from the global registries. These are set up
                    // by the session handler before calling transact().
                    //

                    let update_tx = crate::acp::take_update_sender(&handle);
                    let permission_rx = crate::acp::take_permission_receiver(&handle);
                    tracing::debug!(
                        "acp_prompt: handle='{}' has_update_tx={} has_permission_rx={}",
                        handle,
                        update_tx.is_some(),
                        permission_rx.is_some()
                    );

                    //
                    // If no channels are registered (CLI-mode fallback or misconfigured),
                    // create dummy ones so the ACP client still works.
                    //

                    let (fallback_tx, _fallback_rx);
                    let update_tx = match update_tx {
                        Some(tx) => tx,
                        None => {
                            let (tx, rx) = tokio::sync::mpsc::channel(1024);
                            fallback_tx = tx;
                            _fallback_rx = rx;
                            fallback_tx.clone()
                        }
                    };

                    let (fallback_perm_tx, fallback_perm_rx_holder);
                    let permission_rx = match permission_rx {
                        Some(rx) => rx,
                        None => {
                            let (tx, rx) = std::sync::mpsc::channel();
                            fallback_perm_tx = tx;
                            fallback_perm_rx_holder = rx;
                            drop(fallback_perm_tx);
                            fallback_perm_rx_holder
                        }
                    };

                    let cancel_flag = std::sync::atomic::AtomicBool::new(false);

                    crate::acp::with_client(&handle, |client| {
                        client.send_prompt(
                            &prompt,
                            &update_tx,
                            permission_rx,
                            yolo,
                            interactive,
                            &cancel_flag,
                        )
                    })
                    .ok_or_else(|| {
                        mlua::Error::RuntimeError(format!("ACP handle '{}' not found", handle))
                    })?
                    .map_err(|e| mlua::Error::RuntimeError(format!("ACP prompt failed: {}", e)))
                },
            )
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "acp_cancel",
            lua.create_function(|_, handle: String| {
                crate::acp::with_client(&handle, |client| client.cancel())
                    .ok_or_else(|| {
                        mlua::Error::RuntimeError(format!("ACP handle '{}' not found", handle))
                    })?
                    .map_err(|e| mlua::Error::RuntimeError(format!("ACP cancel failed: {}", e)))
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "acp_close",
            lua.create_function(|_, handle: String| {
                crate::acp::cleanup_channels(&handle);
                if let Some(client) = crate::acp::remove_client(&handle) {
                    client.close();
                }
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "acp_is_alive",
            lua.create_function(|_, handle: String| {
                Ok(crate::acp::with_client(&handle, |client| client.is_alive()).unwrap_or(false))
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "toml_decode",
            lua.create_function(|lua, input: String| {
                let value: toml::Value =
                    toml::from_str(&input).map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                let json: JsonValue = serde_json::to_value(&value)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                lua.to_value(&json)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    //
    // Logging helpers so Lua scripts can emit diagnostics.
    //

    praxis
        .set(
            "log_debug",
            lua.create_function(|_, msg: String| {
                common::log_debug!("lua: {}", msg);
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "log_info",
            lua.create_function(|_, msg: String| {
                common::log_info!("lua: {}", msg);
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "log_warn",
            lua.create_function(|_, msg: String| {
                common::log_warn!("lua: {}", msg);
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    //
    // SQLite query helper. Runs a read-only SQL query via the sqlite3 CLI and
    // returns stdout as a string. Returns nil on error.
    //

    praxis
        .set(
            "sqlite_query",
            lua.create_function(|_, (db_path, sql): (String, String)| {
                let output = crate::utils::silent_command("sqlite3")
                    .arg(&db_path)
                    .arg(&sql)
                    .output();

                match output {
                    Ok(out) if out.status.success() => {
                        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                        Ok(Some(stdout))
                    }
                    _ => Ok(None),
                }
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    //
    // Hex decode helper. Decodes a hex string to its UTF-8 representation.
    // Returns nil on invalid input.
    //

    praxis
        .set(
            "hex_decode",
            lua.create_function(|_, hex_str: String| {
                let hex_str = hex_str.trim();
                if hex_str.len() % 2 != 0 {
                    return Ok(None);
                }
                let bytes: Result<Vec<u8>, _> = (0..hex_str.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16))
                    .collect();
                match bytes {
                    Ok(b) => match String::from_utf8(b) {
                        Ok(s) => Ok(Some(s)),
                        Err(_) => Ok(None),
                    },
                    Err(_) => Ok(None),
                }
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    //
    // Semantic parser helper for internal-tools discovery. Blocks on an
    // async call to the semantic parser service and should only be called
    // during semantic recon.
    //

    praxis
        .set(
            "semantic_discover_internal_tools",
            lua.create_function(|lua, response_text: String| {
                let tools = semantic_discover_internal_tools(&response_text);
                lua.to_value(&tools)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "kill_processes_by_name",
            lua.create_function(|_, name: String| {
                crate::utils::kill_processes_by_name(&name);
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    //
    // Spawn a process without waiting for it to exit. Returns { pid, desktop }
    // where desktop is an opaque handle for the hidden desktop (nil if not used).
    // On Windows, supports hidden desktop and minimize (same as CDP spawn).
    //

    praxis
        .set(
            "spawn_detached",
            lua.create_function(|lua, (path, use_hidden): (String, Option<bool>)| {
                let (pid, desktop_id) = spawn_detached_process(&path, use_hidden.unwrap_or(true))
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                let tbl = lua
                    .create_table()
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                tbl.set("pid", pid)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                tbl.set("desktop", desktop_id)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                Ok(tbl)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    //
    // Minimize a process window by PID.
    //

    praxis
        .set(
            "minimize_window",
            lua.create_function(|_, pid: u32| Ok(crate::utils::minimize_process_window(pid)))
                .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    //
    // Release a hidden desktop handle returned by spawn_detached.
    //

    praxis
        .set(
            "release_desktop",
            lua.create_function(|_, id: Option<String>| {
                if let Some(id) = id {
                    release_desktop_handle(&id);
                }
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    //
    // Switch the current thread to a hidden desktop (for UIA interaction) or
    // back to the original desktop (nil). UIA can only interact with windows
    // on the current thread's desktop.
    //

    praxis
        .set(
            "switch_desktop",
            lua.create_function(|_, id: Option<String>| {
                switch_to_desktop(id.as_deref())
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "sleep_ms",
            lua.create_function(|_, ms: u64| {
                std::thread::sleep(std::time::Duration::from_millis(ms));
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "is_cancelled",
            lua.create_function(|_, key: String| Ok(is_cancelled(&key)))
                .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "register_cancel",
            lua.create_function(|_, key: String| {
                register_cancel_flag(&key);
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    praxis
        .set(
            "unregister_cancel",
            lua.create_function(|_, key: String| {
                remove_cancel_flag(&key);
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    //
    // Format a Unix timestamp as ISO 8601 UTC string.
    // Returns empty string for invalid timestamps.
    //

    praxis
        .set(
            "format_unix_timestamp",
            lua.create_function(|_, timestamp: i64| {
                use chrono::{TimeZone, Utc};
                let result = Utc
                    .timestamp_opt(timestamp, 0)
                    .single()
                    .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_default();
                Ok(result)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;

    super::cdp::install_cdp_api(lua, &praxis)?;
    super::uia::install_uia_api(lua, &praxis)?;

    lua.globals().set("praxis", praxis).map_err(lua_error)?;

    //
    // Remove the os library entirely to prevent os.execute, os.remove, etc.
    //

    let _ = lua.globals().set("os", Value::Nil);

    Ok(())
}

fn install_shared_libraries(lua: &Lua) -> Result<()> {
    let package: Table = lua.globals().get("package").map_err(lua_error)?;
    let preload: Table = package.get("preload").map_err(lua_error)?;

    let helpers_src = include_str!("lib/helpers.lua");
    let helpers_loader = lua
        .create_function(move |lua, ()| {
            let value: Value = lua.load(helpers_src).eval().map_err(|e| {
                mlua::Error::RuntimeError(format!("Failed to load praxis.helpers: {}", e))
            })?;
            Ok(value)
        })
        .map_err(lua_error)?;

    preload
        .set("praxis.helpers", helpers_loader)
        .map_err(lua_error)?;

    let devtools_src = include_str!("lib/devtools.lua");
    let devtools_loader = lua
        .create_function(move |lua, ()| {
            let value: Value = lua.load(devtools_src).eval().map_err(|e| {
                mlua::Error::RuntimeError(format!("Failed to load praxis.devtools: {}", e))
            })?;
            Ok(value)
        })
        .map_err(lua_error)?;

    preload
        .set("praxis.devtools", devtools_loader)
        .map_err(lua_error)?;

    let uiautomation_src = include_str!("lib/uiautomation.lua");
    let uiautomation_loader = lua
        .create_function(move |lua, ()| {
            let value: Value = lua.load(uiautomation_src).eval().map_err(|e| {
                mlua::Error::RuntimeError(format!("Failed to load praxis.uiautomation: {}", e))
            })?;
            Ok(value)
        })
        .map_err(lua_error)?;

    preload
        .set("praxis.uiautomation", uiautomation_loader)
        .map_err(lua_error)?;

    Ok(())
}

fn run_command(spec_json: &JsonValue, handle: Option<String>) -> Result<JsonValue> {
    //
    // Empty Lua tables are ambiguous and mlua serializes them as JSON objects
    // rather than arrays. Normalize the args field so serde can deserialize it
    // into Vec<String>.
    //

    let mut spec_value = spec_json.clone();
    if let Some(obj) = spec_value.as_object_mut() {
        if let Some(args) = obj.get("args") {
            if args.is_object() && args.as_object().map_or(false, |m| m.is_empty()) {
                obj.insert("args".to_string(), json!([]));
            }
        }
    }

    let spec: CommandSpec =
        serde_json::from_value(spec_value).map_err(|e| anyhow!("Invalid command spec: {}", e))?;
    if spec.program.trim().is_empty() {
        return Err(anyhow!("Command program is required"));
    }

    let mut cmd = crate::agent_connectors::utils::build_command(&spec.program);
    cmd.args(&spec.args);

    let default_tmp = std::env::temp_dir();
    let default_tmp_str = default_tmp.to_string_lossy();
    let cwd = spec
        .cwd
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&default_tmp_str);
    cmd.current_dir(cwd);
    crate::agent_connectors::utils::configure_command_for_directory(&mut cmd, Path::new(cwd));

    for (k, v) in &spec.env {
        cmd.env(k, v);
    }

    let (handle_key, anonymous, pid_cell) = get_handle_pid_cell(handle);

    //
    // Anonymous (per-invocation) handles are only needed in the registry
    // while the command runs, so reset can kill the process tree. Remove
    // them on every exit path; named handles persist so Lua can abort them
    // across calls.
    //
    let _handle_guard = HandleGuard {
        key: anonymous.then_some(handle_key),
    };

    use std::process::Stdio;

    if spec.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
    if let Some(input) = &spec.stdin {
        common::log_debug!(
            "command: {} {} (stdin: {})",
            cmd.get_program().to_string_lossy(),
            args.join(" "),
            input.replace('\n', " | ")
        );
    } else {
        common::log_debug!(
            "command: {} {}",
            cmd.get_program().to_string_lossy(),
            args.join(" ")
        );
    }

    let result = match cmd.spawn() {
        Ok(mut child) => {
            pid_cell.store(child.id(), Ordering::SeqCst);

            if let Some(input) = &spec.stdin {
                if let Some(mut stdin_pipe) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin_pipe.write_all(input.as_bytes());
                    let _ = stdin_pipe.flush();
                }
            }

            //
            // Read stdout/stderr on background threads to prevent deadlock.
            // If the child produces enough output to fill the pipe buffer
            // (~64KB), it will block on write. We must drain the pipes
            // concurrently with waiting for exit.
            //

            let stdout_pipe = child.stdout.take();
            let stderr_pipe = child.stderr.take();

            let stdout_thread = std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut pipe) = stdout_pipe {
                    use std::io::Read;
                    let _ = pipe.read_to_end(&mut buf);
                }
                buf
            });

            let stderr_thread = std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut pipe) = stderr_pipe {
                    use std::io::Read;
                    let _ = pipe.read_to_end(&mut buf);
                }
                buf
            });

            //
            // Poll child process with reset and timeout checks so we can
            // abort promptly when a reset signal arrives or the command
            // exceeds its time limit.
            //

            let deadline = spec
                .timeout_secs
                .map(|s| std::time::Instant::now() + std::time::Duration::from_secs(s));

            let status = loop {
                if is_reset() {
                    let _ = child.kill();
                    let _ = child.wait();
                    pid_cell.store(0, Ordering::SeqCst);
                    return Err(anyhow!("reset signal received"));
                }
                if let Some(dl) = deadline {
                    if std::time::Instant::now() >= dl {
                        common::log_warn!(
                            "command timed out after {}s: {}",
                            spec.timeout_secs.unwrap_or(0),
                            spec.program
                        );
                        crate::utils::terminate_process_tree(child.id());
                        let _ = child.wait();
                        pid_cell.store(0, Ordering::SeqCst);
                        break Ok(None);
                    }
                }
                match child.try_wait() {
                    Ok(Some(status)) => break Ok(Some(status)),
                    Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                    Err(e) => break Err(e),
                }
            };
            pid_cell.store(0, Ordering::SeqCst);

            //
            // Join reader threads with a timeout. Grandchild processes that
            // inherited pipe FDs can keep them open after the child exits,
            // blocking read_to_end indefinitely.
            //

            let join_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

            let stdout_bytes = loop {
                if stdout_thread.is_finished() {
                    break stdout_thread.join().unwrap_or_default();
                }
                if std::time::Instant::now() >= join_deadline {
                    common::log_warn!("stdout reader thread timed out, output may be incomplete");
                    break Vec::new();
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            };

            let stderr_bytes = loop {
                if stderr_thread.is_finished() {
                    break stderr_thread.join().unwrap_or_default();
                }
                if std::time::Instant::now() >= join_deadline {
                    common::log_warn!("stderr reader thread timed out, output may be incomplete");
                    break Vec::new();
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            };

            match status {
                Ok(None) => {
                    json!({
                        "success": false,
                        "status": -1,
                        "stdout": String::from_utf8_lossy(&stdout_bytes).to_string(),
                        "stderr": format!(
                            "command timed out after {}s",
                            spec.timeout_secs.unwrap_or(0)
                        ),
                        "timed_out": true
                    })
                }
                Ok(Some(status)) => {
                    let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
                    let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

                    if status.success() {
                        let preview = if stdout.len() > 2000 {
                            let mut end = 2000;
                            while end > 0 && !stdout.is_char_boundary(end) {
                                end -= 1;
                            }
                            format!("{}... ({} bytes total)", &stdout[..end], stdout.len())
                        } else {
                            stdout.clone()
                        };
                        common::log_debug!("command output:\n{}", preview);
                    } else {
                        common::log_warn!(
                            "command failed (status {}): {}",
                            status.code().unwrap_or(-1),
                            if stderr.is_empty() { &stdout } else { &stderr }
                        );
                    }

                    json!({
                        "success": status.success(),
                        "status": status.code().unwrap_or_default(),
                        "stdout": stdout,
                        "stderr": stderr
                    })
                }
                Err(e) => {
                    common::log_warn!("command wait failed: {}", e);
                    json!({
                        "success": false,
                        "status": 1,
                        "stdout": "",
                        "stderr": e.to_string()
                    })
                }
            }
        }
        Err(e) => {
            common::log_warn!("command spawn failed: {}", e);
            json!({
                "success": false,
                "status": 1,
                "stdout": "",
                "stderr": e.to_string()
            })
        }
    };

    Ok(result)
}

//
// Spawn a detached process without waiting for it to exit.
// On Windows, uses hidden desktop if available, otherwise spawns normally
// and minimizes after a short delay. Returns the PID.
//

#[cfg(windows)]
static DESKTOP_HANDLES: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashMap<String, crate::utils::HiddenDesktop>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn spawn_detached_process(path: &str, use_hidden_desktop: bool) -> Result<(u32, Option<String>)> {
    let result = crate::utils::spawn_process_detached(path, None, &[], use_hidden_desktop)?;

    //
    // Don't minimize here — the caller may need UIA access to the window first.
    // Use praxis.minimize_window(pid) from Lua when ready.
    //

    #[cfg(windows)]
    {
        let desktop_id = if let Some(desktop) = result.hidden_desktop {
            let id = uuid::Uuid::new_v4().to_string();
            DESKTOP_HANDLES.lock_safe().insert(id.clone(), desktop);
            Some(id)
        } else {
            None
        };
        return Ok((result.pid, desktop_id));
    }

    #[cfg(not(windows))]
    Ok((result.pid, None))
}

#[cfg(windows)]
pub fn store_desktop_handle(id: &str, desktop: crate::utils::HiddenDesktop) {
    DESKTOP_HANDLES
        .lock()
        .unwrap()
        .insert(id.to_string(), desktop);
}

//
// Save/restore the original desktop for switch_to_desktop.
//

#[cfg(windows)]
static ORIGINAL_DESKTOP: once_cell::sync::Lazy<std::sync::Mutex<Option<isize>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));

fn switch_to_desktop(id: Option<&str>) -> Result<()> {
    #[cfg(windows)]
    {
        use windows::Win32::System::StationsAndDesktops::{
            GetThreadDesktop, HDESK, SetThreadDesktop,
        };

        let current_thread = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };

        match id {
            Some(desktop_id) => {
                let map = DESKTOP_HANDLES.lock_safe();
                let desktop = map
                    .get(desktop_id)
                    .ok_or_else(|| anyhow!("desktop handle not found: {}", desktop_id))?;

                //
                // Save the original desktop before switching.
                //

                let mut orig = ORIGINAL_DESKTOP.lock_safe();
                if orig.is_none() {
                    let current = unsafe { GetThreadDesktop(current_thread) };
                    if let Ok(h) = current {
                        *orig = Some(h.0 as isize);
                    }
                }

                let hdesk = unsafe { std::mem::transmute::<isize, HDESK>(desktop.handle) };
                unsafe {
                    SetThreadDesktop(hdesk)
                        .map_err(|e| anyhow!("SetThreadDesktop failed: {}", e))?;
                }
                common::log_info!("Switched to hidden desktop '{}'", desktop.name);
            }
            None => {
                let mut orig = ORIGINAL_DESKTOP.lock_safe();
                if let Some(handle) = orig.take() {
                    let hdesk = unsafe { std::mem::transmute::<isize, HDESK>(handle) };
                    unsafe {
                        SetThreadDesktop(hdesk)
                            .map_err(|e| anyhow!("SetThreadDesktop (restore) failed: {}", e))?;
                    }
                    common::log_info!("Restored original desktop");
                }
            }
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = id;
        Ok(())
    }
}

pub fn release_desktop_handle(id: &str) {
    #[cfg(windows)]
    {
        DESKTOP_HANDLES.lock_safe().remove(id);
    }
    #[cfg(not(windows))]
    {
        let _ = id;
    }
}

fn get_handle_pid_cell(handle: Option<String>) -> (String, bool, Arc<AtomicU32>) {
    let anonymous = handle.is_none();
    let handle = handle.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut map = COMMAND_HANDLES.lock_safe();
    let cell = map
        .entry(handle.clone())
        .or_insert_with(|| Arc::new(AtomicU32::new(0)))
        .clone();
    (handle, anonymous, cell)
}

//
// Removes an anonymous command handle from the registry when the command
// finishes, whatever the exit path (success, timeout, reset, spawn error).
//

struct HandleGuard {
    key: Option<String>,
}

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            COMMAND_HANDLES.lock_safe().remove(&key);
        }
    }
}

fn abort_handle(handle: &str) -> bool {
    let map = COMMAND_HANDLES.lock_safe();
    let Some(cell) = map.get(handle) else {
        return false;
    };
    let pid = cell.load(Ordering::SeqCst);
    if pid == 0 {
        return false;
    }
    let killed = crate::utils::terminate_process_tree(pid);
    cell.store(0, Ordering::SeqCst);
    killed > 0
}

pub fn abort_command_handle(handle: &str) -> bool {
    abort_handle(handle)
}

//
// Semantic parser helper for Lua agents. Blocks on async calls using
// tokio::task::block_in_place so it can be called from synchronous Lua
// functions while the perform_recon future is being polled.
//

fn semantic_discover_internal_tools(response_text: &str) -> Vec<common::AgentTool> {
    let client = match crate::utils::semantic_parser::get_client() {
        Some(c) => c,
        None => return Vec::new(),
    };

    let cleaned = response_text.replace("Generating response", "");

    let result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(client.parse(
            crate::utils::semantic_parser::INTERNAL_TOOLS_PROMPT.to_string(),
            cleaned,
            crate::utils::semantic_parser::INTERNAL_TOOLS_SCHEMA.to_string(),
        ))
    });

    match result {
        Ok(resp) if resp.success => {
            if let Some(json) = resp.json {
                if let Some(tools) =
                    crate::utils::semantic_parser::parse_internal_tools_from_json(&json)
                {
                    return tools;
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn parse_fingerprint_details(value: Value) -> Result<FingerprintDetails> {
    match value {
        Value::Boolean(b) => Ok(FingerprintDetails {
            available: b,
            process_path: None,
            version: None,
        }),
        Value::Table(t) => {
            let available = t.get::<bool>("available").unwrap_or(false);
            let process_path = match t.get::<Value>("process_path") {
                Ok(Value::String(s)) => Some(s.to_str().map_err(lua_error)?.to_string()),
                _ => None,
            };
            let version = match t.get::<Value>("version") {
                Ok(Value::String(s)) => Some(s.to_str().map_err(lua_error)?.to_string()),
                _ => None,
            };
            Ok(FingerprintDetails {
                available,
                process_path,
                version,
            })
        }
        _ => Err(anyhow!("fingerprint must return a boolean or table")),
    }
}

fn parse_transact_result(lua: &Lua, value: Value) -> Result<(String, JsonValue)> {
    let table = match value {
        Value::Table(t) => t,
        _ => return Err(anyhow!("session_transact must return a table")),
    };

    let response: String = table
        .get("response")
        .map_err(lua_error)
        .map_err(|_| anyhow!("session_transact result missing 'response'"))?;
    let state_value: Value = table.get("state").map_err(lua_error).unwrap_or(Value::Nil);
    let state = if matches!(state_value, Value::Nil) {
        JsonValue::Null
    } else {
        lua.from_value(state_value).map_err(lua_error)?
    };

    Ok((response, state))
}

#[cfg(unix)]
fn env_get_for_home(key: &str, home: Option<&str>) -> Option<String> {
    let current_user_home = std::env::var("HOME").ok();
    let home = home.map(str::trim).filter(|h| !h.is_empty());

    //
    // If home is unspecified or matches the current user, just read from the
    // process environment directly.
    //

    if home.is_none() || home == current_user_home.as_deref() {
        return std::env::var(key).ok().filter(|v| !v.is_empty());
    }
    let home = home.unwrap();

    let home_path = Path::new(home);

    use std::os::unix::fs::MetadataExt;
    use std::os::unix::process::CommandExt;

    if nix::unistd::Uid::effective().is_root() {
        if let Ok(meta) = std::fs::metadata(home_path) {
            let mut cmd = std::process::Command::new("sh");
            cmd.arg("-lc")
                .arg(format!("printf %s \"${{{}-}}\"", key))
                .env("HOME", home)
                .uid(meta.uid())
                .gid(meta.gid());
            if let Ok(output) = cmd.output() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }

    let pam_env = home_path.join(".pam_environment");
    if let Ok(contents) = std::fs::read_to_string(pam_env) {
        for line in contents.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = l.split_once('=') {
                if k.trim() == key {
                    let value = v.trim().trim_matches('"').to_string();
                    if !value.is_empty() {
                        return Some(value);
                    }
                }
            }
        }
    }

    None
}

#[cfg(windows)]
fn env_get_for_home(key: &str, home: Option<&str>) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        if !value.is_empty() {
            return Some(value);
        }
    }

    use winreg::RegKey;
    use winreg::enums::*;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(env) = hkcu.open_subkey("Environment") {
        if let Ok(value) = env.get_value::<String, _>(key) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(env) =
        hklm.open_subkey("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment")
    {
        if let Ok(value) = env.get_value::<String, _>(key) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }

    let Some(home) = home else {
        return None;
    };
    let target_home = home.replace('/', "\\").to_lowercase();

    if let Ok(profile_list) =
        hklm.open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList")
    {
        for sid in profile_list.enum_keys().flatten() {
            let Ok(profile_key) = profile_list.open_subkey(&sid) else {
                continue;
            };
            let Ok(profile_path) = profile_key.get_value::<String, _>("ProfileImagePath") else {
                continue;
            };
            if profile_path.replace('/', "\\").to_lowercase() != target_home {
                continue;
            }

            let hku = RegKey::predef(HKEY_USERS);
            if let Ok(user_env) = hku.open_subkey(format!("{}\\Environment", sid)) {
                if let Ok(value) = user_env.get_value::<String, _>(key) {
                    if !value.is_empty() {
                        return Some(value);
                    }
                }
            }
        }
    }

    None
}

#[cfg(not(any(unix, windows)))]
fn env_get_for_home(key: &str, _home: Option<&str>) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}
