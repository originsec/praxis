# Orchestrator

The Orchestrator is an interactive AI agent that can autonomously manage nodes, agents, sessions, operations, and chains across the Praxis network. Unlike semantic operations (which run predefined tasks), the Orchestrator is a free-form conversational interface where you give high-level goals and the AI figures out the steps.

## Prerequisites

Before using the Orchestrator, you need:

1. **MCP Server enabled** — Open **Settings** with `Ctrl+S`, select
   **Service**, and enable **MCP Server**. The Orchestrator connects to the
   [MCP server](./mcp.md) as a client to access all Praxis tools.

2. **Orchestrator LLM configured** — Open **Settings** with `Ctrl+S`, select
   **LLM**, configure a model definition, then assign it to the Orchestrator
   feature. See [Model Recommendations](#model-recommendations).

If the MCP server is not enabled when you start a session, you'll see an error message directing you to the settings page.

## Starting a Session

1. Open the **Orchestrator** window with `Ctrl+O`
2. Type your goal or question — the session is opened on demand
3. Press `Enter`; the Orchestrator connects to the MCP server and fetches available tools

## Sessions and State

The service holds **no conversation state** for the Orchestrator beyond the
live session. The TUI keeps one in-flight session against the service; when
the TUI disconnects or closes the session, the conversation is dropped
server-side. Two things *do* persist on the service: [playbooks](#playbooks)
and [engagement memory](#engagement-memory).

- **TUI (`praxis`)** — One session per TUI process. The TUI mirrors every turn to `~/.praxis/sessions/<session_id>.json` so you can resume later:
  - `praxis --continue` resumes the most recent saved session.
  - `praxis --resume` lists saved sessions and prompts you to pick one.

  When resuming, the saved transcript is shown immediately and the prior
  turns are sent as conversation history with `session/new` so the model has
  context for the next prompt. Resumed sessions reopen in the engagement they
  were saved under. Reasoning is display-only and is not saved.

Long conversations are [compacted](#context-compaction) rather than
truncated.

## What It Can Do

The Orchestrator has access to all Praxis MCP tools:

- **Node management** — List nodes, select nodes, reset nodes
- **Agent control** — List agents, run recon (static and semantic), query stored recon data (sessions, projects, tools, configs)
- **Sessions** — Create sessions, send prompts, close sessions
- **Operations and chains** — List, create, run, monitor, cancel and delete operations; create chains
- **Triggers** — Create, list, toggle and delete chain triggers
- **Traffic** — Search intercepted traffic with regex patterns
- **Playbooks** — Save or delete [playbooks](#playbooks)

Plus local tools that run inside the service:

| Tool | Purpose |
|------|---------|
| `wait` | Sleep 1-15 seconds (useful when polling operation status) |
| `report_plan` | Show a step-by-step execution plan with progress tracking |
| `load_playbook` | Load the instructions of a [playbook](#playbooks) |
| `delegate` | Run a [sub-orchestrator](#sub-orchestrators) on a scoped sub-task |
| `memory_write` / `memory_read` / `memory_search` | [Engagement memory](#engagement-memory) |

## Example Prompts

**Simple exploration:**
> List all connected nodes and their agents

**Multi-step task:**
> Connect to the first available node, select the Claude Code agent, create a YOLO session, and ask it to list the files in the current directory

**Operation execution:**
> Run the recon::system_info operation on all active nodes and report the results

**Playbook-driven:**
> Do a fleet recon and remember what you find

**Monitoring:**
> Check the status of all running operations and cancel any that have been running for more than 5 minutes

## Tool Calling

The Orchestrator offers its tools through each provider's **native
function-calling API**:

- **Anthropic** — Messages API `tools`, `tool_use` and `tool_result` blocks
- **OpenAI-compatible providers** — Chat Completions `tools` / `tool_calls`
  and `tool` messages
- **Google Gemini** — `functionDeclarations`, `functionCall` and
  `functionResponse` parts (thought signatures are replayed as required)

Native calling is the default for every provider except **Ollama** and
**Custom** endpoints, which serve arbitrary local models and default to
**prompt-based (manual) tool calling**: the tool list is written into the
system prompt and the model replies with `{"tool": ..., "args": ...}` JSON
that the Orchestrator parses (including the `<tool_call>` wrapper some local
models use).

Override the mode in **Settings** → **Orchestrator** → **Tool Calling**
(`llm_orchestrator_tool_mode`): `auto` (provider default), `native` or
`manual`. If an endpoint rejects native tools before producing any output
(for example "model does not support tools"), the session automatically
switches to manual mode and carries on.

Tool calls whose arguments arrive as invalid JSON (usually because the
response hit the output-token limit) are not executed; the model receives an
error result and is asked to retry.

## Reasoning

When **Settings** → **Orchestrator** → **Reasoning** is on
(`llm_orchestrator_thinking`, default on), the Orchestrator asks the model for
its reasoning and shows it as a dimmed thinking segment above the answer:

- **Anthropic** — adaptive thinking with summarized display on current models
  (Claude 4.6 and later); a fixed thinking budget on older thinking-capable
  models. Thinking blocks, including their signatures, are kept in the
  conversation and passed back on tool-use turns as the API requires.
- **Gemini** — thought summaries (`includeThoughts`) on Gemini 2.5 and later.
- **OpenAI-compatible** — `reasoning_content` / `reasoning` fields returned by
  servers that expose them (DeepSeek, OpenRouter, vLLM, Ollama, Groq and
  others). OpenAI's own reasoning summaries are only available through the
  Responses API, which Praxis does not use, so OpenAI models do not show
  reasoning.

**Reasoning Effort** (`llm_orchestrator_effort`) is an optional hint (`low`,
`medium`, `high`, `xhigh`, `max`) sent as Anthropic `output_config.effort` or
OpenAI-compatible `reasoning_effort`. Leave it empty for the provider default.

Models that write literal `<think>...</think>` tags into their output are
still split the same way, so local reasoning models render identically.

## Plan Tracking

The Orchestrator can break complex tasks into steps and show progress via the `report_plan` tool. When the AI calls this tool, you'll see a plan panel with step descriptions and their current status (not started, in progress, done).

## Concurrent Tool Calls

When the model requests several independent tool calls in one response, the
Orchestrator runs them **concurrently** and returns all results together
before the next model turn. Dependent steps still run across turns (call
prerequisites first, wait for results, then continue).

This is useful for fan-out work such as listing nodes and available operations at the same time, or queueing `op_run` against several nodes in one step.

## Sub-orchestrators

For large, independent pieces of work the Orchestrator can call `delegate`
with a self-contained task. A **sub-orchestrator** then runs its own tool loop:

- with a **fresh context** — it sees only the task text, the Orchestrator
  instructions and the playbook list, not the main conversation;
- with the **same tools** except `delegate` (no nesting) and `report_plan`;
- for a **bounded number of model turns** (`max_turns`, default 15, max 30);
  if it runs out it returns a partial result;
- under the same **cancel** (`Ctrl+C`) and [approval gate](#approval-gate) as
  the main session.

Several `delegate` calls in one response run in parallel. Each returns only a
concise summary (capped at 6,000 characters), which keeps the main
conversation small. In the TUI, a sub-orchestrator's tool calls appear as
indented `label › tool` lines under the `delegate` call; its text is not
streamed.

## Playbooks

Playbooks are reusable procedures for the Orchestrator, written in the
[Agent Skills](https://agentskills.io/specification) `SKILL.md` format:

```markdown
---
name: fleet-recon
description: Inventory every node and agent and summarize their tools and configuration. Use when the Operator wants a fleet overview.
license: Apache-2.0          # optional
compatibility: Needs recon data # optional, max 500 chars
metadata:                    # optional string map
  author: red-team
---

# Fleet reconnaissance

1. node_list ...
```

- `name` (required): 1-64 characters, lowercase letters, digits and hyphens,
  no leading/trailing or consecutive hyphens.
- `description` (required): 1-1024 characters; say what the playbook does and
  when to use it — this is what the model sees up front.
- The Markdown body holds the instructions. Optional `allowed-tools` is
  stored but not enforced.

At session start the Orchestrator's instructions list every playbook's name
and description. When a request matches, the model calls `load_playbook` to
fetch the body and follows it. Playbook changes apply to new sessions.

Two built-in playbooks ship with the service: **`fleet-recon`** (read-only
fleet inventory and recon summary) and **`parallel-fanout`** (run one ad-hoc
prompt across many nodes through a temporary operation, then clean up).
Built-ins are refreshed when the service is upgraded unless you have edited
them; deleting a built-in keeps it deleted until the next upgrade.

Manage playbooks from:

- **TUI** — **Settings** (`Ctrl+S`) → **Orchestrator**: `Enter` views/edits a
  playbook in `$EDITOR`, **+ New playbook** starts from a template, `Ctrl+D`
  deletes. Invalid documents are rejected with the validation error and kept
  as a draft for the next edit; changing `name` renames the playbook.
- **CLI** — `praxis playbook list|show|save|delete` (see [CLI](./cli.md)).
- **MCP** — `playbook_list`, `playbook_get`, `playbook_save`,
  `playbook_delete` (see [MCP Server](./mcp.md)).

## Engagement Memory

The Orchestrator keeps persistent notes scoped to an **engagement**. Notes
survive across sessions and service restarts:

- `memory_write` saves a titled note (writing an existing title replaces it),
- `memory_read` reads a note by title,
- `memory_search` finds notes whose title or content contains every query
  word.

At session start the instructions name the engagement and list its 50 most
recently updated note titles, so earlier findings are reused rather than
rediscovered. Sub-orchestrators share the same memory.

In the TUI, `/engagement` shows the current engagement and
`/engagement <name>` switches to another one (starting a fresh session). The
default engagement is `default`; any other engagement is shown in the meta
row under the input. ACP clients select one with `_meta.engagement` on
`session/new`.

## Approval Gate

Tools that change state require the Operator's approval before they run:

- always: `write_file`, `op_create`, `op_delete`, `chain_create`,
  `trigger_create`, `trigger_delete`, `trigger_toggle`, `node_reset`,
  `playbook_save`, `playbook_delete`;
- `session_create` when it enables YOLO mode, and `session_prompt` to a
  session that was created with YOLO mode.

The service sends an ACP `session/request_permission` request; the TUI shows
the tool and its input inline in the transcript. Press `a` to allow once, `l`
to always allow that tool for the rest of the session, or `d` to deny.
Denied calls return an error to the model, which is told not to retry them
unchanged. Several gated calls in one batch are asked one at a time.
`Ctrl+C` cancels a pending approval along with the turn.

Turn the gate off with **Settings** → **Orchestrator** → **Approval Gate**
(`llm_orchestrator_approval_gate`).

## Context Compaction

Instead of dropping old turns, the Orchestrator compacts the conversation when
it grows past the **context budget** (`llm_orchestrator_context_budget`,
default 100,000 estimated tokens; the larger of a local estimate and the
prompt size the provider last reported is used):

- older turns are summarized by the Orchestrator's model into a single
  summary message;
- the most recent whole turns that fit in 40% of the budget are kept
  verbatim;
- cuts only happen at the start of an operator turn, so a tool call is never
  separated from its result;
- kept turns drop their reasoning blocks (providers tie those to the exact
  earlier conversation).

Compaction runs before the model sees a new prompt, including the first
prompt of a resumed session, and appears as a `compact_context` line in the
transcript. If summarization fails, the older turns are dropped with a note
telling the model to ask you to restate anything important.

## Token Usage

Token usage is displayed after each LLM call, showing prompt tokens, completion tokens, and totals. This helps monitor costs when using commercial API providers.

## Session Controls

`Ctrl+C` cancels an in-flight turn — including a model that is still
reasoning, running tools, sub-orchestrators, and pending approvals — while
keeping the conversation. Type `/clear` and press `Enter` to start a fresh
conversation, `/model` to switch models and `/engagement` to show or switch
the engagement. See [Terminal UI](./tui.md#orchestrator-ctrlo) for the
complete Orchestrator controls.

## Settings

Orchestrator behaviour is configured in **Settings** (`Ctrl+S`) →
**Orchestrator** (and the model and max tokens under **LLM**). Changes apply
to new sessions (`/clear`).

| TUI setting | Config key | Default | Description |
|-------------|------------|---------|-------------|
| Approval Gate | `llm_orchestrator_approval_gate` | `true` | Ask before mutating tools run |
| Tool Calling | `llm_orchestrator_tool_mode` | `auto` | `auto`, `native` or `manual` |
| Reasoning | `llm_orchestrator_thinking` | `true` | Request and show model reasoning |
| Reasoning Effort | `llm_orchestrator_effort` | empty | `low`…`max`; empty = provider default |
| Context Budget (tokens) | `llm_orchestrator_context_budget` | `100000` | Compaction threshold (minimum 4000) |
| Orchestrator Max Tokens (LLM tab) | `llm_orchestrator_max_tokens` | `25000` | Output cap per model response |

## Model Recommendations

The Orchestrator needs a model that uses tools reliably over long, multi-step
tasks. Current recommendations (model names from each provider's
documentation, October 2026):

**Anthropic**
- **Claude Opus 5.5** (`claude-opus-5-5`) — recommended. It defaults to
  `medium` effort; set **Reasoning Effort** to `high` for complex multi-node
  work.
- **Claude Sonnet 5.5** (`claude-sonnet-5-5`) — lower cost, good for most
  day-to-day orchestration.
- **Claude Haiku 5.5** (`claude-haiku-5-5`) — fastest and cheapest; best for
  simple, short tasks.

Anthropic models can decline some requests; the Orchestrator then shows "The
model declined to respond (stop reason: refusal)".

**Google Gemini**
- **`gemini-4-argon`** — Google's flagship reasoning model.
- **`gemini-3.8-flash`** — Google's recommended Flash model for agents and
  long-running workflows; cheaper and faster.

**OpenAI**
- **GPT-6 Astra** (`gpt-6-astra`) — OpenAI's flagship; supports function
  calling on Chat Completions, which Praxis uses.
- **GPT-6.1 Sol** and **GPT-6 Luna**: OpenAI documents tool calling for these
  on the Responses API, with Chat Completions supporting them without tool
  calling (Sol) or only at `reasoning_effort` `none` (Luna). With these
  models the Orchestrator falls back to prompt-based tool calling (or set
  **Tool Calling** to `manual`).

**Local and custom endpoints (Ollama, vLLM, LM Studio, ...)** use
prompt-based tool calling by default. Pick the largest instruction-tuned model
you can run; small models often fail to follow the tool-calling format or
hallucinate results. Switch **Tool Calling** to `native` if your server
supports OpenAI-style tool calls for the model.

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

- Ensure you're using a capable model (see recommendations above)
- Check the tool execution results for error messages
- Verify nodes are connected and agents are available
- If the model writes tool calls as text instead of calling them, set
  **Tool Calling** to `manual`; if a local server supports OpenAI tool calls,
  try `native`

### The turn is waiting and nothing happens

A gated tool may be waiting for your approval — look for the **Approve …?**
prompt in the transcript and press `a`, `l` or `d`.

### "The model declined to respond" / "hit the output token limit"

The model ended its turn without any text. For a refusal, rephrase the
request or use another model. For the token limit, raise
`llm_orchestrator_max_tokens` or lower **Reasoning Effort**.

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
