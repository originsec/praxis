# Agent Exposure

Agent exposure analysis turns the recon data Praxis already stores into an
attack-path view across every node: which agents can read private data,
which are exposed to content an attacker controls, which can send data out,
and how they are linked to each other through shared MCP servers and hosts.
Think of it as BloodHound for coding agents.

## The lethal trifecta

The analysis is built on Simon Willison's
["lethal trifecta"](https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/)
(June 2025). An agent that combines all three of the following can be
steered by injected instructions into stealing data:

| Class | Badge | Meaning | Examples |
|-------|-------|---------|----------|
| `private_data` | **P** | Access to private data | file reads, mail, chat history, databases, secrets |
| `untrusted_content` | **U** | Exposure to content an outside party can control | web fetch/search, issues, tickets, email, repo files, browser automation |
| `exfiltration` | **X** | Ability to communicate externally | network requests, messaging, posting comments, `git push`, shell with network access, remote MCP servers |

Every capability an agent has — MCP tools (or the MCP server itself when its
tools could not be listed), skills, and built-in tools — is classified into
zero or more of these classes. An agent holding all three is a **trifecta**
agent.

Coding agents with a shell are trifecta agents almost by definition: a shell
reads files, can fetch untrusted content and can reach the network. What
differs between them is whether a human is still approving each step, which
is what the risk levels capture.

## Risk levels

| Risk | Condition |
|------|-----------|
| `critical` | Trifecta **and** the human is out of the loop: permission prompts are bypassed (YOLO), or a contributing capability is auto-approved |
| `high` | Trifecta |
| `medium` | Two of the three classes |
| `low` | One class or none |

Auto-approval counts towards `critical` when an allow-list entry covers a
whole tool (`Bash`, `Bash(*)`, `mcp__github__create_issue`, a trusted Gemini
MCP server) or a scoped shell entry whose command can reach the network or
run arbitrary code on its own (`curl`, `wget`, `ssh`, `git push`, `gh`,
`python`, `node`, `bash`, …). Narrow entries such as `Bash(ls:*)` or
`WebFetch(domain:example.com)` are still listed as modifiers but do not
raise the risk.

## Running it

Exposure is computed on demand from stored recon, so run recon on the
agents you care about first (see [Reconnaissance](./recon.md)). Agents
without stored recon are reported in a warning and left out.

