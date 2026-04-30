# Reconnaissance

Reconnaissance discovers what an AI agent can do-its tools, configuration, and history. This is your window into understanding an agent's capabilities before interacting with it.

## Running Recon

With an agent selected:

1. Click **Recon** in the agent panel
2. Static recon runs immediately
3. Results appear organized by category

For deeper discovery, click **Semantic Recon** (requires Semantic Parser LLM configured).

## TUI

The CLI (`praxis_cli`) provides the same reconnaissance capabilities in
the terminal. From the **Nodes** window (`Ctrl+L`), navigate into the
detail pane (`→`), select an agent (`↑`/`↓`), and press **`r`** to open
the recon overlay.

The overlay shows three tabs:

1. **Config** — discovered configuration files and their contents
2. **Tools** — MCP servers, skills, and internal tools
3. **Sessions** — conversation history with parsed transcripts

| Key | Action |
|-----|--------|
| `Tab` / `1` `2` `3` | Switch tab |
| `↑` / `↓` | Navigate left pane |
| `PgUp` / `PgDn` | Scroll content |
| `r` | Refresh (static recon) |
| `d` | Discover (semantic recon) |
| `Esc` / `q` | Close overlay |

On first open, the TUI checks the service cache. If no recon data is
stored, it triggers an ACP `_praxis/recon` request on the node and polls
every second until data arrives (60-second timeout). Cached data is
displayed instantly on re-open.

## What Recon Discovers

### Tools

Tools are the capabilities available to the agent. This includes MCP servers (external tool integrations), internal/built-in tools (like file operations, command execution, web browsing), and any extensions or plugins the agent supports. Recon discovers what tools are available, how they're configured, and what parameters they accept.

### Configuration

Config files reveal how the agent is set up. This includes settings files (model preferences, permissions, API configurations), tool/server definitions, and instruction files like CLAUDE.md or similar that influence agent behavior. Recon identifies these files and makes their contents viewable and often editable.

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

Click the **Discover** button to run semantic recon. This performs deeper analysis using an LLM:
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
- `recon_list` - list stored recon data (section: all/sessions/tools/projects/configs)
- `recon_config_read` - read config file content
- `recon_session_read` - read session file content
- `recon_config_grep` - grep config files with regex
- `recon_session_grep` - grep session files with regex

These are useful for quick lookups and for AI agents that need to browse specific recon data without triggering a full scan.

## Using Recon Data

### View Config Files

Click any config file to see its contents. The viewer shows:
- File path
- Full contents
- Syntax highlighting (JSON, YAML)

### Edit Configurations

Some configurations can be edited directly (like Claude's config.json or MCP server definitions):

1. Click on a config file
2. Make changes in the editor
3. Click **Save**
4. Changes are written to disk on the target

This is useful for exploring the offensive impact of configuration changes - adding MCP servers, modifying permissions, changing model settings, or injecting tool configurations.

**Caution**: Editing configs can break the agent if done incorrectly. The changes persist until the user or agent modifies them again.

### View Session History

Click on a session to see the conversation:
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
