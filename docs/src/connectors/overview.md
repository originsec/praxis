# Agent Connectors Overview

Agent connectors are the modules that let Praxis interact with specific AI agents. Each connector knows how to fingerprint, intercept, and communicate with a particular agent type.

## What Connectors Do

A connector handles four main capabilities:

**Fingerprinting** - Detecting whether an agent is installed, finding its executable path, and extracting its version. The `helpers.find_executable` Lua helper searches PATH, explicit directories, and version manager installations. Version is extracted by running `--version` and parsing the output.

**Interception** - Knowing which domains the agent talks to so traffic can be captured.

**Reconnaissance** - Discovering the agent's configuration, tools, and session history. This includes parsing config files, finding MCP server definitions, and locating past conversations.

**Sessions** - Creating interactive sessions where prompts can be sent and responses received. Different agents need different approaches-CLI agents can be spawned in a PTY, browser-based agents need DevTools or UI automation.

## Current Connectors

| Connector | Agent | Platform | Session Mode | Type |
|-----------|-------|----------|--------------|------|
| [`claude-bridge`](./claude-bridge.md) | Claude Code (inbound) | Any | CCRv1 (WS) / CCRv2 (HTTP+SSE) | Native |
| [`claudecode`](./claude-code.md) | Claude Code CLI | Linux, Windows | CLI (PTY) | Lua |
| [`claudedesktop`](./claude-desktop.md) | Claude Desktop | Windows only | DevTools (Electron) | Lua |
| [`codex`](./codex.md) | Codex CLI (OpenAI) | Linux, Windows | CLI | Lua |
| [`cursor`](./cursor.md) | Cursor Agent CLI | Linux only | CLI | Lua |
| [`gemini`](./gemini.md) | Gemini CLI | Linux, Windows | CLI | Lua |
| [`m365copilot`](./m365-copilot.md) | Microsoft 365 Copilot | Windows only | DevTools | Lua |
| [`pi`](./pi.md) | Pi Coding Agent (`@mariozechner/pi-coding-agent`) | Linux, Windows | CLI | Lua |
| [`praxis`](./praxis.md) | Native LLM agent (provider-agnostic) | Any | ACP (native streaming) | Native |

Want to add support for another agent? Contributions welcome! See [Adding New Connectors](./adding-new.md).

**Note**: Agent implementations change over time. Connectors may break when agents update and will require maintenance to work with the latest versions.

## The Trait System

Connectors implement a set of Rust traits:

```rust
// Required: core agent functionality
trait Agent {
    fn name(&self) -> &str;
    fn short_name(&self) -> &str;
    async fn do_fingerprint(&self) -> bool;  // cached for 60s when available
    fn version(&self) -> Option<String>;     // extracted during fingerprinting
    fn create_session(&self, context: &SessionContext) -> Option<Arc<dyn AgentSession>>;
    // ...
}

// Required for sessions: session management
trait AgentSession {
    fn session_id(&self) -> &Uuid;
    fn transact(&self, prompt: &str) -> Result<String>;
    fn close(&self);
    // ...
}

// Optional: reconnaissance support
trait AgentRecon {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult>;
}
```

Traffic interception is no longer per-agent. The set of domains and URL
filters captured by the proxy is configured centrally in the praxis TUI
under **Settings → Intercept**, and pushed to nodes by the service.
Connectors do not declare intercept domains; they only need to declare a
`short_name` which intercept targets can reference for traffic
attribution.

## Feature Support

Not all agents support all features. The core capabilities — fingerprinting, traffic interception, recon (config/tools/sessions discovery, with optional semantic enrichment of internal tools), and sessions — are supported by most connectors. However, some features depend on how the agent works:

**Config editing** requires the agent to have a file-based configuration that can be modified. CLI agents typically store settings in JSON files that can be edited directly. Browser-based agents often don't expose their configuration in an editable format.

**MCP discovery** only applies to agents that support the Model Context Protocol for tool extensions.

## Lua-Based Connectors

