# Orchestrator

The Orchestrator is an interactive AI agent that can autonomously manage nodes, agents, sessions, operations, and chains across the Praxis network. Unlike semantic operations (which run predefined tasks), the Orchestrator is a free-form conversational interface where you give high-level goals and the AI figures out the steps.

## Prerequisites

Before using the Orchestrator, you need:

1. **MCP Server enabled** — Go to **Settings** > **MCP Server** and enable it. The Orchestrator connects to this server to access all Praxis tools.

2. **Orchestrator LLM configured** — Go to **Settings** > **LLM Providers** and configure a model definition, then assign it to the Orchestrator feature in the Feature Selection section.

If the MCP server is not enabled when you start a session, you'll see an error message directing you to the settings page.

## Starting a Session

1. Click **Orchestrator** in the sidebar
2. Click **New Session**
3. The Orchestrator connects to the MCP server and fetches available tools
4. Type your goal or question in the input box

## What It Can Do

The Orchestrator has access to all Praxis MCP tools:

- **Node management** — List nodes, select nodes, request info updates
- **Agent control** — List agents, select agents, run recon (static and semantic), query stored recon data (sessions, projects, tools)
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

When using a model that supports extended thinking (e.g. Claude Sonnet/Opus with thinking enabled), the Orchestrator surfaces the model's reasoning steps inline. Thinking blocks appear in a collapsed section before the final response, showing the chain of reasoning the model used to arrive at its answer.

Thinking mode is enabled automatically when the configured Orchestrator model supports it and has thinking enabled in its API parameters. No separate configuration is needed in Praxis.

## Plan Tracking

The Orchestrator can break complex tasks into steps and show progress via the `report_plan` tool. When the AI calls this tool, you'll see a plan panel with step descriptions and their current status (not started, in progress, done).

## Token Usage

Token usage is displayed after each LLM call, showing prompt tokens, completion tokens, and totals. This helps monitor costs when using commercial API providers.

## Session Controls

- **Cancel** — Stops the current inference but keeps the session alive. Useful if the AI is going in the wrong direction.
- **Stop** — Ends the session entirely. You'll need to start a new session to continue.

## Model Recommendations

The Orchestrator requires a capable model that can follow tool-calling instructions reliably:

**Recommended:**
- Anthropic: Claude Sonnet 4 or Claude Opus 4
- OpenAI: GPT-4o
- Google: Gemini 1.5 Pro

**Not recommended:**
- Smaller/faster models (Haiku, GPT-4o-mini) — these often fail to follow the tool calling format or hallucinate results

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

Go to **Settings** > **MCP Server** and enable it. The Orchestrator requires the MCP server to function.

### "Failed to connect to MCP server"

- Verify the MCP server is running (check the Settings page for status)
- Check that the configured port is not in use by another process
- Look at service logs for MCP server startup errors

### Tools not executing

- Ensure you're using a capable model (see recommendations above)
- Check the tool execution results for error messages
- Verify nodes are connected and agents are available

### Session disconnects

The MCP client connection is tied to the Orchestrator session. If the MCP server restarts, you'll need to start a new Orchestrator session.
