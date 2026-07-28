# Praxis Agent Connector

The Praxis Agent (short name `praxis`) is a native, provider-agnostic LLM connector that runs entirely on the node. Unlike other connectors which wrap an external CLI or browser-based agent, the Praxis Agent has no external binary to fingerprint — the node itself is the agent. It talks directly to a configured LLM endpoint, streams output back over ACP, and exposes a single `run_command` tool for executing shell commands on the host.

## Overview

The Praxis Agent fills a different niche than the other connectors:

- **No external dependency** — the node binary already contains everything needed. There's nothing to install on the target.
- **Cross-platform** — works on Linux, Windows, and macOS without per-OS adaptation.
- **Service-configured** — model selection, thinking effort, system prompt, and the on/off toggle live in the service database. Changes are pushed to nodes via a broadcast and are immediately reflected on subsequent sessions.
- **Provider-agnostic** — uses the shared `common::ai` client, so any provider supported by Praxis (Anthropic, OpenAI, Gemini, OpenRouter, Fireworks, custom OpenAI-compatible endpoints, etc.) works.

The connector is **disabled by default**. It only appears in the agent registry once you turn it on under **Settings → Agents → Praxis Agent** and pick a model definition.

## Configuration

All Praxis Agent settings live service-side in the `service_config` table:

| Key | Type | Description |
|-----|------|-------------|
| `praxis_agent_settings` | JSON | `{ "modelRef": "<model-name>", "thinkingEffort": "<low\|medium\|high>", "enabled": true\|false }` |
| `praxis_agent_system_prompt` | text | Optional custom system prompt. Falls back to the built-in default when empty. |

**`modelRef`** points to a row in your **Settings → LLM → Models** list. The service resolves it into a concrete `PraxisAgentConfig` (provider, API key, endpoint URL, model name) before pushing to the node.

**`thinkingEffort`** is a free-form string forwarded to the model as a sentence appended to the system prompt (e.g. "Requested thinking effort: medium."). Native API thinking budgets are not yet wired up; this is best-effort for models that respond to such hints.

