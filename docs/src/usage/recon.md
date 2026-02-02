# Reconnaissance

Reconnaissance discovers what an AI agent can do-its tools, configuration, and history. This is your window into understanding an agent's capabilities before interacting with it.

## Running Recon

With an agent selected:

1. Click **Recon** in the agent panel
2. Static recon runs immediately
3. Results appear organized by category

For deeper discovery, click **Semantic Recon** (requires Semantic Parser LLM configured).

## What Recon Discovers

### Tools

Tools are the capabilities available to the agent:

**MCP Servers** (Claude Code, Gemini)
- Server names and descriptions
- Commands used to start them
- Environment variables
- Enabled/disabled status

**Internal Tools**
- File operations (read, write, edit)
- Command execution
- Web browsing
- Code analysis

**Extensions/Plugins**
- Agent-specific integrations
- Third-party tools

### Configuration

Config files reveal how the agent is set up:

**Main Config**
- Model preferences
- Permission settings
- API configurations
- Behavior options

**MCP Config**
- Server definitions
- Connection details
- Tool configurations

### Sessions

Session history shows past conversations:

**Session Files**
- Conversation transcripts
- Project contexts
- Timestamps

**Project Paths**
- Directories where the agent has been used
- Recent project history

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

Semantic recon requires the **Semantic Parser** LLM to be configured. Fast models like Claude Haiku or GPT-4o-mini work well since multiple parsing calls may be made.

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

MCP (Model Context Protocol) servers extend agent capabilities. Recon shows:

```
MCP Server: filesystem
  Command: npx -y @anthropic/mcp-server-filesystem
  Args: /home/user/documents
  Status: enabled
```

This tells you:
- What external tools the agent can use
- What data sources it has access to
- Potential attack surface

### Internal Tools

Semantic recon extracts built-in capabilities:

```
Tool: Bash
  Description: Execute shell commands
  Parameters: command (string)

Tool: Read
  Description: Read file contents
  Parameters: path (string)
```

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
- Check if mcp.json exists
- Try semantic recon for deeper discovery
