use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use common::connector_author::{ConnectorDryRun, DryRunFingerprint, DryRunReconSummary};
use mlua::{Lua, Value};

use super::runtime;

//
// Callbacks a connector must define to be usable for sessions. `recon` is
// optional but strongly recommended, so its absence is only a warning.
//

const REQUIRED_CALLBACKS: &[&str] = &[
    "fingerprint",
    "create_session",
    "session_transact",
    "session_close",
];
const OPTIONAL_CALLBACKS: &[&str] = &["recon", "read_session_content"];

//
// Host API lockdown applied before an unreviewed (e.g. LLM-generated) script
// is evaluated. Anything that writes, kills, spawns long-lived processes or
// drives a UI is replaced with a stub that raises, `command_run` is limited
// to `--version` / `--help` style invocations with a capped timeout,
// `sqlite_query` to read-only SELECT/WITH/PRAGMA statements, and the Lua
// `io`, `debug` and unsafe `os` functions are removed (`debug` would let a
// script recover the original functions from the wrappers' upvalues).
//

const SANDBOX_LUA: &str = r#"
local function blocked(name)
  return function()
    error("praxis." .. name .. " is not available during a connector dry run", 2)
  end
end

local deny = {
  "write_file", "remove_dir", "command_run_handle", "command_abort_handle",
  "acp_start", "acp_create_session", "acp_prompt", "acp_cancel", "acp_close",
  "kill_processes_by_name", "spawn_detached", "minimize_window",
  "switch_desktop", "release_desktop", "semantic_discover_internal_tools",
}
for _, name in ipairs(deny) do
  praxis[name] = blocked(name)
end
for name, _ in pairs(praxis) do
  if type(name) == "string" and (name:match("^cdp_") or name:match("^uia_")) then
    praxis[name] = blocked(name)
  end
end

local run = praxis.command_run
local allowed_args = {
  ["--version"] = true, ["-v"] = true, ["-V"] = true, ["version"] = true,
  ["--help"] = true, ["-h"] = true, ["help"] = true,
}
praxis.command_run = function(spec)
  if type(spec) ~= "table" or type(spec.program) ~= "string" then
    error("praxis.command_run expects a spec table with a program", 2)
  end
  local args = spec.args or {}
  if #args == 0 then
    error("praxis.command_run during a connector dry run needs a --version/--help style argument", 2)
  end
  for _, arg in ipairs(args) do
    if not allowed_args[arg] then
      error("praxis.command_run is limited to --version/--help style invocations during a connector dry run (got '" .. tostring(arg) .. "')", 2)
    end
  end
  local timeout = math.min(tonumber(spec.timeout_secs) or 10, 10)
  return run({ program = spec.program, args = args, cwd = spec.cwd, timeout_secs = timeout })
end

local sqlite = praxis.sqlite_query
praxis.sqlite_query = function(db_path, sql)
  local trimmed = tostring(sql or ""):gsub("^%s+", ""):gsub("[%s;]+$", "")
  local head = trimmed:sub(1, 6):lower()
  if trimmed:find(";", 1, true)
    or not (head == "select" or head:sub(1, 4) == "with" or head == "pragma") then
    error("praxis.sqlite_query is limited to a single SELECT/WITH/PRAGMA statement during a connector dry run", 2)
  end
  return sqlite(db_path, trimmed)
end

io = nil
debug = nil
if os then
  os = {
    time = os.time, date = os.date, clock = os.clock,
    getenv = os.getenv, difftime = os.difftime,
  }
end
if package then
  package.loadlib = nil
  package.cpath = ""
end
"#;

fn fail(result: &mut ConnectorDryRun, stage: &str, error: impl Into<String>) {
    result.ok = false;
    result.failed_stage = Some(stage.to_string());
    result.errors.push(error.into());
}

fn create_vm(script: &str, sandbox: bool, deadline: Instant) -> Result<Lua> {
    let lua = runtime::init_vm_shell()?;

    //
    // Replace the shell's reset-only hook with one that also enforces the
    // dry run's deadline, so a runaway script can't pin the blocking thread.
    //

    lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(1000),
        move |_lua, _debug| {
            if runtime::is_reset() {
                Err(mlua::Error::RuntimeError("reset signal received".into()))
            } else if Instant::now() >= deadline {
                Err(mlua::Error::RuntimeError("dry run timed out".into()))
            } else {
                Ok(mlua::VmState::Continue)
            }
        },
    )
    .map_err(|e| anyhow!(e.to_string()))?;

    if sandbox {
        lua.load(SANDBOX_LUA)
            .set_name("dry_run_sandbox")
            .exec()
            .map_err(|e| anyhow!("failed to install dry-run sandbox: {}", e))?;
    }

    let value: Value = lua
        .load(script)
        .set_name("connector")
        .eval()
        .map_err(|e| anyhow!(e.to_string()))?;
    runtime::set_connector(&lua, value)?;
    Ok(lua)
}

