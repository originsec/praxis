# Claude Desktop Connector

The Claude Desktop connector enables interaction with the Claude Desktop Electron app. **Windows only. Experimental.**

> **Warning**: This connector is hacky and flaky. It relies on UI Automation to navigate Electron menus, a raw WebSocket CDP connection to the Node.js main process debugger, and a JavaScript proxy to tunnel CDP commands to the renderer. Any Claude Desktop update can break it. Use at your own risk.

## Overview

Claude Desktop is an Electron app. Unlike browser-based agents with standard DevTools, Electron's main process debugger must be enabled manually via the app's Developer menu. The connector automates this using Windows UI Automation, then establishes a CDP connection to control the renderer.

## Architecture

```diagram
agents/claudedesktop.lua        <- Agent-specific: selectors, UIA flow, config
    | uses
praxis.uiautomation            <- Lua helper: BFS element search, menu navigation
praxis.devtools                 <- Lua helper: Electron proxy, transact loop
    | uses
praxis.uia_*                   <- Native Rust: Windows UI Automation bindings
praxis.cdp_*                   <- Native Rust: Raw WebSocket CDP (Node.js inspector)
```

## How It Works

### Session Creation

1. **Kill any running instance** — Unconditionally kills any existing `claude.exe` process before spawning a fresh one
2. **Write developer_settings.json** — Ensures `allowDevTools: true` so the Developer menu appears
3. **Launch Claude Desktop** — Spawns via `spawn_detached`. Release builds use a hidden desktop by default so UIA automation doesn't disturb the user's visible desktop; debug builds default to visible instead (either can be overridden with `PRAXIS_NOT_HIDDEN`). When a hidden desktop is used, the connector switches to it before the UIA steps below
4. **Enable debugger via UI Automation** — Navigates Menu > Developer > Enable Main Process Debugger using Windows UIA. Uses BFS element search to avoid hangs on Electron's large UIA tree. Retries up to 3 times
5. **Dismiss Inspector dialogs** — Closes any Inspector popup windows that appear after enabling the debugger
6. **Switch back or minimize** — Switches back to the original desktop if a hidden desktop was used; otherwise falls back to minimizing the window
7. **Connect to CDP on port 9229** — Uses raw WebSocket (`tokio-tungstenite`) instead of chromiumoxide, because Electron's main process debugger is a Node.js inspector endpoint with no pages/tabs
8. **Set up Electron renderer proxy** — Injects JavaScript into the main process that uses `webContents.debugger` to proxy CDP commands to the renderer matching `claude.ai`
9. **Post-initialize** — Selects Chat/Code mode, waits for input readiness, sends Ctrl+Shift+I for incognito mode

### Why Not Just Use DevTools Directly?

Electron's renderer DevTools aren't exposed on a network port by default. The main process debugger (port 9229) is a Node.js inspector, not Chrome DevTools. To reach the renderer, the connector:

1. Connects to the main process via raw WebSocket
2. Runs `Runtime.evaluate` to call Electron's `webContents.debugger.attach()` and `sendCommand()` APIs
3. Sets up a JavaScript proxy (`globalThis.cdp()`) that forwards CDP commands from the main process to the renderer

This is the `setup_electron_proxy` function in `praxis.devtools`.

### BFS Element Search

The standard `uiautomation` Rust crate's `find_first(Descendants)` hangs for 25+ seconds on Electron's large UIA tree. The connector implements breadth-first search (`uia_find_bfs`) using `find_first(Children)` at each level, which returns instantly.

## Fingerprinting

Searches for `claude.exe` in:
1. PATH
2. `%LOCALAPPDATA%\AnthropicClaude`

Verifies it's Claude Desktop (not Claude Code) and extracts the version via PowerShell.

## Interception

Traffic is intercepted for:
- Domains: `api.anthropic.com`, `a-api.anthropic.com`
- URL pattern: `messages`

## Working Directories

- **Chat** (default) — Claude Desktop's chat mode
- **Code** — Currently disabled (wraps Claude Code, which has a dedicated connector)

## Reconnaissance

Config discovery from `%APPDATA%\Claude`:
- `claude_desktop_config.json` — Global settings, MCP server definitions
- `config.json` — App config
- `extensions-blocklist.json` — Extension blocklist
- `Preferences` — App preferences
- `developer_settings.json` — Developer settings
- `logs/*.log` — Log files

## Known Issues

- **Session creation is slow** (~15-20s) due to UIA menu navigation, Inspector dialog dismissal, and CDP connection handshake
- **UIA is fragile** — Menu structure changes in Claude Desktop will break the debugger enablement flow
- **Response detection may not work** — The CSS selectors for message elements and the stop button (`div.contents`, `button[aria-label="Stop response"]`) may not match the current Claude Desktop UI
- **Visible only as a fallback** — Release builds automate on a hidden desktop by default so the user's desktop isn't disturbed; if a hidden desktop can't be obtained (or in debug builds, which default to visible), the connector falls back to a normal spawn and minimizes the window instead
- **Electron updates break things** — Any change to the Electron DevTools menu structure, renderer URL, or DOM will require selector updates

## Requirements

- **Windows** — This connector is Windows-only
- **Claude Desktop** — Must be installed (not Claude Code)
- **Hidden desktop by default** — `spawn_detached` is called with `use_hidden_desktop = true`; release builds automate on a hidden desktop (switching back afterward), debug builds default to visible, and either default can be overridden with the `PRAXIS_NOT_HIDDEN` environment variable

## Troubleshooting

### "Menu trigger not found: Menu"

The UIA BFS search couldn't find the Menu button. Claude Desktop may have changed its UI structure, or the window didn't load in time.

### "URL error: URL scheme not supported"

The CDP connection is trying to use an HTTP URL instead of a WebSocket URL. Check that the Node.js debugger on port 9229 is responding with a valid `/json` endpoint.

### "No pages found" then falls back to raw WebSocket

This is normal. Electron's main process debugger has no pages — the raw WebSocket fallback is the expected path.

### Session creation hangs

Check the node logs for which step is stuck. Common culprits:
- UIA menu navigation (enable_debugger)
- Inspector dialog dismissal
- CDP connection (port 9229 not responding)
