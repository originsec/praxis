# Adding New Connectors

This guide walks through creating a connector for a new AI agent.

**Prefer Lua connectors** for all agents. Lua scripts are easier to write, can be updated at runtime via the web UI without recompiling, and share common helpers for executable discovery, version extraction, and multi-user support. For browser-based agents, the `praxis.devtools` Lua library and `praxis.cdp_*` native API provide Chrome DevTools Protocol support (see M365 Copilot as an example). Use Rust connectors only when you need OS-level capabilities that aren't exposed through the Lua API.

## Lua Connector (Recommended)

Lua agent scripts live in `agents/` at the project root and are embedded into binaries at build time. They can also be uploaded via the web UI (Settings > Agents).

> **Tip**: Scripts uploaded or created through the web UI are tagged as user scripts and won't be overwritten by Praxis updates. If you want to customize a built-in script, create a copy with your changes and disable the original.

### CLI Agents vs Browser-Based Agents

For **CLI agents** (e.g. Claude Code, Gemini CLI), use `praxis.command_run` / `praxis.command_run_handle` to spawn processes and interact via stdin/stdout. For agents that support the [Agent Client Protocol](https://agentclientprotocol.com/) (ACP), use the `praxis.acp_*` APIs for long-lived subprocess sessions with real-time streaming (see [ACP Sessions](#acp-sessions-streaming-agents) below).

For **browser-based agents** (e.g. M365 Copilot), use the `praxis.devtools` library and `praxis.cdp_*` native API to drive the agent via Chrome DevTools Protocol. See [DevTools-Based Agents](#devtools-based-agents-browser-automation) below.

### Script Structure

A Lua connector returns a table with `name`, `short_name`, and callback functions. For CLI agents, follow the same high-level structure used by `agents/gemini.lua`:

```lua
local helpers = require("praxis.helpers")

local AGENT_NAME = "Example AI"
local AGENT_SHORT_NAME = "exampleai"
local INTERCEPT_DOMAINS = { "api.exampleai.com" }

local function verify_binary(path)
  local result = praxis.command_run({ program = path, args = { "--version" } })
  if result.success then
    local version = (result.stdout or ""):match("(%d[%d%.%-a-zA-Z]*)")
    return true, version
  end
  return false, nil
end

local function pick_path()
  return helpers.find_executable({
    name = "exampleai",
    global_dirs = {
      default = { "/usr/local/bin", "/usr/bin" },
    },
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\.local\\bin" },
    },
    verify = verify_binary,
  })
end

return {
  name = AGENT_NAME,
  short_name = AGENT_SHORT_NAME,

  fingerprint = function(_ctx)
    local process_path, process_version = pick_path()
    return {
      available = process_path ~= nil,
      process_path = process_path,
      version = process_version,
    }
  end,

  -- Optional: traffic interception domains.
  intercept_domains = function(_ctx)
    return INTERCEPT_DOMAINS
  end,

  -- Optional but recommended: reconnaissance.
  -- Use run_standard_recon + declarative recon_config.
  recon = function(ctx)
    return helpers.run_standard_recon(ctx, recon_config)
  end,

  -- Required for sessions.
  create_session = function(ctx)
    return {
      handle = praxis.uuid_v4(),
      process_path = ctx.process_path,
      working_dir = ctx.working_dir,
      yolo_mode = ctx.yolo_mode == true,
    }
  end,

  session_transact = function(_ctx, state, prompt)
    local result = praxis.command_run_handle({
      program = state.process_path,
      args = { "--prompt", "-" },
      cwd = state.working_dir,
      stdin = prompt,
    }, state.handle)
    return { response = result.stdout or "", state = state }
  end,

  session_close = function(_ctx, state)
    -- Cleanup if needed.
  end,
}
```

Recommended pattern for recon config (same style as Gemini/Cursor/ClaudeCode):

```lua
local recon_config = {
  home_dir = ".exampleai",

  home_configs = {
    { path = ".exampleai/settings.json", type = "global_settings", mcp = true },
  },

  project_markers = { "/.exampleai/settings.json" },

  project_configs = {
    { path = ".exampleai/settings.json", type = "project_settings", mcp = true },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json_flexible,
  },

  auth_check = path_has_valid_auth,
  session_discovery = discover_sessions_for_home,

  session_fns = {
    create = run_create_session,
    transact = run_session_transact,
    close = run_session_close,
  },
}
```

Key points:
- `recon` receives a context object: `recon = function(ctx) ... end`
- Semantic vs non-semantic recon is driven by `ctx.is_semantic` inside helpers
- Avoid mutable global process state; return `process_path` from `fingerprint` and consume it via `ctx.process_path`

### `helpers.find_executable` Config

The `find_executable` helper searches for an agent binary in 4 phases:

1. **PATH search** via `praxis.find_executables(name)` - searches the system PATH
2. **Global directories** - explicit absolute paths (e.g. `/usr/local/bin`)
3. **Home directories** - templates expanded per user home (e.g. `${HOME}/.local/bin`)
4. **Glob patterns** - for version manager installations (e.g. nvm, mise)

On Windows, `.cmd` is tried before `.exe` for each directory. The `verify` function receives a candidate path and returns `(passed, version)`.

Config fields:
- `name` (string) - executable name for PATH search and path construction
- `global_dirs` (table) - `{ default = {...}, windows = {...} }` absolute directories
- `home_dirs` (table) - same shape, directory templates with `${HOME}` etc.
- `glob_paths` (table) - full glob patterns (wildcards embedded in path)
- `verify` (function) - `fn(path) -> passed, version`

OS resolution: `tbl[os_name] or tbl.default or {}` where `os_name` is `"linux"`, `"macos"`, or `"windows"`.

### Available Lua APIs

The `praxis` global provides:

- **Filesystem**: `path_exists`, `path_join`, `read_file`, `walk_files`, `glob_files`
- **Commands**: `command_run`, `command_run_handle`, `command_abort_handle`
- **ACP**: `acp_start`, `acp_create_session`, `acp_prompt`, `acp_close`
- **Environment**: `os_name`, `user_homes`, `env_get`, `expand_path`
- **Process**: `find_executables`, `kill_processes_by_name`
- **CDP**: `cdp_spawn_and_connect`, `cdp_connect`, `cdp_evaluate`, `cdp_click`, `cdp_type_text`, `cdp_press_key`, `cdp_wait_for_element`, `cdp_find_elements`, `cdp_close`, `cdp_process_id`
- **Utilities**: `json_decode`, `toml_decode`, `uuid_v4`, `now_unix`, `sleep_ms`, `log_info`, `log_warn`

The `helpers` module (`require("praxis.helpers")`) provides `find_executable`, `expand_path`, `starts_with`, `ends_with`, `dedup`, `parse_json`, `parse_toml`, `user_homes_with_dir`, `for_each_user_home_coalesce`, `run_standard_recon`, `collect_configs`, `extract_mcp_servers`, and parser helpers such as `parse_mcp_from_json`, `parse_mcp_from_json_flexible`, and `parse_mcp_from_toml`.

The `devtools` module (`require("praxis.devtools")`) provides `connect`, `transact`, and `close` for browser-based agents using Chrome DevTools Protocol. See [DevTools-Based Agents](#devtools-based-agents-browser-automation) below.

### Deploying

- **Embedded**: Add the `.lua` file to `agents/` and rebuild. It will be compiled into both node and service binaries.
- **Runtime**: Upload via Settings > Agents in the web UI. The script is stored in the service database and pushed to all connected nodes.

---

## ACP Sessions (Streaming Agents)

For agents that support the [Agent Client Protocol](https://agentclientprotocol.com/) (ACP), sessions use a long-lived subprocess with JSON-RPC 2.0 over NDJSON stdio. Praxis uses the `agent-client-protocol` crate internally, providing typed `ClientSideConnection` communication with `Client` trait callbacks for real-time streaming updates (text chunks, tool calls, plans, permission requests).

### ACP Lua API

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `praxis.acp_start` | spec table | handle (string) | Spawn an ACP subprocess and perform the initialize handshake |
| `praxis.acp_create_session` | handle, cwd | session_id (string) | Create an ACP session with a working directory |
| `praxis.acp_prompt` | handle, prompt, yolo, interactive | response (string) | Send a prompt and wait for the streamed response. `yolo` auto-approves permission requests; `interactive` forwards them to the user |
| `praxis.acp_close` | handle | — | Close the ACP session and terminate the subprocess |

The `acp_start` spec table:

| Field | Type | Description |
|-------|------|-------------|
| `program` | string | Path to the agent executable |
| `args` | table | Command-line arguments (e.g. `{ "acp" }` or `{ "--acp" }`) |
| `cwd` | string | Working directory for the subprocess |

### Example

```lua
create_session = function(ctx)
  local acp_handle = praxis.acp_start({
    program = ctx.process_path,
    args = { "--acp" },
    cwd = ctx.working_dir or "",
  })

  local session_id = praxis.acp_create_session(acp_handle, ctx.working_dir or "")

  return {
    acp_handle = acp_handle,
    acp_session_id = session_id,
    yolo_mode = ctx.yolo_mode == true,
    interactive = ctx.interactive == true,
  }
end,

session_transact = function(_ctx, state, prompt)
  local response = praxis.acp_prompt(
    state.acp_handle, prompt,
    state.yolo_mode or false,
    state.interactive or false
  )
  return { response = response, state = state }
end,

session_close = function(_ctx, state)
  if state.acp_handle then
    praxis.acp_close(state.acp_handle)
  end
end,
```

During `acp_prompt`, streaming updates (text, tool calls, tool results) are automatically forwarded to the client (TUI or web UI) in real time. The function blocks until the full response is assembled and returns the final text.

---

## DevTools-Based Agents (Browser Automation)

For agents that run in a browser or WebView (e.g. M365 Copilot), Praxis provides a CDP (Chrome DevTools Protocol) stack. The architecture has three layers:

```
your_agent.lua               ← Agent-specific: CSS selectors, response parsing
    ↓ uses
require("praxis.devtools")   ← Generic transact loop, connect/close lifecycle
    ↓ uses
praxis.cdp_*                 ← Native Rust: CDP connection, JS eval, DOM ops
```

### The `devtools` Module

`require("praxis.devtools")` provides three functions:

| Function | Description |
|----------|-------------|
| `devtools.connect(config)` | Spawn a process with a debug port, connect via CDP, return a handle string |
| `devtools.transact(handle, adapter, prompt)` | Send a prompt and poll for response using the adapter's selectors |
| `devtools.close(handle)` | Close the CDP connection and terminate the process tree |

The `connect` config table:

| Field | Type | Description |
|-------|------|-------------|
| `process_path` | string | Path to the executable |
| `debug_port_env_var` | string | Environment variable for the debug port argument |
| `debug_port_format` | string | Format string, e.g. `"--remote-debugging-port={}"` |
| `base_port` | number | Base port number (random offset added) |
| `port_range` | number | Range for random port selection (default 778) |
| `kill_existing` | bool | Kill existing processes first (default true) |
| `use_hidden_desktop` | bool | Spawn on hidden desktop on Windows (default true). In debug builds, `PRAXIS_NOT_HIDDEN` defaults to `1` (visible); in release builds it defaults to `0` (hidden). |

### The Adapter Table

The `transact` function takes an adapter table that defines how to interact with the specific agent's UI:

```lua
local my_adapter = {
  -- CSS selector for the text input element (required)
  input_selector = '#chat-input',

  -- CSS selector for response message elements (required)
  message_selector = 'div.response-message',

  -- Check response state by running JS in the page (required)
  -- Returns: { response = string|nil, is_generating = bool, has_new_messages = bool }
  check_response_state = function(handle, initial_count)
    local result = praxis.cdp_evaluate(handle, [[
      (function() {
        var messages = document.querySelectorAll('div.response-message');
        var text = '';
        if (messages.length > 0) {
          text = messages[messages.length - 1].innerText.trim();
        }
        var loading = document.querySelector('.loading-indicator');
        return {
          responseText: text,
          messageCount: messages.length,
          isGenerating: loading !== null
        };
      })()
    ]])

    local count = (result and result.messageCount) or 0
    local generating = (result and result.isGenerating) or false
    local text = (result and result.responseText) or ""

    local response = nil
    if count > initial_count and not generating and #text > 0 then
      response = text
    end

    return {
      response = response,
      is_generating = generating,
      has_new_messages = count > initial_count,
    }
  end,

  -- Optional: wait for submit button to be enabled before pressing Enter
  wait_for_submit_ready = function(handle)
    praxis.cdp_wait_for_element(handle, 'button.send:not([disabled])', 50, 100)
  end,
}
```

### Full Example

Here is an M365-style DevTools-based agent template:

```lua
local helpers = require("praxis.helpers")
local devtools = require("praxis.devtools")

local AGENT_NAME = "My DevTools Agent"
local AGENT_SHORT_NAME = "mydevtools"

local PROCESS_NAME = "MyAgent.exe"
local INPUT_SELECTOR = '#chat-input'
local MESSAGE_SELECTOR = 'div.assistant-message'
local SEND_BUTTON_SELECTOR = 'button[aria-label=\"Send\"]:not([aria-disabled=\"true\"])'
local STOP_BUTTON_SELECTOR = 'button[aria-label=\"Stop generating\"]'

local my_adapter = {
  input_selector = INPUT_SELECTOR,
  message_selector = MESSAGE_SELECTOR,

  check_response_state = function(handle, initial_count)
    local js = "(function() {"
      .. "var msgs = document.querySelectorAll('" .. MESSAGE_SELECTOR .. "');"
      .. "var text = '';"
      .. "if (msgs.length > 0) {"
      .. "  var last = msgs[msgs.length - 1];"
      .. "  text = (last.innerText || last.textContent || '').trim();"
      .. "}"
      .. "var stopBtn = document.querySelector('" .. STOP_BUTTON_SELECTOR .. "');"
      .. "return { responseText: text, messageCount: msgs.length, isGenerating: stopBtn !== null };"
      .. "})()"
    local result = praxis.cdp_evaluate(handle, js)

    local message_count = (result and result.messageCount) or 0
    local is_generating = (result and result.isGenerating) or false
    local response_text = (result and result.responseText) or ""
    local has_new_messages = message_count > initial_count

    local response = nil
    if has_new_messages and not is_generating and #response_text > 0 then
      response = response_text
    end

    return {
      response = response,
      is_generating = is_generating,
      has_new_messages = has_new_messages,
    }
  end,

  wait_for_submit_ready = function(handle)
    praxis.cdp_wait_for_element(handle, SEND_BUTTON_SELECTOR, 100, 100)
  end,
}

local function post_initialize(handle, _working_dir)
  -- Wait for the chat UI to be ready.
  praxis.cdp_wait_for_element(handle, INPUT_SELECTOR, 30, 300)

  -- Optional: click mode toggle, open fresh chat, dismiss banners, etc.
  -- pcall(praxis.cdp_click, handle, 'button[data-testid=\"new-chat\"]')
end

local function run_create_session(ctx)
  praxis.kill_processes_by_name(PROCESS_NAME)
  praxis.sleep_ms(500)

  local cdp_handle = devtools.connect({
    process_path = ctx.process_path,
    debug_port_env_var = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    debug_port_format = "--remote-debugging-port={}",
    base_port = 9222,
    port_range = 778,
  })

  post_initialize(cdp_handle, ctx.working_dir)

  return {
    handle = cdp_handle,
    cdp_handle = cdp_handle,
    working_dir = ctx.working_dir,
    process_id = praxis.cdp_process_id(cdp_handle),
  }
end

local function run_session_transact(state, prompt)
  local response = devtools.transact(state.cdp_handle, my_adapter, prompt)
  return { response = response, state = state }
end

local function run_session_close(state)
  if state and state.cdp_handle then
    devtools.close(state.cdp_handle)
  end
end

local function do_recon(ctx)
  if praxis.os_name() ~= "windows" then
    return nil
  end

  local internal_tools = {}
  if ctx.is_semantic == true then
    internal_tools = helpers.discover_internal_tools(
      { process_path = ctx.process_path, working_dir = nil },
      { create = run_create_session, transact = run_session_transact, close = run_session_close }
    )
  end

  return {
    tools = { internal_tools = internal_tools, mcp_servers = {}, skills = {} },
    project_paths = {},
    metadata = nil,
  }
end

local function do_fingerprint()
  if praxis.os_name() ~= "windows" then
    return nil
  end
  local paths = praxis.find_executables(PROCESS_NAME) or {}
  if #paths > 0 then
    return paths[1]
  end
  return nil
end

return {
  name = AGENT_NAME,
  short_name = AGENT_SHORT_NAME,

  fingerprint = function(_ctx)
    local path = do_fingerprint()
    return { available = path ~= nil, process_path = path }
  end,

  recon = function(ctx)
    return do_recon(ctx)
  end,

  create_session = function(ctx)
    return run_create_session(ctx)
  end,

  session_transact = function(_ctx, state, prompt)
    return run_session_transact(state, prompt)
  end,

  session_close = function(_ctx, state)
    run_session_close(state)
  end,
}
```

### Session State Keys

For CDP sessions to support abort and cleanup, the session state returned by `create_session` should include:

- `handle` — used by the Rust session layer for command abort lookup
- `cdp_handle` — the CDP connection handle string (cleaned up by Rust on drop)
- `process_id` — the spawned process PID (killed by Rust on abort or drop)

### CDP API Reference

Low-level functions available on the `praxis` global:

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `cdp_spawn_and_connect` | config table | handle string | Spawn process, connect via CDP |
| `cdp_connect` | port (number) | handle string | Connect to existing DevTools endpoint |
| `cdp_evaluate` | handle, js (string) | value | Execute JavaScript, return result |
| `cdp_find_elements` | handle, selector | count (number) | Count matching DOM elements |
| `cdp_click` | handle, selector | — | Click an element |
| `cdp_type_text` | handle, text | — | Insert text via CDP InsertText (handles emojis) |
| `cdp_press_key` | handle, selector, key | — | Press a key on an element |
| `cdp_wait_for_element` | handle, selector, retries, delay_ms | bool | Poll for element existence |
| `cdp_close` | handle | — | Close connection, terminate process |
| `cdp_process_id` | handle | number or nil | Get PID of spawned process |

---

## Rust Connector (for native/OS-level agents)

Use this approach only when Lua cannot access the required OS capabilities.

### Step 1: Create the Directory Structure

Create a new directory under `node/src/agent_connectors/`:

```
node/src/agent_connectors/
├── exampleai/
│   ├── mod.rs          # Main agent implementation
│   ├── fingerprint.rs  # Fingerprinting logic
│   ├── intercept.rs    # Interception domains
│   ├── recon.rs        # Reconnaissance
│   └── session.rs      # Session management
├── factory.rs
├── mod.rs
└── traits.rs
```

## Step 2: Implement the Agent Trait

In `mod.rs`:

```rust
mod fingerprint;
mod intercept;
mod recon;
mod session;

pub use session::ExampleAISession;

use crate::agent_connectors::traits::{Agent, AgentIntercept, AgentRecon, AgentSession};
use async_trait::async_trait;
use once_cell::sync::OnceCell;
use std::sync::{Arc, RwLock};

const AGENT_NAME: &str = "ExampleAI";
const AGENT_SHORTNAME: &str = "exampleai";

pub struct ExampleAIAgent {
    pub(crate) process_path: OnceCell<String>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
}

impl ExampleAIAgent {
    pub fn new() -> Self {
        Self {
            process_path: OnceCell::new(),
            session: RwLock::new(None),
        }
    }
}

#[async_trait]
impl Agent for ExampleAIAgent {
    fn name(&self) -> &str {
        AGENT_NAME
    }

    fn short_name(&self) -> &str {
        AGENT_SHORTNAME
    }

    fn as_intercept(&self) -> Option<&dyn AgentIntercept> {
        Some(self)  // Return None if no interception support
    }

    fn as_recon(&self) -> Option<&dyn AgentRecon> {
        Some(self)  // Return None if no recon support
    }

    async fn do_fingerprint(&self) -> bool {
        self.do_fingerprint_impl().await
    }

    fn create_session(&self, context: &common::SessionContext) -> Option<Arc<dyn AgentSession>> {
        match ExampleAISession::new(self.process_path.get().cloned(), context) {
            Ok(session) => {
                let session_arc = Arc::new(session) as Arc<dyn AgentSession>;
                *self.session.write().unwrap() = Some(Arc::clone(&session_arc));
                Some(session_arc)
            }
            Err(e) => {
                common::log_error!("{}: Failed to create session: {}", AGENT_NAME, e);
                None
            }
        }
    }

    fn get_session(&self) -> Option<Arc<dyn AgentSession>> {
        self.session.read().unwrap().clone()
    }

    fn close_session(&self) {
        let mut guard = self.session.write().unwrap();
        if let Some(session) = guard.as_ref() {
            session.close();
        }
        *guard = None;
    }
}
```

## Step 3: Implement Fingerprinting

In `fingerprint.rs`:

```rust
use super::ExampleAIAgent;
use std::path::PathBuf;

impl ExampleAIAgent {
    pub(crate) async fn do_fingerprint_impl(&self) -> bool {
        // Check for config file
        if let Some(config_path) = find_config_file() {
            common::log_info!("ExampleAI: Found config at {:?}", config_path);

            // Optionally find and cache the binary path
            if let Some(binary_path) = find_binary() {
                let _ = self.process_path.set(binary_path);
            }

            return true;
        }

        // Check for running process
        if is_process_running("exampleai") {
            return true;
        }

        false
    }
}

fn find_config_file() -> Option<PathBuf> {
    let home = dirs::home_dir()?;

    // Check common config locations
    let paths = [
        home.join(".exampleai/config.json"),
        home.join(".config/exampleai/config.json"),
    ];

    paths.into_iter().find(|p| p.exists())
}

fn find_binary() -> Option<String> {
    which::which("exampleai").ok().map(|p| p.to_string_lossy().to_string())
}

fn is_process_running(name: &str) -> bool {
    // Platform-specific process detection
    // ...
    false
}
```

## Step 4: Implement Interception

In `intercept.rs`:

```rust
use super::ExampleAIAgent;
use crate::agent_connectors::traits::AgentIntercept;

impl AgentIntercept for ExampleAIAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        vec!["api.exampleai.com"]
    }

    fn intercept_url_pattern(&self) -> Option<&str> {
        // Optional: regex to filter which URLs to capture
        Some("v1/chat")
    }
}
```

## Step 5: Implement Reconnaissance

In `recon.rs`:

```rust
use super::ExampleAIAgent;
use crate::agent_connectors::traits::AgentRecon;
use async_trait::async_trait;
use common::ReconResult;

#[async_trait]
impl AgentRecon for ExampleAIAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        let mut result = ReconResult::default();

        // Discover configuration files
        if let Some(config) = discover_config() {
            result.config.push(config);
        }

        // Discover tools/plugins
        result.tools = discover_tools();

        // Discover session history
        result.sessions = discover_sessions();

        // For semantic recon, use LLM to extract more info
        if is_semantic {
            // Request semantic parsing from service
            // ...
        }

        Some(result)
    }
}

fn discover_config() -> Option<common::ConfigItem> {
    // Parse config files, return structured data
    None
}

fn discover_tools() -> common::ReconTools {
    // Find plugins, extensions, MCP servers
    common::ReconTools::default()
}

fn discover_sessions() -> Vec<common::SessionItem> {
    // Find session history files
    Vec::new()
}
```

## Step 6: Implement Session Management

In `session.rs`:

```rust
use crate::agent_connectors::traits::{AgentMode, AgentSession};
use anyhow::Result;
use common::SessionContext;
use uuid::Uuid;

pub struct ExampleAISession {
    session_id: Uuid,
    process_path: Option<String>,
    working_dir: Option<String>,
    pty: Option<PtyHandle>,  // Your PTY abstraction
}

impl ExampleAISession {
    pub fn new(process_path: Option<String>, context: &SessionContext) -> Result<Self> {
        let session_id = Uuid::new_v4();

        // Spawn the agent process
        let mut cmd = std::process::Command::new(
            process_path.as_deref().unwrap_or("exampleai")
        );

        if let Some(ref dir) = context.working_dir {
            cmd.current_dir(dir);
        }

        if context.yolo_mode {
            cmd.arg("--auto-approve");
        }

        // Create PTY and spawn
        let pty = create_pty_session(cmd)?;

        Ok(Self {
            session_id,
            process_path,
            working_dir: context.working_dir.clone(),
            pty: Some(pty),
        })
    }
}

impl AgentSession for ExampleAISession {
    fn session_id(&self) -> &Uuid {
        &self.session_id
    }

    fn process_path(&self) -> Option<String> {
        self.process_path.clone()
    }

    fn working_dir(&self) -> Option<String> {
        self.working_dir.clone()
    }

    fn mode(&self) -> AgentMode {
        AgentMode::Cli
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        // Send prompt to PTY stdin
        // Wait for and parse response
        // Return assistant's message

        if let Some(ref pty) = self.pty {
            pty.write(prompt)?;
            let response = pty.read_until_complete()?;
            Ok(parse_response(&response))
        } else {
            Err(anyhow::anyhow!("No PTY available"))
        }
    }

    fn close(&self) {
        if let Some(ref pty) = self.pty {
            pty.close();
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
```

## Step 7: Register in Factory

Update `node/src/agent_connectors/factory.rs`:

```rust
use super::exampleai::ExampleAIAgent;  // Add import

impl AgentFactory {
    pub fn create_all_agents(&self) -> Vec<Arc<dyn Agent>> {
        let mut agents: Vec<Arc<dyn Agent>> = Vec::new();

        agents.push(Arc::new(ClaudeCodeAgent::new()));
        agents.push(Arc::new(GeminiAgent::new()));

        // Add your new agent
        agents.push(Arc::new(ExampleAIAgent::new()));

        #[cfg(windows)]
        agents.push(Arc::new(M365CopilotAgent::new()));

        agents
    }
}
```

Update `node/src/agent_connectors/mod.rs`:

```rust
pub mod exampleai;  // Add this line
```

## Step 8: Test

1. Build the node: `cargo build -p praxis_node`
2. Run with the target agent installed
3. Check fingerprinting works
4. Test reconnaissance
5. Test session creation and prompts
6. Test interception (if implemented)

## Tips

### Fingerprinting

- Be defensive-check multiple locations
- Handle missing files gracefully
- Log what you find for debugging

### Sessions

- Handle terminal control sequences properly
- Parse output carefully-agents have different formats
- Implement proper cleanup on close

### Recon

- Start with static discovery
- Add semantic recon for deeper analysis
- Cache results where appropriate

### Testing

- Test without the agent installed (should not crash)
- Test with partial configuration
- Test session edge cases (timeouts, errors)