In addition to compiled Rust connectors, Praxis supports writing agent connectors in Lua. Lua scripts are stored in the service database and pushed to nodes via the agent registry.

### Default Scripts

Default Lua agent scripts live in the `agents/` directory at the project root. These are embedded into both the node and service binaries at build time:

- **Node**: Scripts from `agents/` are compiled into the node binary and loaded on startup as fallback connectors.
- **Service**: Scripts are embedded and seeded into the `lua_agent_scripts` database table on first startup. Built-in scripts are tagged with the current Praxis version.

When Praxis is upgraded to a newer version, built-in scripts are automatically updated to the latest version. User-added scripts are never modified by updates.

### Built-in vs User Scripts

Scripts are tagged as either **built-in** or **user**. Built-in scripts ship with Praxis and are automatically updated when the service version changes. User scripts are created through the praxis TUI's **Settings → Agents** tab or uploaded manually and are never overwritten by updates.

Built-in scripts show a "builtin" badge in the script list.

> **Note**: If you need to customize a built-in script, the recommended approach is to:
> 1. Create a new script with your modifications (Settings > Agents > Upload or create new)
> 2. Disable the original built-in script using the toggle in the script list
> 3. Your custom script will be used instead and won't be overwritten on updates
>
> Editing a built-in script directly is possible but not recommended, as your changes will be replaced on the next Praxis update.

### Disabling Scripts

Scripts can be individually enabled or disabled via the toggle icon in the script list. Disabled scripts are not sent to nodes, so the agents they define won't be available. This is useful for:

- Temporarily removing an agent without deleting the script
- Replacing a built-in script with a custom version
- Testing by toggling scripts on and off

### Managing Scripts

Lua agent scripts can be managed through the **Agents** tab in the praxis TUI's **Settings** page (`Ctrl+S`). From there you can:

- View and edit existing scripts
- Upload new `.lua` scripts
- Enable or disable individual scripts
- Delete scripts
- Reset all scripts back to the built-in defaults

When scripts are modified in the database, the service broadcasts an agent registry update to all connected nodes so they reload the latest scripts.

## Adding New Connectors

Want to add support for another agent? See [Adding New Connectors](./adding-new.md) for a step-by-step guide.

For Rust connectors, the basic process is:
1. Create a directory under `node/src/agent_connectors/`
2. Implement the `Agent` trait
3. Add fingerprinting logic
4. Implement interception domains (if applicable)
5. Add reconnaissance (parsing config, finding sessions)
6. Implement session management
7. Register in the factory

For Lua connectors, add a `.lua` file to the `agents/` directory or upload it through the praxis TUI's **Settings → Agents** tab.

## Connector Selection

When a node starts, it runs fingerprinting for all registered connectors. Any agent that fingerprints successfully gets added to the node's agent list and reported to the service. Agent version is also extracted and displayed in the praxis TUI.

Fingerprint results are cached for 60 seconds when the agent is available. Agents that are not found are re-checked on every cycle so they are discovered as soon as they are installed.

Most connectors (Claude Code, Claude Desktop, Codex, Cursor, Gemini, M365 Copilot, Pi) are Lua-based and loaded from embedded scripts or the service database. GUI-based agents like Claude Desktop (Electron) and M365 Copilot (WebView) use the `praxis.cdp_*` native API and `praxis.devtools` Lua library for Chrome DevTools Protocol interaction. The Praxis Agent and the Claude Bridge are native (Rust) connectors — the Praxis Agent is gated by service config (it appears only when enabled and a model definition is selected), and Claude Bridge is always present.

## Development Builds

In debug builds, the environment variable `PRAXIS_IGNORE_SERVICE_AGENTS` controls whether the node uses Lua scripts pushed from the service or only its embedded scripts. It defaults to `1` (ignore service scripts) for development convenience. Set it to `0` to test service-managed scripts:

```bash
PRAXIS_IGNORE_SERVICE_AGENTS=0 cargo run --bin praxis_node
```
