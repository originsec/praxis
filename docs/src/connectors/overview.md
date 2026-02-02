# Agent Connectors Overview

Agent connectors are the modules that let Praxis interact with specific AI agents. Each connector knows how to fingerprint, intercept, and communicate with a particular agent type.

## What Connectors Do

A connector handles four main capabilities:

**Fingerprinting** - Detecting whether an agent is installed and getting its process path. This usually means checking for config files, finding running processes, or looking in common installation locations.

**Interception** - Knowing which domains the agent talks to so traffic can be captured. For Claude Code, that's `api.anthropic.com`. For Gemini, it's `generativelanguage.googleapis.com`.

**Reconnaissance** - Discovering the agent's configuration, tools, and session history. This includes parsing config files, finding MCP server definitions, and locating past conversations.

**Sessions** - Creating interactive sessions where prompts can be sent and responses received. Different agents need different approaches-CLI agents can be spawned in a PTY, browser-based agents need DevTools or UI automation.

## Current Connectors

| Connector | Agent | Platform | Session Mode |
|-----------|-------|----------|--------------|
| `claudecode` | Claude Code CLI | Linux, Windows, macOS | CLI (PTY) |
| `gemini` | Gemini CLI | Linux, Windows, macOS | CLI (PTY) |
| `m365copilot` | Microsoft 365 Copilot | Windows only | DevTools / UIAutomation |

### Claude Code

Anthropic's command-line AI assistant. The connector:
- Fingerprints via `~/.claude.json` or `~/.config/claude/config.json`
- Intercepts traffic to `api.anthropic.com`
- Discovers MCP servers from config and `~/.claude/mcp.json`
- Creates sessions by spawning `claude` in a PTY

See [Claude Code](./claude-code.md) for details.

### Gemini CLI

Google's command-line AI assistant. The connector:
- Fingerprints via `~/.gemini/settings.json`
- Intercepts traffic to `generativelanguage.googleapis.com`
- Discovers extensions and config
- Creates sessions by spawning `gemini` in a PTY

See [Gemini CLI](./gemini.md) for details.

### M365 Copilot

Microsoft 365 Copilot running in Edge. Windows only. The connector:
- Fingerprints by checking Edge and Copilot availability
- Intercepts traffic to `substrate.office.com` and related
- Discovers capabilities via the web interface
- Creates sessions using Chrome DevTools Protocol (default) or UI Automation

See [M365 Copilot](./m365-copilot.md) for details.

## The Trait System

Connectors implement a set of Rust traits:

```rust
// Required: core agent functionality
trait Agent {
    fn name(&self) -> &str;
    fn short_name(&self) -> &str;
    async fn do_fingerprint(&self) -> bool;
    fn create_session(&self, context: &SessionContext) -> Option<Arc<dyn AgentSession>>;
    // ...
}

// Optional: traffic interception support
trait AgentIntercept {
    fn intercept_domains(&self) -> Vec<&str>;
    fn intercept_url_pattern(&self) -> Option<&str>;
}

// Optional: reconnaissance support
trait AgentRecon {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult>;
}

// Required for sessions: session management
trait AgentSession {
    fn session_id(&self) -> &Uuid;
    fn transact(&self, prompt: &str) -> Result<String>;
    fn close(&self);
    // ...
}
```

## Feature Support

Not all agents support all features. The core capabilities - fingerprinting, traffic interception, static recon, semantic recon, and sessions - are supported by most connectors. However, some features depend on how the agent works:

**Config editing** requires the agent to have a file-based configuration that can be modified. CLI agents typically store settings in JSON files that can be edited directly. Browser-based agents often don't expose their configuration in an editable format.

**MCP discovery** only applies to agents that support the Model Context Protocol for tool extensions. This is typically limited to CLI agents that have MCP server configuration.

## Adding New Connectors

Want to add support for another agent? See [Adding New Connectors](./adding-new.md) for a step-by-step guide.

The basic process:
1. Create a directory under `node/src/agent_connectors/`
2. Implement the `Agent` trait
3. Add fingerprinting logic
4. Implement interception domains (if applicable)
5. Add reconnaissance (parsing config, finding sessions)
6. Implement session management
7. Register in the factory

## Connector Selection

When a node starts, it runs fingerprinting for all registered connectors. Any agent that fingerprints successfully gets added to the node's agent list and reported to the service.

The factory in `node/src/agent_connectors/factory.rs` creates all connector instances:

```rust
pub fn create_all_agents(&self) -> Vec<Arc<dyn Agent>> {
    let mut agents: Vec<Arc<dyn Agent>> = Vec::new();
    agents.push(Arc::new(ClaudeCodeAgent::new()));
    agents.push(Arc::new(GeminiAgent::new()));
    #[cfg(windows)]
    agents.push(Arc::new(M365CopilotAgent::new()));
    agents
}
```
