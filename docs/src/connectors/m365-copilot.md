# M365 Copilot Connector

The M365 Copilot connector enables interaction with Microsoft 365 Copilot. **Windows only.**

## Overview

Microsoft 365 Copilot runs in a WebView2 browser component. The connector uses Chrome DevTools Protocol (CDP) via the `praxis.devtools` Lua library to interact with the Copilot UI programmatically.

## Architecture

```diagram
agents/m365copilot.lua        ← Agent-specific: selectors, recon JS, config
    ↓ uses
praxis.devtools               ← Lua helper: generic transact loop, lifecycle
    ↓ uses
praxis.cdp_*                  ← Native Rust: CDP connection, JS eval, DOM ops
```

The M365 connector is a Lua agent (`agents/m365copilot.lua`) that uses the shared `praxis.devtools` library for DevTools session management and the native `praxis.cdp_*` API for CDP operations.

## Fingerprinting

The connector checks for Copilot availability:
1. Searches for `M365Copilot.exe` in running processes
2. Checks the Windows package install location (`Microsoft.MicrosoftOfficeHub`)

## Interception

Traffic is intercepted for:
- Domain: `substrate.office.com`
- URL pattern: `m365Copilot/Chathub`

## Session Management

### Creating Sessions

When you create a session:
1. All running `M365Copilot.exe` processes are killed by name
2. All existing CDP connections are drained and their process trees terminated
3. App is launched with a random debugging port via `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`
4. On Windows, the process is spawned on a **hidden desktop** so the window is invisible (release builds by default; debug builds default to visible). Override with `PRAXIS_NOT_HIDDEN=1` to show the window, or `PRAXIS_NOT_HIDDEN=0` to hide it in debug builds. If the hidden desktop cannot be created, the window is **minimized** after DevTools connects.
5. CDP connection is established via chromiumoxide (5 attempts, 2s interval)
6. Post-initialization: waits for input element, clicks Work/Web toggle, opens new private chat

### Transacting

The `praxis.devtools` library provides a generic transact loop:
1. Waits for input element (`#m365-chat-editor-target-element`)
2. Counts existing messages
3. Clicks input, inserts text via CDP `InsertText` (handles emojis/special chars), presses Enter
4. Polls for response (250ms interval, 120s max)
5. Detects idle state (no activity for ~3s) and retries up to 3 times

Response completion is detected by checking:
- New `div[data-testid="markdown-reply"]` elements
- Absence of "Stop generating" button
- Non-empty response text

### Aborting

CDP sessions support `abort_transaction` — when a transaction is cancelled (e.g. via the praxis TUI), the entire process tree is terminated by PID. The session state stores the `process_id` which the Rust session layer uses for process-level cancellation.

### Cleanup Safety Net

When a session is closed (or dropped), the Rust layer performs cleanup even if the Lua `session_close` callback fails:
- Kills the process tree by PID
- Removes the CDP connection handle from the global map

This prevents orphaned browser processes after crashes or Lua errors.

### Working Directories

M365 Copilot supports two working directories that map to toggle buttons:
- **Work** - Enterprise/organizational context
- **Web** - Web search context

## Reconnaissance

### Static Recon

Discovers user identity and available toggles by executing JavaScript in a temporary DevTools session:
- User identity via `nestedAppAuthService` profile object (UPN and display name)
- Available toggles (Work/Web) by checking for toggle button elements

Recon requires a valid `process_path` from a prior fingerprint. If fingerprint hasn't run, recon returns empty results.

### Semantic Recon

Creates a temporary session and asks Copilot to list its tools, then parses the response with the semantic parser. Uses a dual-prompt fallback: tries a JSON-format prompt first, and if zero tools are parsed, retries with a high-level overview prompt.

## Requirements

- **Windows** - This connector is Windows-only
- **M365 License** - User must have Copilot access
- **Logged In** - User must be authenticated to Microsoft

## Troubleshooting

### "Agent not fingerprinted"

- Verify the user has M365 Copilot access
- Check that `M365Copilot.exe` is installed

### "Session creation failed"

- Check that the app can launch with debugging enabled
- Verify M365 authentication is valid
- Look for firewall blocking debugging ports (9222-9999 range)
- Check node logs for CDP errors
- Set `PRAXIS_NOT_HIDDEN=1` to see the app window for debugging

### "Responses not captured"

- UI selectors may have changed; report as an issue
- Check for Copilot page structure changes

## Limitations

- No config editing (browser-based)
- No MCP server discovery
- Requires active M365 authentication
- Session reliability depends on Microsoft's UI
