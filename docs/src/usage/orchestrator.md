# Orchestrator

The Orchestrator is an interactive AI agent that can autonomously manage nodes, agents, sessions, operations, and chains across the Praxis network. Unlike semantic operations (which run predefined tasks), the Orchestrator is a free-form conversational interface where you give high-level goals and the AI figures out the steps.

## Prerequisites

Before using the Orchestrator, you need:

1. **MCP Server enabled** — Open **Settings** with `Ctrl+S`, select
   **Service**, and enable **MCP Server**. The Orchestrator connects to the
   [MCP server](./mcp.md) as a client to access all Praxis tools.

2. **Orchestrator LLM configured** — Open **Settings** with `Ctrl+S`, select
   **LLM**, configure a model definition, then assign it to the Orchestrator
   feature.

If the MCP server is not enabled when you start a session, you'll see an error message directing you to the settings page.

## Starting a Session

1. Open the **Orchestrator** window with `Ctrl+O`
2. Type your goal or question — the session is opened on demand
3. Press `Enter`; the Orchestrator connects to the MCP server and fetches available tools

## Sessions and State

Ordinary chat sessions remain in memory on the service. The TUI keeps one in-flight session against the service; when the TUI disconnects or closes the session, that conversation is dropped server-side. [Durable missions](./missions.md) provide persistent work, checkpoints, and explicit recovery independently of the chat session.

- **TUI (`praxis`)** — One session per TUI process. The TUI mirrors every turn to `~/.praxis/sessions/<session_id>.json` so you can resume later:
  - `praxis --continue` resumes the most recent saved session.
  - `praxis --resume` lists saved sessions and prompts you to pick one.

  When resuming, the saved transcript is shown immediately and the prior turns are sent as conversation history with `session/new` so the model has recent context for the next prompt. Before every model turn, the service estimates context size against a 24,000-token history budget. It keeps the system instruction and initial user objective, preserves recent complete tool exchanges, and replaces older turns with bounded, explicitly incomplete extracts. Large tool results may be truncated in model context. This is a heuristic budget, not a provider tokenizer or a guarantee that an exceptionally large pinned objective will fit the selected model. The TUI resume transcript contains text, so a resumed chat is fresh model context rather than a continuation of provider-signed native tool history.

## What It Can Do

The Orchestrator has access to all Praxis MCP tools:

- **Node management** — List nodes, select nodes, request info updates
- **Agent control** — List agents, target agents explicitly, run recon (static and semantic), query stored recon data (sessions, projects, tools)
- **Sessions** — Create sessions, send prompts, close sessions
- **Operations** — List, run, monitor, and cancel semantic operations
- **Chains** — List, run, monitor, and cancel chain workflows
- **Traffic** — Search intercepted traffic with regex patterns

Plus two local tools:
- **wait** — Sleep for a specified duration (useful when polling operation status)
- **report_plan** — Show a step-by-step execution plan with progress tracking

## Example Prompts

**Simple exploration:**
> List all connected nodes and their agents

**Multi-step task:**
> Connect to the first available node, select the Claude Code agent, create a YOLO session, and ask it to list the files in the current directory

**Operation execution:**
> Run the recon::system_info operation on all active nodes and report the results

**Monitoring:**
> Check the status of all running operations and cancel any that have been running for more than 5 minutes

## Thinking Mode

If a model emits literal `<think>...</think>` tags in its plain-text output, the Orchestrator splits them out and renders them inline as a distinct segment before the rest of the response. This is plain-text parsing, not an API-level extended-thinking integration — there is no model-capability detection and no collapse/expand toggle; a thinking segment is always shown inline when present. Whether a model emits `<think>` tags at all depends entirely on the model/provider, not on any Praxis setting.

## Plan Tracking

The Orchestrator can break complex tasks into steps and show progress via the `report_plan` tool. When the AI calls this tool, you'll see a plan panel with step descriptions and their current status (not started, in progress, done).

## Concurrent Tool Calls

