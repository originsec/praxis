# Sessions

Sessions let you interact with AI agents in real-time. When you create a session, Praxis spawns the agent process on the target node and gives you a direct communication channel.

## Creating a Session

From the **Nodes** window (`Ctrl+L`) in the praxis TUI, with an agent
selected:

1. Open a session chat
2. Optionally enable **YOLO Mode** and pick a working directory
3. Wait for the session to initialize

The agent process starts on the target node with a PTY attached.

## Session Interface

The chat view shows a conversation:

- Your messages and agent responses interleave in the transcript
- Responses are rendered as markdown with syntax highlighting

Type in the input field and press Enter to send a prompt.

## YOLO Mode

By default, agents require confirmation before executing potentially dangerous actions. YOLO mode auto-approves everything:

- File operations proceed without confirmation
- Commands execute immediately
- Tool calls run automatically

Use YOLO mode when you want uninterrupted operation execution. Be aware that this removes safety guardrails-the agent will do whatever you ask without asking first.

## Session Context

Sessions can be created with context:

**Working Directory** - The directory where the agent operates. This affects file paths and command execution. When running semantic operations or chains from an agent with an active session, the session's working directory is used.

**Prompt Timeout** - Maximum time in seconds a single prompt can run before the agent process is killed. Defaults to the service-wide `prompt_timeout_secs` setting (600 seconds). Can be overridden per-session using the `--timeout` (`-T`) flag in the CLI.

**Session ID** - A unique identifier for tracking the session. Used internally for message routing.

## What Happens During a Session

Clients (the praxis TUI, external ACP tools) never talk to the node
directly. Each prompt is an [Agent Client Protocol](https://agentclientprotocol.com/)
(ACP) JSON-RPC frame that travels client → RabbitMQ → service → RabbitMQ
→ node. The node runs a single ACP server that multiplexes all its
connectors; the target connector is selected per-session via
`_meta.praxis.connector` on `session/new`, and subsequent frames for the
returned `sessionId` are routed by the service proxy automatically.

When you send a prompt:

1. `session/prompt` is forwarded to the node that owns the session
2. The node's per-session Lua VM handles the prompt — invoking the
   connector's PTY (`claude-code`, `codex`, `m365-copilot`) or the
   connector's embedded ACP subprocess (`cursor`, `gemini`)
3. Streaming updates (`session/update` notifications) flow back as the
   agent generates text, calls tools, and builds plans
4. The final `session/prompt` response carries a `stopReason`
   (`end_turn` or `cancelled`)

### Streaming Sessions (ACP)

All sessions are wrapped in ACP externally, but for agents that natively
speak ACP inside the node (currently Cursor and Gemini) you also get
typed streaming updates end-to-end. Regardless of the underlying
transport, `session/update` notifications relay:

- **Text chunks** — incremental output as the agent generates its response
- **Tool calls** — tool name and input displayed as the agent invokes tools
- **Tool results** — output from each tool call (with error highlighting)
- **Plans** — the agent's execution plan with step status tracking
- **Permission requests** — when the agent needs approval for an action (interactive sessions only)
- **Token usage** — prompt/completion token counts updated in real time

Cancellation goes through `session/cancel` (a JSON-RPC notification, no
response) — Ctrl+C in the TUI sends it. The in-flight `session/prompt`
then resolves with
`stopReason: "cancelled"` and any partial output is preserved in the
conversation history.

### Session IDs

Sessions created on a node (via the node's ACP server) are raw UUIDs.
Sessions hosted directly on the service — the orchestrator, MCP-driven
sessions, and external ACP bridges — are prefixed by caller type so a
client can filter the orchestrator session list to its own entries:

- `CLI_` — created by the TUI's orchestrator
- `ACP_` — created by an external ACP client

## Session Messages

The TUI tracks messages per session:

- Messages persist while the session is active
- Conversation history shows the full exchange
- You can save a transcript with `Ctrl+Alt+W`

## Ending a Session

Press Ctrl+C in a chat view (when idle) or `d` on the Active Sessions
overlay to terminate. This sends `session/close` to the node, which
drops the per-session Lua VM and any owned subprocess. Only the targeted
session is affected — any other live sessions on the same connector keep
running.

## Sessions and Operations

Semantic operations always create their own dedicated session. When an
operation runs it calls `session/new`, executes, and then closes. Because
each ACP session owns its own Lua VM (and, where applicable, its own ACP
subprocess or PTY), operations run concurrently with interactive sessions
on the same agent without interfering.

## Bridge Sessions

When Claude Code connects to Praxis via the [Claude Bridge](../connectors/claude-bridge.md), a session is created automatically as part of the connection. Bridge sessions differ from regular sessions:

- The session starts immediately when Claude connects (no manual creation needed)
- Permissions are always bypassed (YOLO mode) since the bridge sets `bypassPermissions` during handshake
- Only one prompt can be in-flight at a time
- Closing the session sends an `end_session` request to Claude and terminates the connection
- The virtual node is deregistered when the session ends

Bridge sessions are otherwise used the same way -- you can send prompts, run operations, and include them in chains.

## Multiple Sessions

A single node can host any number of concurrent ACP sessions across any
combination of connectors. Each `session/new` returns a fresh `sessionId`,
and every session gets its own isolated per-session Lua VM built from
bytecode compiled once at connector-load time, so there is no global
state shared between sessions even when they target the same connector.

### Listing and resuming

The TUI refreshes its view of live sessions by calling `session/list` on
each connected node. It does this on first connect, when you open the
Nodes window (`Ctrl+L`), and ~1.5s after a node reset. Any server-side
sessions the TUI hadn't yet seen — for example a session left alive
across a TUI restart — are merged into the local sessions list and
become resumable.

### In the TUI

`Ctrl+W` in the Nodes window toggles the **Active Sessions** overlay. It
lists every live session with node, agent, session id preview, status
(`idle` / `working`), and how long ago it was created.

- `Enter` resumes the selected session
- `d` or `Del` discards (sends `session/cancel` if the session is
  mid-prompt, then `session/close`)
- `Esc` or `Ctrl+W` dismisses the overlay

Inside a chat view, `Esc` or `Ctrl+W` **pauses** the session (hides the
chat; the session stays alive on the node and can be resumed from the
overlay). `Ctrl+C` cancels the in-flight prompt when the agent is
working, and closes the session when the agent is idle. The status bar
shows an `N sessions` counter when any concurrent sessions are live.

## Troubleshooting

### Session won't create

- Check the agent binary exists on the node
- Verify the node is connected
- Look at node logs for spawn errors

### Messages not appearing

- Ensure the session is active (check the indicator)
- Try toggling away and back to the chat view
- Check the TUI's RabbitMQ connection status (`praxis --status`)

### Session hangs

- The agent may be waiting for input
- Check if YOLO mode should be enabled
- Try sending a simpler prompt

### Unexpected responses

- Remember the agent has full system access
- Previous conversation context affects responses
- Try closing and creating a fresh session
