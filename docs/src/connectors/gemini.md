# Gemini CLI Connector

The Gemini connector enables interaction with Google's Gemini CLI agent.

## Overview

Gemini CLI is Google's command-line AI assistant. Like Claude Code, it can read files, execute commands, and work with code. The connector works on Linux, Windows, and macOS.

## Fingerprinting

The connector looks for Gemini CLI by checking:

1. **Config file existence** - `~/.gemini/settings.json`
2. **Process search** - Looking for running `gemini` processes
3. **Binary location** - Finding the `gemini` executable in PATH

## Interception

Traffic is intercepted for the domain:
- `generativelanguage.googleapis.com`

When interception is enabled, you'll see:
- Prompts sent to the Gemini API
- Responses including assistant messages
- Function/tool calls and results

## Reconnaissance

### Static Recon

Static reconnaissance discovers:

**Configuration**
- Settings file (`~/.gemini/settings.json`)
- Model preferences and API configuration

**Extensions**
- Gemini extensions (similar to MCP servers)
- Extension names and configurations

**Sessions**
- Session files under `~/.gemini/sessions/`
- Conversation history

### Semantic Recon

When semantic recon is enabled, the connector also:
- Parses configuration for tool definitions
- Identifies available Gemini capabilities
- Extracts extension details

## Session Management

Sessions work similarly to Claude Code-the connector spawns Gemini CLI in a PTY:

```
┌─────────────────────────────────────────┐
│              Praxis Node                 │
│                                          │
│  ┌─────────────────────────────────┐    │
│  │         PTY Session              │    │
│  │                                  │    │
│  │  gemini ─────────────────────────┼────┼──▶ Gemini Process
│  │    │                             │    │
│  │    └─ stdin/stdout ──────────────│    │
│  └─────────────────────────────────┘    │
└─────────────────────────────────────────┘
```

### Session Context

**Working Directory** - Where Gemini should operate.

**YOLO Mode** - Auto-approve tool calls (behavior depends on Gemini's configuration).

## Config Editing

You can view and edit Gemini's configuration from the Praxis UI:
- Settings file with model and API preferences
- Extension configurations

## Files and Paths

| File | Path | Content |
|------|------|---------|
| Settings | `~/.gemini/settings.json` | Main configuration |
| Sessions | `~/.gemini/sessions/` | Session history |

## Troubleshooting

### "Agent not fingerprinted"

- Ensure Gemini CLI is installed
- Check that `~/.gemini/settings.json` exists
- Verify the `gemini` command is in PATH

### "Session creation failed"

- Check that Gemini CLI can run normally from terminal
- Verify Google API credentials are configured
- Look at node logs for detailed errors