**`enabled`** toggles registration. When false (or when the referenced model can't be resolved) the connector is not added to the registry.

### UI

The praxis TUI exposes these controls under **Settings** (`Ctrl+S`) **→ Agents → Praxis Agent**:

- **Enabled** toggle.
- **Model** dropdown (populated from the LLM Models list).
- **Thinking Effort** input (free-form text).
- **System Prompt** editor (opens in your `$EDITOR`).

## Configuration flow

```diagram
                    Service                                        Node
   ┌────────────────────────────────────┐         ┌─────────────────────────────────────┐
   │ Settings change                    │         │                                     │
   │   praxis_agent_settings updated    │         │ NodeState.factory_config            │
   │            │                       │         │   .praxis_agent_config: Option<…>   │
   │            ▼                       │         │            │                        │
   │ resolve_praxis_agent_config()      │         │            ▼                        │
   │            │                       │         │ AgentFactory.create_all_agents()    │
   │            ▼                       │         │   if Some(cfg) { push PraxisAgent } │
   │ broadcast PraxisAgentEnabled       │ ──────► │            │                        │
   │   { enabled, config }              │         │            ▼                        │
   │                                    │         │ AgentRegistry rebuilt; "praxis"     │
   │ -- on registration --              │         │ entry appears (or disappears).      │
   │ NodeRegistrationAck carries the    │         │                                     │
   │ same {enabled, config} payload.    │         │                                     │
   └────────────────────────────────────┘         └─────────────────────────────────────┘
```

The node never inspects ACP `_meta` for the agent's credentials. Whatever a session reaches, the agent already has its config baked in from the broadcast or registration ack. This keeps the ACP dispatch path transparent and the credential surface narrow.

## Tools

The Praxis Agent exposes a single tool today:

### `run_command`

Execute a shell command on the host.

| Argument | Type | Description |
|---|---|---|
| `command` | string (required) | Shell command. Run with `sh -c` on Unix, `cmd /C` on Windows. |
| `working_dir` | string (optional) | If non-empty, sets the child process's `cwd`. |

**Output format:**

```
exit_code: <code or "terminated by signal">
stdout:
<captured stdout>
stderr:
<captured stderr>
```

**Limits:**

- Wall-clock deadline (default 60s, override via `PraxisAgentConfig.command_timeout_secs`). On timeout the child is killed and the tool result is reported as an error.
- Cancellation: shares the `NodeSession.cancel_flag`. `session/cancel` kills the running command within ~1s.

There is no permission gate; the agent runs every tool call it produces. Treat the Praxis Agent the same way you'd treat any agent with shell access — only enable it on hosts where that's intended.

## Tool calling

The agent currently uses **manual tool-call parsing**: a system-prompt rule teaches the model to emit tool invocations as a JSON block (`{"tool": "run_command", "args": {…}}`) and `parse_manual_tool_call` extracts them from the streamed text. Native function calling (Anthropic `tool_use`, OpenAI `tools`) is a planned follow-up — it requires extending `common::ai::ChatCompletionRequest` with a typed `tools` field across all providers.

Practical implication: the raw tool-call JSON streams to the user as text alongside the structured `ToolCall` notification. Clients render the structured event as an inline tool call (with status updates), but the JSON itself is also visible in the chunk stream.

## Session lifecycle

```diagram
session/new ──► PraxisAgent.create_session_with_id(config)
                 │
                 ▼
              PraxisAgentSession
                 │  (handle = "praxis-<session-uuid>")
                 ▼
session/prompt ─► handler registers SessionUpdateKind sender on `handle`
                 │
                 ▼
              PraxisAgentSession.transact()
                 │  (block_on transact_async)
                 ▼
              ┌─────────────────────────────────────────┐
              │  Loop until no tool call or max iters   │
              │                                         │
              │  1. ChatCompletionRequest.stream()      │
              │     ├──► TextChunk per delta           │
              │     └──► Append to assistant message   │
              │                                         │
              │  2. parse_manual_tool_call(full_text)   │
              │     ├──► None: persist text, return    │
              │     └──► Some(tool):                   │
              │           ├── ToolCall update          │
              │           ├── Run tool (run_command)    │
              │           ├── ToolResult update         │
              │           └── Append tool result        │
              └─────────────────────────────────────────┘
                 │
                 ▼
session/close ─► PraxisAgentSession.close()
                 └─► cleanup_channels(handle)
```

### Streaming

Output is forwarded over the same `crate::acp::register_update_sender` channel that ACP-backed Lua sessions use. The session emits `common::SessionUpdateKind` events:

- `TextChunk { text }` — per-delta text from the LLM stream.
- `ToolCall { tool_name, tool_id, input }` — structured tool invocation.
- `ToolResult { tool_id, output, is_error }` — outcome of the tool.

The node ACP handler translates each event into the appropriate `session/update` JSON-RPC notification.

### Conversation history

The session keeps a persistent message log across `transact()` calls, so multi-turn chats see prior exchanges. The model's actual streamed assistant text (including any tool-call JSON) is what gets written into history — not a synthesized summary — so the next turn sees what the user saw.

### Cancellation

`session/cancel` sets the `NodeSession.cancel_flag`. The session starts with a fresh, local cancel flag at construction, and adopts the caller-supplied flag at the start of each `transact_with_context` call (this connector implements `set_cancel_flag` itself rather than relying on the trait default), so:

- The `chat_completion_stream` loop checks it per delta.
- The tool-call branch checks it before launching `run_command`.
- `run_command` polls it once per second and kills the child.

## Configuration knobs (per-config)

`PraxisAgentConfig` carries the per-node configuration:

| Field | Type | Default | Notes |
|---|---|---|---|
| `provider` | string | (from model def) | `anthropic`, `openai`, `gemini`, `openrouter`, etc. |
| `apiKey` | string | (from model def) | Forwarded to the provider client. |
| `endpointUrl` | string | (from model def) | Trailing slashes trimmed. |
| `modelName` | string | (from model def) | Provider-specific model id. |
| `systemPrompt` | string? | built-in default | Custom prompt, set via `praxis_agent_system_prompt`. |
| `thinkingEffort` | string? | none | Appended to the system prompt as a sentence. |
| `maxToolIterations` | u32? | 10 | Cap on tool-call iterations per `transact`. |
| `commandTimeoutSecs` | u64? | 60 | `run_command` wall-clock deadline. |

Wire format is camelCase. The two configurable limits (`maxToolIterations`, `commandTimeoutSecs`) are reserved for future plumbing; today they're hardcoded defaults exposed in the schema for easy override.

## Architecture notes

- The Praxis Agent is constructed by `AgentFactory` on every registry rebuild. Whenever the service pushes a fresh `PraxisAgentEnabled { enabled, config }`, the runtime calls `factory.set_config(...)` and rebuilds — meaning configuration changes are picked up at the next rebuild without restarting the node.
- `PraxisAgentSession` lives in `node/src/agent_connectors/praxis/session.rs`. It implements the same `AgentSession` trait as Lua sessions, exposing `acp_handle()` so the ACP handler treats both kinds of streaming sessions uniformly.
- There is no fingerprinting step (`do_fingerprint` returns `true` unconditionally) and no version (the connector is part of the node binary). The agent simply appears in the node's agent list when configured.

## Differences from Lua connectors

| | Lua connectors | Praxis Agent |
|---|---|---|
| External dependency | Yes (e.g. `claude`, `cursor`, `pi`) | None |
| Fingerprinting | Probes for binary | Always available |
| Version reporting | Extracted from binary | None |
| Configuration | Detected from agent config files | Pushed by Praxis service |
| Session backend | CLI (PTY) / DevTools / ACP-via-Lua | ACP-native streaming |
| Tool catalog | Whatever the agent natively exposes | `run_command` only (today) |

## Troubleshooting

### The `praxis` connector doesn't appear on a node

- Check **Settings → Agents → Praxis Agent → Enabled** is on.
- Check that a model is selected and that the model definition has a non-empty endpoint URL (or a provider whose default endpoint resolves).
- Watch the service log on save — if the resolved config is `None` you'll see `Praxis agent is enabled but its selected model could not be resolved`.
- The runtime logs `Received PraxisAgentEnabled: enabled (config: present)` when the broadcast arrives. If you only see `(config: absent)`, the resolution failed.

### Sessions stream raw JSON tool-call blocks alongside the structured tool call

Expected with the current manual tool-call parser. The structured `ToolCall` event is what UIs render inline; the raw JSON is part of the underlying assistant text. Native function calling will eliminate this once landed.

### `run_command` cancellation is slow

Cancellation polls every second. A command that's stuck in a syscall longer than that will exit on the next poll. If the host is unresponsive, the kill signal may take longer to propagate.

### "maximum Praxis agent tool iterations (10) reached"

The model emitted a tool call on every iteration without producing a final response. This usually means the prompt is too open-ended or the tool result keeps prompting another tool call. Increase `maxToolIterations` (planned UI), narrow the prompt, or inspect the conversation log on the next turn.