//
// Manifest stage: name/short_name present and non-empty, and every required
// callback defined as a function. Records the callbacks found.
//

fn check_manifest(lua: &Lua, result: &mut ConnectorDryRun) -> bool {
    let manifest = match runtime::vm_parse_manifest(lua) {
        Ok(m) => m,
        Err(e) => {
            fail(result, "manifest", e.to_string());
            return false;
        }
    };
    result.name = Some(manifest.name.clone());
    result.short_name = Some(manifest.short_name.clone());
    if manifest.short_name.trim().is_empty() || manifest.name.trim().is_empty() {
        fail(result, "manifest", "name and short_name must be non-empty");
        return false;
    }

    let connector: mlua::Table = match lua.globals().get("_connector") {
        Ok(t) => t,
        Err(e) => {
            fail(result, "manifest", e.to_string());
            return false;
        }
    };
    let is_function = |name: &str| matches!(connector.get::<Value>(name), Ok(Value::Function(_)));
    for name in REQUIRED_CALLBACKS.iter().chain(OPTIONAL_CALLBACKS) {
        if is_function(name) {
            result.callbacks.push(name.to_string());
        }
    }
    let missing: Vec<&str> = REQUIRED_CALLBACKS
        .iter()
        .copied()
        .filter(|name| !is_function(name))
        .collect();
    if !missing.is_empty() {
        fail(
            result,
            "manifest",
            format!(
                "missing required callback function(s): {}",
                missing.join(", ")
            ),
        );
        return false;
    }
    if !is_function("recon") {
        result
            .warnings
            .push("no 'recon' callback: config, tools and sessions will not be discovered".into());
    }
    true
}

//
// Validate a connector script without creating a session: syntax check,
// manifest and required-callback check, fingerprint, then static (non-
// semantic) recon. Blocking; run on a blocking thread. Stops at the first
// failing stage.
//

