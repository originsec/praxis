# Adding New Connectors

This guide walks through creating a connector for a new AI agent.

**Prefer Lua connectors** for CLI-based agents. Lua scripts are easier to write, can be updated at runtime via the web UI without recompiling, and share common helpers for executable discovery, version extraction, and multi-user support. Use Rust connectors only when you need OS-level capabilities (DevTools, UI automation, process injection) that aren't exposed through the Lua API.

## Lua Connector (Recommended for CLI agents)

Lua agent scripts live in `agents/` at the project root and are embedded into binaries at build time. They can also be uploaded via the web UI (Settings > Agents).

### Script Structure

A Lua connector returns a table with a `name`, `short_name`, and callback functions:

```lua
local helpers = require("praxis.helpers")

local process_path = nil
local process_version = nil

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
  name = "Example AI",
  short_name = "exampleai",

  fingerprint = function(_ctx)
    process_path, process_version = pick_path()
    return {
      available = process_path ~= nil,
      process_path = process_path,
      version = process_version,
    }
  end,

  -- Optional: traffic interception domains
  intercept_domains = function(_ctx)
    return { "api.exampleai.com" }
  end,

  -- Optional: reconnaissance
  recon = function(is_semantic)
    -- Discover config files, sessions, tools
    return { config = {}, sessions = {}, project_paths = {} }
  end,

  -- Required for sessions
  create_session = function(ctx)
    return {
      handle = praxis.uuid_v4(),
      process_path = ctx.process_path or process_path,
      working_dir = ctx.working_dir,
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
    -- Cleanup if needed
  end,
}
```

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

The `praxis` global provides filesystem operations (`path_exists`, `path_join`, `read_file`, `walk_files`, `glob_files`), command execution (`command_run`, `command_run_handle`), environment access (`os_name`, `user_homes`, `env_get`, `expand_path`), and utilities (`json_decode`, `toml_decode`, `uuid_v4`, `now_unix`, `log_info`, `log_warn`).

The `helpers` module (`require("praxis.helpers")`) provides `find_executable`, `expand_path`, `starts_with`, `ends_with`, `dedup`, `parse_json`, `parse_toml`, `user_homes_with_dir`, `for_each_user_home_coalesce`, `new_recon_result`, `merge_recon_result`, `discover_internal_tools`, and `extract_metadata`.

### Deploying

- **Embedded**: Add the `.lua` file to `agents/` and rebuild. It will be compiled into both node and service binaries.
- **Runtime**: Upload via Settings > Agents in the web UI. The script is stored in the service database and pushed to all connected nodes.

---

## Rust Connector (for native/OS-level agents)

Use this approach only when Lua cannot access the required OS capabilities. The M365 Copilot connector is the primary example — it uses Windows DevTools and UI Automation APIs.

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