The Orchestrator sends native JSON Schema tool definitions by default. It collects complete provider tool calls, validates tool names and required arguments, then runs independent calls **concurrently**. All results return with their original call IDs before the next model turn. Dependent steps still run across turns: call prerequisites first, wait for results, then continue. Calls are never executed from an incomplete or failed model stream.

For an OpenAI-compatible endpoint that lacks native tool support, set the service configuration key `llm_orchestrator_native_tools` to `false` and create a new chat session. This explicitly enables the existing text-parser protocol; there is no automatic downgrade after a provider error. Native mode never interprets JSON printed in ordinary assistant text as a tool call. See [native AI tool compatibility](../development/ai-tools.md#provider-compatibility) for supported protocols and model limitations.

This is useful for fan-out work such as listing nodes and available operations at the same time, or queueing `op_run` against several nodes in one step.

## Token Usage

Token usage is displayed after each LLM call, showing prompt tokens, completion tokens, and totals. This helps monitor costs when using commercial API providers.

## Session Controls

`Ctrl+C` cancels an in-flight inference or tool wait while keeping the conversation, including when the provider has stopped sending chunks. A chat session accepts one active prompt at a time. Cancellation during a tool batch records an uncertain outcome for every unresolved call: a remote action may already have occurred, so the Orchestrator must inspect state before trying it again. Cancellation does not automatically replay interrupted calls. Type
`/clear` and press `Enter` to start a fresh conversation. See
[Terminal UI](./tui.md#orchestrator-ctrlo) for the complete Orchestrator
controls.

## Model Compatibility

Choose a model that supports native function calling on the configured provider
endpoint. Praxis currently uses OpenAI-compatible Chat Completions, Anthropic
Messages, or Gemini Generate Content. A model that requires OpenAI's Responses
API for tools cannot use that protocol through the current Chat Completions
transport. Configure a compatible model or explicitly use text compatibility
mode when appropriate for the endpoint. Model selection remains under operator
control.

## How It Differs from Semantic Operations

| Aspect | Orchestrator | Semantic Operations |
|--------|-------------|-------------------|
| Interface | Interactive chat | Predefined tasks |
| Scope | Full Praxis network | Single node/agent |
| Tools | All MCP tools | `session_prompt` only (agent mode) |
| Use case | Ad-hoc exploration, complex multi-node tasks | Repeatable, automated tasks |

The Orchestrator is best for exploration, debugging, and complex ad-hoc tasks. Semantic operations are better for repeatable workflows that you want to run consistently.

## Troubleshooting

### "MCP server is not enabled"

Open **Settings** with `Ctrl+S`, select **Service**, and enable **MCP Server**.
The Orchestrator requires the MCP server to function.

### "Could not connect to the MCP server"

The Orchestrator connects to the MCP server when the session is created, so a
connection problem is reported up front with the reason (rather than surfacing
later as an opaque error on your first prompt). If you see this:

- Verify the MCP server is running in **Settings** (`Ctrl+S`) → **Service**
- Check that the configured port is not in use by another process
- Look at service logs for MCP server startup errors
- If you just enabled the MCP server, give it a moment to bind, then start the
  session again

### Tools not executing

- Check that the selected model supports the configured tool protocol (see compatibility above)
- Check the tool execution results for error messages
- Verify nodes are connected and agents are available

### Session disconnects

If the **service** restarts, its in-memory Orchestrator sessions are lost. The
next prompt you send is detected as targeting a lost session, and the TUI
automatically starts a fresh session (re-seeding it with the prior transcript)
and resends your prompt — no action needed. If recovery can't re-establish the
session, use `/clear` to start a clean one.

`/clear` uses standard ACP: `session/close` then `session/new`. The service
keeps a **shared** MCP connection for the orchestrator, so after the first
session of the process, close+new is near-instant (conversation only; no MCP
reconnect).

To bring back a prior conversation deliberately, start the TUI with
`praxis --continue` (or `--resume`): the saved transcript is replayed locally
and re-seeded as history for the next prompt.