pub fn dry_run(script: &str, sandbox: bool, timeout: Duration) -> ConnectorDryRun {
    let deadline = Instant::now() + timeout;
    let mut result = ConnectorDryRun {
        sandboxed: sandbox,
        ..Default::default()
    };

    if let Err(e) = runtime::compile_bytecode(script) {
        fail(&mut result, "syntax", e.to_string());
        return result;
    }

    let lua = match create_vm(script, sandbox, deadline) {
        Ok(lua) => lua,
        Err(e) => {
            fail(&mut result, "load", e.to_string());
            return result;
        }
    };

    if !check_manifest(&lua, &mut result) {
        return result;
    }
    let has_recon = result.callbacks.iter().any(|c| c == "recon");

    let fingerprint = match runtime::vm_fingerprint_details(&lua) {
        Ok(fp) => fp,
        Err(e) => {
            fail(&mut result, "fingerprint", e.to_string());
            return result;
        }
    };
    result.fingerprint = Some(DryRunFingerprint {
        available: fingerprint.available,
        process_path: fingerprint.process_path.clone(),
        version: fingerprint.version.clone(),
    });
    if !fingerprint.available {
        fail(
            &mut result,
            "fingerprint",
            "fingerprint returned available=false: the agent was not detected on this node",
        );
        return result;
    }
    if fingerprint.process_path.is_none() {
        result.warnings.push(
            "fingerprint did not return process_path; sessions will not know which binary to run"
                .into(),
        );
    }

    if has_recon {
        match runtime::vm_recon(&lua, false, fingerprint.process_path.as_deref()) {
            Ok(recon) => {
                let summary = DryRunReconSummary {
                    config_items: recon.config.items.len(),
                    project_paths: recon.config.project_paths.len(),
                    mcp_servers: recon.tools.mcp_servers.len(),
                    skills: recon.tools.skills.len(),
                    sessions: recon.sessions.items.len(),
                };
                if summary.is_empty() {
                    result.warnings.push("recon returned no data".into());
                }
                result.recon = Some(summary);
            }
            Err(e) => {
                fail(&mut result, "recon", e.to_string());
                return result;
            }
        }
    }

    result.ok = true;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
local helpers = require("praxis.helpers")
return {
  name = "Test Agent",
  short_name = "testagent",
  fingerprint = function(_ctx)
    return { available = true, process_path = "/bin/sh", version = "1.0" }
  end,
  recon = function(_ctx)
    return {
      config = { items = {}, project_paths = { "/tmp" } },
      tools = { mcp_servers = {}, skills = {}, internal_tools = {} },
      sessions = { items = {} },
    }
  end,
  create_session = function(ctx) return { handle = praxis.uuid_v4() } end,
  session_transact = function(_ctx, state, prompt) return { response = prompt, state = state } end,
  session_close = function(_ctx, _state) end,
}
"#;

    fn run(script: &str, sandbox: bool) -> ConnectorDryRun {
        dry_run(script, sandbox, Duration::from_secs(10))
    }

    #[test]
    fn good_script_passes() {
        let result = run(GOOD, true);
        assert!(result.ok, "{:?}", result);
        assert_eq!(result.short_name.as_deref(), Some("testagent"));
        assert_eq!(result.recon.unwrap().project_paths, 1);
        assert!(result.callbacks.contains(&"recon".to_string()));
    }

    #[test]
    fn syntax_error_is_reported() {
        let result = run("return { name = ", true);
        assert!(!result.ok);
        assert_eq!(result.failed_stage.as_deref(), Some("syntax"));
    }

    #[test]
    fn non_table_return_fails_load() {
        let result = run("return 42", true);
        assert_eq!(result.failed_stage.as_deref(), Some("load"));
    }

    #[test]
    fn missing_callbacks_fail_manifest() {
        let script = GOOD.replace("session_close = function(_ctx, _state) end,", "");
        let result = run(&script, true);
        assert_eq!(result.failed_stage.as_deref(), Some("manifest"));
        assert!(result.errors[0].contains("session_close"));
    }

    #[test]
    fn unavailable_fingerprint_fails() {
        let script = GOOD.replace("available = true", "available = false");
        let result = run(&script, true);
        assert_eq!(result.failed_stage.as_deref(), Some("fingerprint"));
    }

    #[test]
    fn recon_error_fails_recon_stage() {
        let script = GOOD.replace(
            "recon = function(_ctx)",
            "recon = function(_ctx)\n    error(\"boom\")",
        );
        let result = run(&script, true);
        assert_eq!(result.failed_stage.as_deref(), Some("recon"));
        assert!(result.errors[0].contains("boom"));
    }

    #[test]
    fn empty_recon_is_a_warning() {
        let script = GOOD.replace("project_paths = { \"/tmp\" }", "project_paths = {}");
        let result = run(&script, true);
        assert!(result.ok);
        assert!(result.warnings.iter().any(|w| w.contains("no data")));
    }

    #[test]
    fn sandbox_blocks_side_effects() {
        let script = GOOD.replace(
            "return { available = true",
            "praxis.write_file('/tmp/praxis-dry-run-should-not-exist', 'x')\n    return { available = true",
        );
        let result = run(&script, true);
        assert_eq!(result.failed_stage.as_deref(), Some("fingerprint"));
        assert!(result.errors[0].contains("not available during a connector dry run"));
        assert!(!std::path::Path::new("/tmp/praxis-dry-run-should-not-exist").exists());

        for snippet in [
            "praxis.command_run({ program = 'rm', args = { '-rf', '/nonexistent' } })",
            "os.execute('true')",
            "io.open('/tmp/x', 'w')",
            "debug.getupvalue(praxis.command_run, 1)",
            "praxis.sqlite_query('/tmp/x.db', 'DELETE FROM t')",
        ] {
            let script = GOOD.replace(
                "return { available = true",
                &format!("{}\n    return {{ available = true", snippet),
            );
            let result = run(&script, true);
            assert!(!result.ok, "sandbox should block: {}", snippet);
        }
    }

    #[cfg(unix)]
    #[test]
    fn sandbox_allows_version_flags() {
        let script = GOOD.replace(
            "return { available = true",
            "local r = praxis.command_run({ program = '/bin/echo', args = { '--version' } })\n    assert(r ~= nil)\n    return { available = true",
        );
        assert!(run(&script, true).ok);
    }

    #[test]
    fn runaway_script_hits_deadline() {
        let script = GOOD.replace(
            "return { available = true",
            "while true do end\n    return { available = true",
        );
        let result = dry_run(&script, true, Duration::from_millis(200));
        assert_eq!(result.failed_stage.as_deref(), Some("fingerprint"));
        assert!(result.errors[0].contains("timed out"));
    }

    #[test]
    fn embedded_connectors_pass_manifest_stage() {
        for script in super::super::EMBEDDED_LUA_SCRIPTS {
            let lua = create_vm(script, true, Instant::now() + Duration::from_secs(10))
                .expect("embedded connector should load in the sandbox");
            let mut result = ConnectorDryRun::default();
            assert!(check_manifest(&lua, &mut result), "{:?}", result);
        }
    }
}
