# Praxis Agent Implementation Plan

## Overview
Implement a native (non-Lua) Praxis agent connector in the node that:
- Talks to an AI endpoint via the existing `common/src/ai/` client (supports all providers)
- Has a tool to run OS commands (Windows/Mac/Linux)
- Is configured from the service: only an enabled/disabled flag is broadcast to nodes
- Per-session config (endpoint, API key, model, thinking effort, system prompt) is passed via ACP `_meta` at session creation time
- Reuses standard ACP fields like `model` when available
- Is exposed via ACP like any other agent

## Generic Factory Architecture
`AgentFactory` takes a `FactoryConfig` struct that can contain arbitrary agent configs.
`FactoryConfig` has `praxis_agent_enabled: bool`.
Future native agents add their own enabled flag to `FactoryConfig`.

## Config Flow
1. Service stores `praxis_agent_settings` as JSON in `service_config` DB:
   - `model_ref`: string referencing an existing model definition (e.g. "anthropic::claude-sonnet-4")
   - `thinking_effort`: string (e.g. "low", "medium", "high")
   - `enabled`: bool
2. Service also stores `praxis_agent_system_prompt` as raw text in `service_config`
3. When settings change, service broadcasts `NodeBroadcastMessage::PraxisAgentEnabled { enabled: bool }`
4. Nodes receive it, store in `NodeState`, rebuild registry via `AgentFactory`
5. On node registration, service broadcasts current enabled state to the new node
6. When opening a Praxis agent session via ACP, the client/service puts resolved config in `_meta.praxis.agentConfig`:
   - `provider`, `api_key`, `endpoint_url`, `model_name`, `thinking_effort`, `system_prompt`
   - Standard ACP `model` field is used for model name when available

## AI Client Reuse
The node PraxisAgent uses `common/src/ai/`:
- `create_ai_client()` with `Provider::from_str()` to support all providers
- `execute_with_tool_parsing()` for tool call detection (manual `{"tool": "...", "args": {...}}` JSON blocks)
- `get_system_prompt_with_tools()` for tool prompt formatting
- `build_message()`, `Message::system()`, `Message::user()`, `Message::assistant()`
- `tokio::runtime::Handle::current().block_on(async { ... })` inside synchronous `transact()`

## System Prompt Editor
- Stored in `service_config` as `praxis_agent_system_prompt` (raw text)
- Web UI: textarea/code editor in Praxis Agent settings section
- CLI/TUI: external editor (same pattern as Lua scripts)

---

## Track A: Common Types + Service Config (Step 1)

### Files:
1. `common/src/messaging.rs` — add `PraxisAgentConfig`, `FactoryConfig`, `PraxisAgentSettings`, `PraxisAgentEnabled` broadcast, extend `SessionContext`
2. `service/src/config/service_config.rs` — add config keys, `PraxisAgentSettings`, resolve method
3. `service/src/dispatch/client.rs` — broadcast on config change
4. `service/src/dispatch/node.rs` — send enabled state on registration

---

## Track B: Node PraxisAgent Core (Step 2, parallel with Web + CLI)

### New files:
- `node/src/agent_connectors/praxis/mod.rs`
- `node/src/agent_connectors/praxis/agent.rs` — `PraxisAgent` implements `Agent`
- `node/src/agent_connectors/praxis/session.rs` — `PraxisAgentSession` implements `AgentSession`

### Session behavior:
- `transact(prompt)` runs async AI completion inside `block_on`
- Uses `common::ai::create_ai_client`, `execute_with_tool_parsing`, `get_system_prompt_with_tools`
- `run_command` tool executes via `tokio::process::Command` with OS shell
- Max 10 tool-call iterations, 60s command timeout, cancellation support

---

## Track C: Node Integration (Step 3, after A + B)

### Files:
- `node/src/agent_connectors/factory.rs` — `FactoryConfig`, conditional `PraxisAgent` creation
- `node/src/agent_connectors/registry.rs` — pass factory config through rebuild
- `node/src/agent_connectors/mod.rs` — add `praxis` module
- `node/src/app/node_state.rs` — add `factory_config`, `last_lua_scripts`
- `node/src/runtime.rs` — handle `PraxisAgentEnabled` broadcast, store lua_scripts
- `node/src/handlers/config_handler.rs` — no changes (broadcast handles it)
- `node/src/acp_server/handlers.rs` — extract `praxis_agent_config` from `_meta` into `SessionContext`
- `node/src/main.rs` — update `AgentFactory::new()` call site

---

## Track D: Web Frontend Settings (Step 2, parallel)

### File:
- `web/frontend/src/components/command/SettingsModal.tsx`
- Add "Praxis Agent" sub-tab in LLM tab
- Model dropdown, thinking effort input, enabled toggle, system prompt textarea
- Load/save `praxis_agent_settings` (JSON) and `praxis_agent_system_prompt` (raw text)

---

## Track E: CLI/TUI Settings (Step 2, parallel)

### Files:
- `cli/src/app/settings.rs` — add praxis fields to `SettingsState`, load/save
- `cli/src/ui/settings/llm.rs` — render praxis section with model, thinking effort, toggle, prompt
- `cli/src/ui/settings/mod.rs` — may need prompt editor state
