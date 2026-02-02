# Claude Code Connector

The Claude Code connector enables interaction with Anthropic's Claude Code CLI agent.

## Overview

Claude Code is a command-line AI assistant that can read files, execute commands, and work with code. The connector supports all major platforms (Linux, Windows, macOS).

## Fingerprinting

The connector looks for Claude Code by checking:

1. **Config file existence** - `~/.claude.json` or `~/.config/claude/config.json`
2. **Process search** - Looking for running `claude` processes
3. **Binary location** - Finding the `claude` executable in PATH

If any of these succeed, fingerprinting returns true and the agent appears in the node's agent list.

## Interception

Traffic is intercepted for the domain:
- `api.anthropic.com`

With URL pattern filter:
- `messages` - Only capture requests to the messages endpoint (filters out telemetry)

When interception is enabled, you'll see:
- Prompts sent to the Claude API
- Responses including assistant messages and tool calls
- Token usage and other metadata

## Reconnaissance

### Static Recon

Static reconnaissance discovers:

**Configuration**
- Main config file (`~/.claude.json` or `~/.config/claude/config.json`)
- Permission settings, model preferences, etc.

**MCP Servers**
- From `~/.claude/mcp.json`
- Server names, commands, environment variables
- Enabled state

**Sessions**
- Project directories under `~/.claude/projects/`
- Session files with conversation history
- Recent project paths

### Semantic Recon

When semantic recon is enabled (requires Semantic Parser LLM), the connector also:
- Parses configuration to extract tool definitions
- Identifies internal Claude tools from session transcripts
- Extracts capability information

## Session Management

Sessions are created by spawning Claude Code in a PTY (pseudo-terminal):

```
┌─────────────────────────────────────────┐
│              Praxis Node                 │
│                                          │
│  ┌─────────────────────────────────┐    │
│  │         PTY Session              │    │
│  │                                  │    │
│  │  claude --yes-always ───────────┼────┼──▶ Claude Process
│  │         │                        │    │
│  │         └─ stdin/stdout ─────────│    │
│  └─────────────────────────────────┘    │
└─────────────────────────────────────────┘
```

### Session Context

When creating a session, you can specify:

**Working Directory** - Where Claude should operate. This affects what files it can see with `ls`, `cat`, etc.

**YOLO Mode** - When enabled, passes `--yes-always` to Claude, which auto-approves all tool calls. Without this, Claude asks for confirmation before running commands.

### Transacting

Sending prompts works by:
1. Writing the prompt text to the PTY stdin
2. Waiting for Claude to process and respond
3. Parsing the response from stdout
4. Returning the assistant's message

The connector handles the terminal control sequences and output parsing.

## Config Editing

You can view and edit Claude's configuration files directly from the Praxis UI:

- **Main config** - Model selection, permissions, API settings
- **MCP servers** - Add, remove, or modify MCP server definitions

Changes are written back to disk and take effect on the next Claude session.

## Tool Discovery

The connector discovers several categories of tools:

**MCP Servers** - External tools connected via the Model Context Protocol:
- File system access
- Database connections
- Custom tools

**Internal Tools** - Claude's built-in capabilities discovered through semantic parsing:
- File operations (read, write, edit)
- Command execution (bash)
- Web browsing
- Code analysis

## Files and Paths

| File | Path | Content |
|------|------|---------|
| Main config | `~/.claude.json` | Settings, permissions, API config |
| Alt config | `~/.config/claude/config.json` | Same (alternate location) |
| MCP servers | `~/.claude/mcp.json` | MCP server definitions |
| Projects | `~/.claude/projects/` | Session history by project |

## Troubleshooting

### "Agent not fingerprinted"

- Ensure Claude Code is installed and configured
- Check that config file exists
- Verify the `claude` command is in PATH

### "Session creation failed"

- Check that Claude Code can run normally from terminal
- Verify API key is configured in Claude's settings
- Look at node logs for detailed errors

### "No MCP servers found"

- MCP servers are optional-not all installations have them
- Check `~/.claude/mcp.json` exists if you've configured servers
- Run semantic recon for deeper tool discovery
