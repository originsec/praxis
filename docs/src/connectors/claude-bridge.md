# Claude Bridge (CCRv1 / CCRv2)

The Claude Bridge lets Claude Code connect directly to Praxis without a deployed node. Instead of Praxis spawning Claude as a child process, Claude connects *inward* to the service using Anthropic's Claude Code Router protocol. Each connection registers as a virtual node with an active session.

## Overview

Traditional Praxis nodes discover Claude Code on the target machine, fingerprint it, and spawn it in a PTY for sessions. The Claude Bridge reverses this: the Praxis service listens on a port, and Claude Code connects to it as a remote worker. This is useful when:

- Claude is already running (e.g. in an IDE, desktop app, or cloud environment) and you want to bring it under Praxis control
- You want to avoid deploying a full Praxis node to the target machine
- You are building integrations that launch Claude Code with custom environment variables

The bridge implements two protocol versions that correspond to the two transport modes Claude Code supports.

## Protocol Versions

### CCRv1 (WebSocket)

CCRv1 uses a bidirectional WebSocket connection with newline-delimited JSON (NDJSON). This is the simpler protocol -- Claude connects via `ws://` and all messages flow over a single WebSocket.

**Default port**: 8586

**Wire format**: Each message is `JSON.stringify(msg) + "\n"` sent as a WebSocket text frame. Multiple JSON objects may arrive in a single frame.

**Handshake**:
1. Claude opens a WebSocket connection to the bridge
2. Bridge sends `initialize` control request
3. Claude responds with `control_response` and `system/init`
4. Bridge sends `set_permission_mode` (bypassPermissions)
5. Bridge registers as a virtual node with the service

### CCRv2 (HTTP + SSE)

CCRv2 uses HTTP POST for client-to-server messages and Server-Sent Events (SSE) for server-to-client messages. This is the newer protocol used by Anthropic's cloud infrastructure.

**Default port**: 8587

