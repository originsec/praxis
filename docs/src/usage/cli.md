# CLI

The Praxis CLI (`praxis_cli`) provides both an interactive terminal UI and a non-interactive command-line interface for controlling the Praxis C2 network.

## Purpose

The CLI is the **first-party** and only first-class supported client for
Praxis. It provides:
- Full-featured interactive terminal UI for hands-on control
- Non-interactive commands for scripting and automation
- Works equally well over SSH and in headless environments

## Installation

The CLI is installed automatically with the native installation scripts:

```bash
# Linux/macOS
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

The binary is installed to `~/.praxis/bin/praxis_cli`.

When using Docker, the CLI binary is built into the container image and copied to the data volume on startup. You can extract it with:

```bash
docker cp $(docker compose ps -q praxis):/app/praxis_cli ./praxis_cli
```

> **Note:** The container name depends on your project directory. Run this from the directory containing your `docker-compose.yml`.

## Interactive Terminal UI (Default Mode)

Running `praxis_cli` with no arguments launches the interactive terminal UI:

```
$ praxis_cli
```

The terminal UI provides five main windows, switched with keyboard shortcuts:

### Orchestrator (`Ctrl+O`)

LLM-powered conversation interface for coordinating operations across the Praxis network. Features:
- Real-time streaming responses with tool execution display
- Plan tracking with step visualization
- Token usage statistics
- Command history and conversation scrolling
- Single orchestrator session per TUI run — the conversation lifetime equals the TUI process lifetime. Use `praxis --continue` or `praxis --resume` on the next launch to bring it back. `Ctrl+Alt+W` exports the transcript to markdown.
- `Ctrl+C` cancels the in-flight prompt
- `Ctrl+E` toggles the tools panel; `Ctrl+Alt+E` expands it fully

### Nodes (`Ctrl+L`)

Node and agent management with integrated session chat and terminal access:
- Node list with status indicators (active/warning/inactive), OS details, and agent counts
- Agent selection and concurrent ACP session management
- **Session Chat** — direct conversation with agents, with YOLO mode and working directory selection
- **Active Sessions** overlay (`Ctrl+W`) — see every live session across nodes and connectors; Enter to resume, `d` / `Del` to discard, Esc to dismiss
- **Terminal** (`Ctrl+Y` to toggle) — full PTY terminal emulation with scrollback
- **Recon** (`r` with an agent selected in the detail pane) — view reconnaissance results directly in the terminal

Inside a chat view, `Esc` or `Ctrl+W` **pauses** the session (leaves it
running on the node; resume from the Active Sessions overlay). `Ctrl+C`
cancels an in-flight prompt, or closes the session if the agent is idle.
The status bar shows `N sessions` whenever any concurrent sessions are
live. On first connect, whenever you open the Nodes window, and after a
node reset, the TUI calls `session/list` on each node to pick up
sessions left alive from previous runs or other clients.

#### Recon Overlay

The recon overlay opens as a full-screen modal from the Nodes detail
pane. It shows config files, tools, and sessions in a tabbed terminal
interface.

| Key | Action |
|-----|--------|
| `Tab` / `1` `2` `3` | Switch tab (Config / Tools / Sessions) |
| `↑` / `↓` | Navigate left pane list |
| `PgUp` / `PgDn` | Scroll right pane content |
| `r` | Trigger static recon refresh |
| `d` | Trigger semantic recon (Discover) |
| `Ctrl+E` | Edit selected Config file in `$EDITOR` (Config tab only) |
| `Esc` / `q` | Close overlay |

When opened, the TUI first checks the service cache for existing recon
data. If none is cached, it sends an ACP `_praxis/recon` request to the
node and polls `request_recon` every second for up to 60 seconds. Cached
recon data appears instantly on re-open.

The **Config** tab shows discovered configuration files in the left pane
and the selected file's contents in the right pane. Pre-fetched contents
are shown inline; files discovered by static recon but not yet fetched
display a placeholder. Press `Ctrl+E` to open the selected file in
`$VISUAL`/`$EDITOR`; on a clean exit with changes, the new contents are
written back to the node and the right pane refreshes (a transient
"Saved" / "No changes" / error status shows in the recon header).

The **Tools** tab has three categories: MCP Servers, Skills, and Internal
tools. The left pane shows the category list; the right pane shows
server details and tool lists for MCP, or flat tool lists for Skills and
Internal.

The **Sessions** tab shows discovered session files on the left and parsed
conversation transcripts on the right. Session content is parsed as
JSONL, JSON array, or raw text depending on the agent's format.

### Intercept (`Ctrl+T`)

Live traffic interception with three tabs (`Tab` / `Shift+Tab` to switch):

- **Log** — incoming traffic streams from every node into a ring buffer.
  HTTP entries show individually; WebSocket and HTTP/2 frames group by
  `(node, url)` so streaming endpoints don't flood the list.
- **Rules** — create, edit, delete, and toggle intercept rules (regex
  patterns with direction and scope). Rules can carry an optional LLM
  summarisation prompt.
- **Matches** — matched-traffic review with AI summaries (when a rule
  has a summarisation prompt).

#### Log tab

| Key | Action |
|-----|--------|
| `Enter` | Focus detail pane (then `↑`/`↓` scrolls detail) |
| `Esc` | Unfocus detail / clear search |
| `/` | Focus search box (regex, falls back to substring) |
| `f` | Cycle protocol filter: all → http → ws → h2 |
| `n` | Cycle node filter (no popup; `Esc` clears) |
| `a` | Cycle agent filter |
| `p` | Pause / resume the live stream |
| `r` | Re-request the initial page from the service |
| `c` | Clear ALL traffic (with confirmation) |
| `H` | Cycle body render mode: pretty → raw → hex |
| `i` | Toggle interception on the selected entry's node |

Request and response bodies arrive via a second fetch on selection to
keep the broadcast payload small — large bodies load within a few
hundred milliseconds after you navigate to an entry.

#### Rules tab

| Key | Action |
|-----|--------|
| `n` | Create a new rule |
| `e` | Edit the selected rule |
| `d` | Delete the selected rule (with confirmation) |
| `Space` | Toggle enabled / disabled |
| `Enter` | Jump to the Matches tab filtered to this rule |
| `r` | Refresh the rules list |

The rule form (open via `n` or `e`) fields: Name, Regex, Direction
(`send` / `receive` / `both`), Scope (`all` / `node` / `agent`), and an
optional LLM summary prompt. `Tab` moves between fields, `Space` /
`←` / `→` cycles select-style fields, `Ctrl+S` saves, `Esc` cancels.

#### Matches tab

| Key | Action |
|-----|--------|
| `Enter` | Focus match detail pane |
| `f` | Cycle rule filter |
| `Esc` | Clear rule filter / unfocus detail |
| `r` | Refresh |

### Log Query (`Ctrl+G`)

KQL-style query interface over captured logs (intercepted traffic, event
logs, recon results, operations history, and more — 12 virtual tables in
total). See [Log Query](./log-query.md) for the full query reference.

- Multi-line editor with basic KQL keyword highlighting
- `Ctrl+Enter` runs the query; the spinner in the hint line indicates
  in-flight execution
- `Tab` opens a context-aware autocomplete popup (tables at start of
  query, operators after `|`, columns inside `where` / `project` /
  `sort`, functions & keywords inline). `↑`/`↓` navigate, `Enter`
  accepts, `Esc` dismisses
- `?` toggles a schema sidebar listing every available table with its
  columns and descriptions
- `Esc` from the editor moves focus to the results; `i` from the results
  moves focus back to the editor

Results pane:

| Key | Action |
|-----|--------|
| `↑` `↓` `PgUp` `PgDn` `g` `G` | Row navigation |
| `Enter` | Expand the selected row into a key/value detail pane (JSON fields pretty-printed) |
| `/` | Open a row-search filter (substring match across all cells) |
| `s` | Cycle the sort column |
| `S` | Toggle sort direction |
| `r` | Re-run the last query |
| `Esc` | Close expanded row / clear search / return to editor |

Response bodies in `TrafficLogs` and JSON columns like
`ToolkitActionsLog.details_json` auto-pretty-print in the detail pane.

### Operations (`Ctrl+P`)

Operation and chain management with three tabs (`Tab` / `Shift+Tab` to switch):

- **Executions** — live tracking of running/queued/completed operations and chains with duration timers
- **Library** — browse operation and chain definitions with search filtering and detail view
- **Triggers** — automated chain firing rules

Common actions:
- Create new operations inline (`Ctrl+N` on the Library tab)
- Create new chains via the chain builder (`Ctrl+Alt+N` on the Library tab, or click `^!n new chain` in the hint bar)
- Edit an existing op or chain (`Ctrl+E` with the row selected — opens the op form for ops, the chain builder for chains)
- Run operations and chains with node/agent selection and YOLO mode (`Ctrl+R`)
- Delete the selected op or chain (`Ctrl+D`)
- Create, edit, enable/disable and delete chain triggers

#### Library tab — chain builder

The chain builder is a full-screen TUI canvas with draggable element blocks and
orthogonal line connectors between ports. It is mouse-first but fully usable
from the keyboard for core authoring actions.

- **Canvas** — drag a block by its body to move it (drag starts only after a small movement threshold so a plain click does not nudge the block); drag empty space to pan; the mouse wheel scrolls vertically. Block positions persist in `ChainDefinitionInput.positions` so each chain remembers its layout.
- **Ports** — every block exposes filled circles `●` on its left (input) and right (output) edges. Ports and connector segments hit-test with a multi-cell tolerance. Click an output port and drag to an input port on another block to create a connection. A rubber-band line follows the cursor while you drag. Loop outputs are labeled `r` (retry / port 0) and `x` (exit / port 1).
- **Selection** — single-click a block or connector segment to select it. `Enter` or double-click opens the **properties modal** for that selection.
- **Properties modal** — for blocks: click fields to edit inline; prompts and tool params accept multi-line input (`Shift+Enter` inserts a newline); pickers for operation / model / tool / payload / session group; kind cycler `◂ Kind ▸` only among body kinds (Trigger and Termination stay fixed); memory mode store/retrieve; session group + per-block config (max runtime, YOLO, working dir, require-all-inputs). For connections: condition cycler toggles `any` / `on success` / `on failure` (also shown as ✓/✗ glyphs on the edge); `c` cycles condition when a connection is selected.
- **Header strip** — `Name`, `Category`, `Timeout`, and `Description` at the top; click to edit. Incomplete blocks show a `!` badge on the canvas; save rejects invalid graphs with a clear error list.
- **Palette** — `[+ OP]`, `[+ TXR]`, … buttons along the bottom. New blocks place to the right of the current selection (when one exists) and auto-wire into the graph. New chains pre-select the Trigger so the first add wires in automatically. Adding an Operation opens the op picker immediately. Keyboard (canvas only, not while the properties modal is open): `o` op, `t` transform, `g` generic prompt, `m` memory, `p` loop, `k` tool, `y` payload.
- **Keyboard selection** — `Tab` / `Shift+Tab` cycle selection through blocks then connections (no mouse required). `Enter` opens properties for the selection. `c` selects the first connection if needed and cycles its condition (`any` / on success / on failure).
- **Layout** — `Layout` button or `l` re-runs left-to-right auto-layout from triggers.
- **Save / Cancel** — top-right buttons; `Ctrl+S` saves; `Esc` closes the properties modal first, then cancels the form (confirms if there are unsaved changes). A dirty `*` marker appears in the hint bar while editing.

A newly created chain is seeded with a connected `Trigger → Termination` pair so the graph is valid out of the box; auto-layout is applied to existing chains that don't yet have stored positions.

#### Triggers tab

Triggers fire a chain on a schedule, when an intercept rule matches, or when a new node connects. Each trigger picks a target chain, a trigger type, and a target spec (nodes + agents, with an optional OS substring filter and, for event triggers, an "include triggering node" toggle).

| Key | Action |
|-----|--------|
| `Enter` | Toggle enabled/disabled for the selected trigger |
| `Ctrl+N` | New trigger |
| `Ctrl+E` | Edit selected trigger |
| `Ctrl+D` | Delete selected trigger |

In the trigger form, `↑/↓` or `Tab`/`Shift+Tab` move between fields, `←/→` cycle picker options, `Space`/`Enter` toggle checkboxes and list items, `Ctrl+S` saves, and `Esc` cancels. The form is fully mouse-driven: click a row to focus/toggle it, click `Ctrl+S`/`Esc` in the hint bar to save or cancel.

### Settings (`Ctrl+S`)

Configuration management:
- **LLM** — model definitions, provider selection, API keys, and feature assignment (orchestrator, semantic ops, semantic parser, traffic parser)
- **Service** — MCP server toggle, MCP port, Claude Bridge settings (CCRv1/CCRv2 enable and port configuration), logging, log query row limits, prompt timeout
- **About** — connection info

### Mouse Support

The TUI supports mouse interactions across all windows:

- **Click** — select items in lists, tabs, and interactive elements
- **Double-click** — activate items (e.g. open an operation, select a node)
- **Drag** — scroll through lists and content areas
- **Scroll wheel** — scroll through lists, chat history, and scrollable content

Mouse interactions work alongside keyboard controls in all windows and popups.

### Global Keybindings

| Key | Action |
|-----|--------|
| `Ctrl+O` | Orchestrator window |
| `Ctrl+L` | Nodes window |
| `Ctrl+T` | Intercept window |
| `Ctrl+P` | Operations window |
| `Ctrl+S` | Settings window |
| `Ctrl+Y` | Toggle terminal mode (Nodes) |
| `Ctrl+Q` | Quit |

`Ctrl+W` is window-scoped: in Nodes it toggles the Active Sessions
overlay (or pauses the current chat session), in Orchestrator it closes
the active orchestrator session.

## Non-Interactive Mode

### One-Shot Commands

Use `-C` to run a single command and exit:

```bash
praxis_cli -C "node list"
praxis_cli -C "session create --node abc123 --agent codex --yolo"
```

### Direct Subcommands

Subcommands can also be passed directly:

```bash
praxis_cli node list
praxis_cli session create --node abc123 --agent codex --yolo
```

### Available Commands

**Node Management:**
```bash
node list                          # List all connected nodes
node select <prefix>               # Select node by ID prefix
node reset <prefix>                # Reset a node
```

**Agent Management:**
```bash
agent list --node <prefix>                   # List agents on a node
agent update --node <prefix>                 # Request agent info update
agent config read --node <prefix> --agent <name> <path>     # Read config file
agent config write --node <prefix> <path> <contents>        # Write config file (agent-independent)
agent config grep --node <prefix> --agent <name> <path> <pattern>  # Grep config file
agent session read --node <prefix> --agent <name> <file>    # Read session file
agent session grep --node <prefix> --agent <name> <file> <pattern> # Grep session file
```

**Session Management:**
```bash
session create --node <prefix> --agent <name> [--yolo] [--project <path>] [--timeout <secs>]
session prompt --node <prefix> <text>
session close --node <prefix>
```

Every command that needs an agent takes `--agent` explicitly; ACP
sessions are per-agent, so the same node can host concurrent sessions
under different agents.

Non-interactive mode persists a single session id per node in
`~/.praxis/cli.json` — `session create` stores it, `session prompt` and
`session close` read it. The interactive TUI runs concurrent in-memory
sessions and does not share state with the non-interactive subcommands.

## Global Options

| Option | Description | Default |
|--------|-------------|---------|
| `-r, --rabbitmq` | RabbitMQ URL | `amqp://praxis:praxis@localhost:5672` |
| `-t, --timeout` | Connection/command timeout in seconds | `600` |
| `-C, --command` | Run a single command and exit | - |
| `--acp` | Run as an ACP bridge (stdin/stdout proxy) | - |
| `--clear` | Clear local state and exit | - |
| `--status` | Check service connection status | - |
| `--continue` | Resume the most recent saved orchestrator session | - |
| `--resume` | List saved orchestrator sessions and pick one to resume | - |

The RabbitMQ URL can also be set via the `PRAXIS_RABBITMQ_URL` environment variable.

## ACP Bridge Mode

The CLI can act as an [Agent Client Protocol](https://agentclientprotocol.com/) bridge, exposing the Praxis service as a standard ACP agent over stdin/stdout. This allows any ACP-compatible client to interact with Praxis.

```bash
praxis_cli --acp
```

In this mode the CLI:
- Reads NDJSON JSON-RPC requests from **stdin**
- Forwards them to the Praxis service via RabbitMQ
- Writes JSON-RPC responses and notifications to **stdout** as NDJSON
- Only forwards responses to requests it originated (filters out other clients' traffic)

This means any ACP client can use Praxis as its agent. For example, using [acpx](https://www.npmjs.com/package/acpx):

```bash
acpx --agent 'praxis_cli --acp' 'list agents'
```

The bridge connects with an `acp_` prefixed client ID, so sessions created through it get `ACP_` prefixed session IDs.

## Local State

The CLI stores persistent state in `~/.praxis/cli.json`. This file contains:

- **client_id**: A unique identifier for this CLI instance, used for RabbitMQ queue routing

The client ID is generated on first run and reused for subsequent executions.

To reset local state:
```bash
praxis_cli --clear
```

