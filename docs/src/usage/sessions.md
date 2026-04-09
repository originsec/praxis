# Sessions

Sessions let you interact with AI agents in real-time. When you create a session, Praxis spawns the agent process on the target node and gives you a direct communication channel.

## Creating a Session

From the agent detail page:

1. Click **Create Session**
2. Optionally enable **YOLO Mode**
3. Wait for the session to initialize

The agent process starts on the target node with a PTY attached. You'll see a session indicator once it's ready.

## Session Interface

The session panel shows a conversation view:

- **Your messages** appear on one side
- **Agent responses** appear on the other
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

When you send a prompt:

1. Text goes to the node via RabbitMQ
2. Node writes to the agent's PTY stdin (or sends via ACP for supported agents)
3. Agent processes the prompt
4. Response comes back through the PTY or as streaming updates via ACP
5. Node parses and extracts the response
6. Response appears in the UI

### Streaming Sessions (ACP)

Agents that support the Agent Communication Protocol (ACP) -- currently Cursor and Gemini -- provide real-time streaming updates during prompt execution. Instead of waiting for the full response, you see:

- **Text chunks** — incremental output as the agent generates its response
- **Tool calls** — tool name and input displayed as the agent invokes tools
- **Tool results** — output from each tool call (with error highlighting)
- **Permission requests** — when the agent needs approval for an action (interactive sessions only)

Streaming sessions also support cancellation -- pressing Ctrl+C (TUI) or clicking Cancel (web UI) sends a cancel signal that interrupts the agent mid-response. Any partial output is preserved in the conversation history.

## Session Messages

The UI tracks messages per session:

- Messages persist while the session is active
- Conversation history shows the full exchange
- You can export the session transcript

## Ending a Session

Click **Close Session** to terminate. This:

1. Sends a close command to the node
2. Terminates the agent process
3. Clears the session state
4. Updates the UI

The agent returns to the fingerprinted state.

## Sessions and Operations

Semantic operations always create their own dedicated sessions. When an operation runs, it spawns a fresh session, executes, and closes it.

**Warning**: Running an operation will implicitly end any open interactive session you have with that agent. Interactive sessions and operation sessions should not be expected to run concurrently - an agent supports one session at a time.

## Bridge Sessions

When Claude Code connects to Praxis via the [Claude Bridge](../connectors/claude-bridge.md), a session is created automatically as part of the connection. Bridge sessions differ from regular sessions:

- The session starts immediately when Claude connects (no manual creation needed)
- Permissions are always bypassed (YOLO mode) since the bridge sets `bypassPermissions` during handshake
- Only one prompt can be in-flight at a time
- Closing the session sends an `end_session` request to Claude and terminates the connection
- The virtual node is deregistered when the session ends

Bridge sessions are otherwise used the same way -- you can send prompts, run operations, and include them in chains.

## Multiple Sessions

Each agent can have one active session at a time. To work with a different agent:

1. Close the current session (or leave it open)
2. Select the other agent
3. Create a new session

Sessions are per-node, per-agent.

## Troubleshooting

### Session won't create

- Check the agent binary exists on the node
- Verify the node is connected
- Look at node logs for spawn errors

### Messages not appearing

- Ensure the session is active (check the indicator)
- Try refreshing the page
- Check WebSocket connection status

### Session hangs

- The agent may be waiting for input
- Check if YOLO mode should be enabled
- Try sending a simpler prompt

### Unexpected responses

- Remember the agent has full system access
- Previous conversation context affects responses
- Try closing and creating a fresh session