- **TUI** — press `x` in the **Nodes** window (`Ctrl+L`). See
  [TUI](./tui.md#exposure-overlay).
- **CLI** — `praxis exposure summary|agent|paths|export|rules`. See
  [CLI](./cli.md#available-commands).
- **MCP** — `exposure_summary`, `exposure_agent`, `exposure_paths`,
  `exposure_export`. See [MCP Server](./mcp.md#exposure-analysis).
- **Log Query** — the `ExposureLogs` table. See
  [Log Query](./log-query.md#exposurelogs).

Two optional passes refine the report. Both cache their results, so later
reports (and `ExposureLogs`) reuse them without repeating the work:

- **Probe configs** (TUI `p`, CLI `--probe`, MCP `probe_configs`) — stored
  recon does not keep config file contents, so detecting modifiers means
  reading the agents' settings files on the node with the same file-read
  path the recon overlay uses. Only online nodes are probed; each read has
  a 10-second timeout and the pass is abandoned after 45 seconds. Note that
  this touches files on the endpoint.
- **LLM classification** (TUI `l`, CLI `--llm`, MCP `llm`) — capabilities no
  rule matched are sent, by name and description only, to the
  [Semantic Parser](../development/semantic-parser.md) LLM. Up to 120 are
  classified per run, in batches, and the pass stops after 150 seconds;
  run again to continue. Verdicts are cached per capability and shown with
  the reason `llm`. Never required.

## What the report contains

- **Agents** — per agent: risk, trifecta flag, every capability with its
  classes and the reason for each (`rule:<id>` or `llm`), modifiers
  (`yolo` / `auto_approve` with the setting and the file it came from),
  unclassified capabilities, and hosts seen in intercepted traffic.
- **Paths** — one path per trifecta agent: untrusted-input sources → agent →
  private-data capabilities → exfiltration channels, plus any modifiers that
  leave the path unattended.
- **Shared** — MCP servers and hosts reachable from two or more agents,
  flagged when they span nodes (`cross-node`) or reach a trifecta agent.
  These are the links that tie otherwise separate agents together.
- **Graph** — the underlying entities and edges, exportable as Graphviz DOT
  or JSON.

### Graph model

| Entity | Id | Notes |
|--------|----|-------|
| node | `node:<node_id>` | Endpoint running a Praxis node |
| agent | `agent:<node_id>/<short_name>` | Filled by risk in DOT exports |
| mcp_server | `mcp:<key>` | Deduplicated across agents and nodes (see below) |
| host | `host:<hostname>` | Remote MCP endpoints and intercepted-traffic hosts |
| skill | `skill:<node_id>/<agent>/<name>` | Only skills with at least one class |

| Edge | From → To |
|------|-----------|
| `runs` | node → agent |
| `uses` | agent → mcp_server |
| `has_skill` | agent → skill |
| `connects_to` | mcp_server → host |
| `observed` | agent → host (intercepted traffic) |

Entity and edge kinds are open-ended, and entities carry free-form `attrs`,
so later recon (credentials, identities) can add node types without
changing existing ones.

The same MCP server is recognised across agents and nodes by a normalised
key: for remote servers the URL host, port and path (scheme, credentials,
query string and fragment dropped); for stdio servers the program basename
plus arguments, with launcher flags such as `npx -y` dropped, package
version pins (`pkg@1.2.3`, `@latest`) removed and obvious secret values
(`--api-key …`, `TOKEN=…`) redacted. Loopback endpoints (`localhost`,
`127.*`, `::1`) are local to each node and are never merged across nodes.

## Exposure rules

Classification is driven by a TOML rule file stored by the service, like
the intercept targets file. Edit it from the TUI overlay (`e`, which opens
`$VISUAL` / `$EDITOR`) or with `praxis exposure rules show|set|reset`. A
file that fails validation is rejected and the stored rules are left
unchanged (the TUI keeps your draft and reopens it on the next `e`). Until
you save an edit, the shipped defaults are used, so default improvements
arrive with upgrades.

The file has three parts:

```toml
# Built-in tools assumed when recon did not enumerate internal tools
# (static recon). They appear with kind "builtin".
[builtins]
claudecode = ["Read", "Write", "Edit", "Glob", "Grep", "Bash", "WebFetch", "WebSearch"]

# Classification rules, evaluated in file order. Every matching rule adds
# its classes; `stop = true` prevents later rules from matching.
[[rule]]
id = "web-fetch"
classes = ["untrusted_content", "exfiltration"]
kinds = ["builtin", "internal", "mcp_tool"]   # optional
agents = ["claudecode"]                       # optional
name = '^(webfetch|web_fetch|fetch)$'         # regex, case-insensitive
# description / server / command / url matchers are also available

# An empty class list records an explicit benign verdict.
[[rule]]
id = "benign-planning"
classes = []
name = '^(task|todowrite|update_plan)$'
stop = true

# Agent-level modifiers read from settings files when configs are probed.
[[modifier]]
id = "claude-bypass-permissions"
kind = "yolo"                                  # or auto_approve
agents = ["claudecode"]
config_types = ["global_settings", "project_settings", "project_settings_local"]
key = "permissions.defaultMode"                # dotted path, `*` = any key
equals = ["bypassPermissions"]                 # optional
```

Rule notes:

- At least one of `name`, `description`, `server`, `command` or `url` is
  required; every matcher given must match.
- `server`, `command` and `url` come from the MCP server definition, so a
  tool-level rule can target "every tool on a browser-automation server".
- An MCP server only appears as a capability of its own (kind `mcp_server`)
  when recon could not list its tools; the shipped `server-*` rules cover
  well-known servers for that case.
- Capabilities no rule matches are reported as unclassified and are the
  input to LLM classification.

Modifier notes:

- `config_types` are recon config types (the part before `:` for
  project-scoped types such as `project_settings:/path/to/project`).
- Without `equals`, a `true` boolean, a non-empty string, or each string in
  an array matches; arrays produce one modifier per entry, so allow-list
  entries can be matched to individual capabilities.
- The shipped modifiers cover Claude Code (`permissions.defaultMode`,
  `permissions.allow`, `enableAllProjectMcpServers`), Codex
  (`approval_policy = "never"`, `sandbox_mode = "danger-full-access"`),
  Gemini CLI (`tools.autoAccept`, `tools.allowed`, trusted MCP servers),
  Cursor (`permissions.allow`) and Droid (`commandAllowlist`).

## Exports

```bash
praxis exposure export --format dot -o exposure.dot
dot -Tsvg exposure.dot -o exposure.svg
praxis exposure export --format json -o exposure.json
```

The TUI's `x` writes both formats to `praxis-exposure-<timestamp>.dot` and
`.json` in the CLI's working directory. DOT output is deterministic
(entities and edges sorted), so successive exports diff cleanly.

## Limitations

- The analysis is only as complete as the stored recon. Static recon does
  not enumerate built-in tools, so the `[builtins]` list stands in for
  them; run semantic recon for the agent's own view.
- Classification is heuristic. Rules err towards flagging, and the reason
  on every class makes each call reviewable; refine the rule file for your
  environment.
- Intercepted-traffic hosts are only those covered by intercept targets,
  which are mostly LLM provider APIs.