**Endpoints**:

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/worker` | GET | Returns worker metadata |
| `/worker` | PUT | Worker status updates (idle/processing) |
| `/worker/events` | POST | Batched messages from Claude to bridge |
| `/worker/events/stream` | GET | SSE stream from bridge to Claude |
| `/worker/internal-events` | POST | Internal events (ack with epoch check) |
| `/worker/heartbeat` | POST | Keep-alive (every ~20s from Claude) |
| `/worker/events/delivery` | POST | Event delivery confirmation |

**Epoch tracking**: CCRv2 uses a `worker_epoch` integer that appears in every request. If a stale worker reconnects with an old epoch, the server returns 409 Conflict and Claude exits. This prevents ghost sessions from interfering with new ones.

**Disconnect detection**: If no activity is received for 45 seconds (heartbeats normally arrive every 20s), the bridge treats the worker as disconnected and tears down the session. SSE disconnection also triggers immediate teardown.

## Enabling the Bridge

Both bridge versions are disabled by default. Enable them in the web UI under **Settings** > **Claude Bridge**, or in the CLI TUI under **Settings** (`Ctrl+S`) > **Service** tab.

| Setting | Default | Description |
|---------|---------|-------------|
| CCRv1 Enabled | `false` | Enable the WebSocket bridge listener |
| CCRv1 Port | `8586` | Port for WebSocket connections |
| CCRv2 Enabled | `false` | Enable the HTTP+SSE bridge listener |
| CCRv2 Port | `8587` | Port for HTTP connections |

Changes take effect immediately -- the bridge starts or stops without restarting the service.

## Connecting Claude Code

To make Claude Code connect to a Praxis bridge instead of Anthropic's servers, launch it with the appropriate environment variables and the `--sdk-url` flag pointing to your bridge URL, with the specified stream-json I/O formats.

### CCRv1 (WebSocket)

```powershell
$env:CLAUDE_CODE_SESSION_ACCESS_TOKEN = "local-token"
claude --sdk-url ws://localhost:8586 --output-format stream-json --input-format stream-json
```

The `CLAUDE_CODE_SESSION_ACCESS_TOKEN` is passed as an `Authorization: Bearer` header on the WebSocket upgrade request. The Praxis bridge does not validate the token, so any non-empty value works. You can also omit it entirely for CCRv1 -- the WebSocket transport accepts empty auth headers.

### CCRv2 (HTTP + SSE)

```powershell
$env:CLAUDE_CODE_USE_CCR_V2 = "1"
$env:CLAUDE_CODE_WORKER_EPOCH = "1"
$env:CLAUDE_CODE_SESSION_ACCESS_TOKEN = "local-token"
claude --sdk-url http://localhost:8587 --output-format stream-json --input-format stream-json
```

CCRv2 has stricter requirements:

| Variable | Required | Description |
|----------|----------|-------------|
| `CLAUDE_CODE_USE_CCR_V2` | Yes | Set to `"1"` to select the SSE+POST transport |
| `CLAUDE_CODE_WORKER_EPOCH` | Yes | Integer epoch (e.g. `"1"`). Must be present and numeric or Claude exits with `missing_epoch` |
| `CLAUDE_CODE_SESSION_ACCESS_TOKEN` | Yes | Auth token. Claude exits with `no_auth_headers` if missing. A dummy value like `"local-token"` works since the bridge does not validate tokens |

### Environment Variable Reference

| Variable | V1 | V2 | Description |
|----------|:--:|:--:|-------------|
| `CLAUDE_CODE_SESSION_ACCESS_TOKEN` | optional | **required** | Bearer token for auth. V1 accepts empty headers. V2 crashes without it. A dummy value works for local bridges. |
| `CLAUDE_CODE_USE_CCR_V2` | N/A | **required** | When `"1"`, selects SSE transport. Without it, falls back to WebSocket (V1). |
| `CLAUDE_CODE_WORKER_EPOCH` | N/A | **required** | Integer epoch for V2 requests. Missing or non-numeric causes `missing_epoch` error. |
| `CLAUDE_CODE_ENVIRONMENT_KIND` | optional | optional | Set to `"bridge"` for minor diagnostic effects. Not functionally required. |

### Auth Token Resolution

Claude Code resolves auth tokens in this order:
1. `CLAUDE_CODE_SESSION_ACCESS_TOKEN` environment variable
2. File descriptor via `CLAUDE_CODE_WEBSOCKET_AUTH_FILE_DESCRIPTOR`
3. Well-known file at `CCR_SESSION_INGRESS_TOKEN_PATH` (or `CLAUDE_SESSION_INGRESS_TOKEN_FILE`)

If all return null, V2 crashes and V1 proceeds with empty headers.

## How Bridge Nodes Appear

When Claude connects, the bridge registers a virtual node with the service. This node appears in the web UI and CLI just like a deployed node, with some differences:

- **Node type**: `claude-ccrv1` or `claude-ccrv2` (shown in the UI)
- **Machine name**: Same as the node type
- **Capabilities**: Session only (no interception, recon, or terminal)
- **Agent**: Claude Code (auto-selected, with version reported from the `system/init` message)
- **Session**: Automatically active in YOLO mode (bypassPermissions)
- **Working directory**: Reported by Claude's `system/init` message (the cwd where Claude was launched)

Bridge nodes are ephemeral -- they exist only while Claude is connected. When Claude disconnects, the node is automatically deregistered and disappears from the UI.

## Using Bridge Sessions

Once connected, a bridge session works like any other Praxis session. You can:

- Send prompts from the web UI or CLI
- Run semantic operations against the bridge node
- Include bridge nodes in chain workflows
- Use the orchestrator with bridge nodes

The key difference is that permissions are always bypassed (YOLO mode) -- Claude auto-approves all tool calls since the bridge sets `bypassPermissions` during the handshake.

One session exists per connection. Closing the session from Praxis sends an `end_session` control request to Claude, which terminates the process. Only one prompt can be in-flight at a time; sending a second prompt while one is active returns an error.

## Troubleshooting

### Claude exits immediately after connecting

**CCRv2**: Ensure all three required environment variables are set (`CLAUDE_CODE_USE_CCR_V2`, `CLAUDE_CODE_WORKER_EPOCH`, `CLAUDE_CODE_SESSION_ACCESS_TOKEN`). Missing any of them causes Claude to exit with a specific error.

**Both versions**: Check that the bridge is enabled and the port is correct. Look at the service logs for connection/handshake errors.

### Node appears but no session

The bridge waits up to 30 seconds for the handshake to complete. If Claude does not respond to the `initialize` control request in time, the session fails. Check Claude's output for errors (API key issues, network problems, etc.).

### "Prompt already in-flight" error

Bridge sessions only support one concurrent prompt. Wait for the current response before sending another. If a prompt appears stuck, cancel the transaction or close the session.

### Node disappears unexpectedly

Bridge nodes are tied to the connection. If Claude crashes, the network drops, or the process is killed, the node is immediately deregistered. For CCRv2, the 45-second silence timeout also triggers cleanup if heartbeats stop.

### CCRv2 epoch mismatch (409)

This means a stale worker is trying to use an old epoch. Increment `CLAUDE_CODE_WORKER_EPOCH` when relaunching Claude, or simply restart the bridge (toggle the setting off and on).
