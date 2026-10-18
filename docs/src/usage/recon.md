# Reconnaissance

Reconnaissance discovers what an AI agent can do-its tools, configuration, and history. This is your window into understanding an agent's capabilities before interacting with it.

## Running Recon

From the **Nodes** window (`Ctrl+L`), navigate into the detail pane
with `→`, select an agent with `↑`/`↓`, and press `r` to open the recon
overlay. Static recon runs automatically when no cached result is
available; press `r` to refresh it. Press `Ctrl+U` for deeper semantic
recon, which requires a Semantic Parser LLM.

The overlay is a hierarchical browser (similar to a connectors/MCP
picker): group headers expand and collapse, leaf items open a detail
pane on the right, and a filter bar narrows the tree.

Three tabs:

1. **Config** — config files grouped by type (settings, instructions,
   memory, …) plus a metadata-only **credentials** group
2. **Tools** — MCP servers (with nested tools), skills, hooks, plugins,
   marketplaces, and internal tools
3. **Sessions** — conversation history grouped by project path

See [Terminal UI](./tui.md#nodes-ctrll) for the complete recon
keybindings and mouse controls.

On first open, the TUI checks the service cache. If no recon data is
stored, it triggers an ACP `_praxis/recon` request on the node and polls
about every 1.5 seconds until data arrives (~90-second timeout). Cached
data is displayed instantly on re-open.

### Tree layout

- **Tools:** `MCP Servers` → server → tools; peer sections for `Skills`,
  `Hooks`, `Plugins`, `Marketplaces` and `Internal`. Servers show
  transport, tool count, and a status badge (`[ok]` / `[empty]`). Hook
  rows read `Event [matcher]` with the scope on the right; the detail
  pane shows the handler type, command, scope and source file. Plugin
  rows show their scope (or `disabled`).
- **Config:** groups by `config_type` (path suffixes like
  `project_instructions: /home/...` collapse under a single
  `project_instructions` node); expand a group to pick a file and view
  contents (fetched on select). The trailing `credentials` group lists
  credential stores with a `[present]` / `[keychain]` badge; selecting
  one shows its kind, last-modified time, file mode and non-secret
  metadata — there is no file content behind these rows.
- **Sessions:** groups by project `context_path`; session rows show
  message count and relative time (`2h ago`).

## What Recon Discovers

### Tools

Tools are the capabilities available to the agent. This includes MCP servers (external tool integrations), internal/built-in tools (like file operations, command execution, web browsing), and any extensions or plugins the agent supports. Recon discovers what tools are available, how they're configured, and what parameters they accept.

### Configuration

Config files reveal how the agent is set up. This includes settings files (model preferences, permissions, API configurations), tool/server definitions, and instruction files like CLAUDE.md or similar that influence agent behavior. Recon identifies these files and makes their contents viewable and often editable.

### Hooks

Lifecycle hooks are commands the agent runs on its own events (before a
tool call, on session start, on stop, …). Recon lists each hook's
event, matcher, handler type and command, along with the scope it was
declared in (`user`, `project`, `local`, `managed`, `plugin`,
`extension`) and the file it came from. Hooks are a persistence and
interception surface, so they are worth checking before running
operations against an agent.

### Plugins and Marketplaces

Installed plugins/extensions (name, version, source marketplace, enabled
flag, scope, install path) and the marketplace sources an agent is
configured to install from. Plugins can contribute skills, MCP servers
and hooks; those are surfaced in their own sections.

### Skills

Slash commands plus [Agent Skills](https://agentskills.io) (`SKILL.md`
directories). Recon scans each agent's own skill dirs and the shared
cross-agent locations it reads (for example `~/.agents/skills` and
`<project>/.agents/skills`), parsing the frontmatter `name` and
`description`.

### Memory and Instructions

Persistent memory stores (for example Claude Code auto-memory, Codex
memories, Gemini `save_memory` targets) and instruction files
(`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, rules) appear as Config items, so
their contents are viewable like any other config. Instruction files are
collected globally and for every project known from the agent's session
history, and `@path` imports in them are followed.

### Credentials (metadata only)

For exposure assessment, recon reports which credential stores an agent
uses and whether they exist: the path (or `env:VAR` / `keychain:service`),
the kind (`oauth_token_file`, `api_key_in_config`, `encrypted_store`,
`os_keychain`, `env_var`), last-modified time, Unix file mode, and
non-secret metadata such as account email, plan, auth mode, expiry and
which secret fields are present (`refresh_token_present = true`).

Secret values — tokens, API keys, refresh tokens — are never collected.
Connectors copy only allow-listed fields, and the node redacts anything
that is secret-named or secret-looking before the recon result leaves
the machine. Credential stores are not config items, so
`recon_config_read` cannot be used to read them.

### Sessions

Session history shows past conversations. Recon discovers session files containing conversation transcripts, project contexts, and timestamps. It also identifies project paths where the agent has been used, giving you visibility into recent activity and what the user has been working on.

## Static vs Semantic Recon

### Static Recon

Fast discovery based on file parsing:
- Reads known config file locations
- Parses JSON/YAML configurations
- Lists files and directories
- No LLM required

Best for: Quick overview, checking configuration

### Semantic Recon

In the recon overlay, press `Ctrl+U` to run semantic recon. This performs
deeper analysis using an LLM:
- Parses complex configurations
- Extracts tool definitions from text
- Identifies capabilities from session transcripts
- Creates sessions and communicates directly with the agent
- Understands context

This takes longer than static recon because it actually interacts with the agent to discover its full capabilities.

Best for: Full capability discovery, understanding what tools do

Semantic recon requires the **Semantic Parser** LLM to be configured. Choose a model that balances speed and capability - multiple parsing calls may be made so fast inference helps, but the model also needs to be capable enough to extract meaningful information from complex configurations.

## Querying Stored Recon Data

After running recon, the results are stored in the service database. You can query specific sections without re-running recon:

**MCP tools:**
- `recon_list` - list stored recon data (section: all/sessions/tools/projects/configs/hooks/plugins/credentials/memory/instructions)
- `recon_config_read` - read config file content
- `recon_session_read` - read session file content
- `recon_config_grep` - grep config files with regex
- `recon_session_grep` - grep session files with regex

These are useful for quick lookups and for AI agents that need to browse specific recon data without triggering a full scan.

## Using Recon Data

### View Config Files

Select a config leaf in the TUI tree to view its contents. The detail pane
shows:
- File path
- Full contents
- Syntax highlighting (JSON, YAML)

### Edit Configurations

Some configurations can be edited directly (like Claude's config.json or MCP
server definitions). Select the config leaf and press `Ctrl+E` to open it in
`$VISUAL` or `$EDITOR`; save and exit the editor to write changes to the
target. On Windows, save to the opened file, close the editor, and press
Enter in the terminal to apply the changes. See [Terminal UI](./tui.md#nodes-ctrll) for the complete recon
controls.

This is useful for exploring the offensive impact of configuration changes - adding MCP servers, modifying permissions, changing model settings, or injecting tool configurations.

**Caution**: Editing configs can break the agent if done incorrectly. The changes persist until the user or agent modifies them again.

### View Session History

Select a session leaf to view the conversation:
- Full transcript with prompts and responses
- Tool calls and results
- Timestamps

This reveals:
- What projects the user worked on
- What questions they asked
- What files were accessed
- Sensitive information mentioned

## Tool Discovery Details

### MCP Servers

MCP (Model Context Protocol) servers extend agent capabilities. Recon discovers server definitions including stdio commands and arguments, SSE endpoints, and environment variables. It also attempts to connect to each MCP server to pull out the actual tools it provides - giving you visibility into what external capabilities the agent has access to and potential attack surface.

Note that if an MCP server requires specific authentication or environment setup, the tool discovery connection may fail. Praxis does its best to replicate the agent's environment but some servers may not respond.

### Internal Tools

Semantic recon discovers built-in agent tools by creating a session and asking the agent directly about its capabilities. The response is then passed through the semantic parser to extract structured tool definitions.

This approach has some pitfalls: the agent may refuse to disclose its tools, provide incomplete information, or the parser may fail to extract tools from the response. The prompt used to ask the agent is defined in the agent connector code and can be customized if needed for better results with specific agents.

Understanding available tools helps you craft effective prompts for operations.

## Best Practices

### Start with Static

Run static recon first-it's fast and gives you the lay of the land. Then run semantic recon for deeper understanding.

### Check Session History

Session history often contains valuable information:
- API keys mentioned in prompts
- File paths discussed
- Security-relevant conversations

### Note Interesting Tools

Pay attention to powerful tools:
- Database access
- File system access
- Network capabilities
- Code execution

These are your leverage points for operations.

### Compare Before/After

After modifying configs, run recon again to verify changes took effect.

## Troubleshooting

### No recon data

- Ensure agent is fingerprinted
- Check that config files exist
- Verify node has read permissions

### Semantic recon fails

- Check Semantic Parser LLM is configured
- Verify API key is valid
- Look for errors in service logs

### Missing MCP servers

- Some agents don't use MCP
- Try semantic recon for deeper discovery
