# M365 Copilot Connector

The M365 Copilot connector enables interaction with Microsoft 365 Copilot running in Edge. **Windows only.**

## Overview

Microsoft 365 Copilot is different from CLI-based agents-it runs in a browser. The connector supports two interaction modes: DevTools Protocol (default) and UI Automation.

## Fingerprinting

The connector checks for:
1. **Microsoft Edge** - The browser must be installed
2. **Copilot availability** - The M365 Copilot web interface must be accessible

## Interception

Traffic is intercepted for domains related to M365 Copilot:
- `substrate.office.com`
- Related Microsoft backend services

The URL pattern filters for Copilot-specific API calls.

## Session Modes

### DevTools Mode (Default)

Uses Chrome DevTools Protocol to interact with Copilot:

```
┌─────────────────────────────────────────────────────────┐
│                     Praxis Node                          │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │              DevTools Adapter                       │ │
│  │                                                     │ │
│  │  Edge ──CDP Connection──▶ Copilot Page             │ │
│  │   │                          │                      │ │
│  │   └─ Hidden Desktop ─────────┘                      │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

**How it works:**
1. Launches Edge with remote debugging enabled
2. Navigates to M365 Copilot
3. Uses CDP to inject prompts and extract responses
4. Runs on a hidden desktop to avoid interfering with user activity

**Advantages:**
- More reliable than UI automation
- Faster response extraction
- Works without visible UI

### UI Automation Mode

Uses Windows UI Automation to interact with the browser UI directly. This mode is experimental and less reliable.

**How it works:**
1. Finds the Edge window with Copilot
2. Locates input and output elements via UI Automation
3. Simulates typing and reads responses

**Disadvantages:**
- Flaky element detection
- Slower and more fragile
- Requires visible window

## Hidden Desktop

By default, DevTools mode runs Edge on a hidden desktop. This:
- Prevents interference with the user's screen
- Allows Copilot interactions without visible UI
- Keeps the session isolated

Set `PRAXIS_NOT_HIDDEN=1` to disable this for debugging.

## Reconnaissance

### Static Recon

Limited compared to CLI agents:
- Browser profile information
- Copilot configuration (if accessible)

### Semantic Recon

Semantic recon attempts to discover:
- Available Copilot capabilities
- Connected M365 services
- User context

## Session Management

### Creating Sessions

When you create a session:
1. Edge launches with debugging port
2. Hidden desktop is created (if enabled)
3. CDP connection is established
4. Copilot page loads and authenticates

### Transacting

Prompts are sent by:
1. Finding the input field via CDP
2. Injecting the prompt text
3. Triggering submission
4. Waiting for and extracting the response

### Authentication

M365 Copilot requires Microsoft authentication. The session uses the user's existing Edge profile and login state.

## Requirements

- **Windows** - This connector is Windows-only
- **Microsoft Edge** - Required browser
- **M365 License** - User must have Copilot access
- **Logged In** - User must be authenticated to Microsoft

## Troubleshooting

### "Agent not fingerprinted"

- Ensure Microsoft Edge is installed
- Verify the user has M365 Copilot access
- Check that Copilot works manually in Edge

### "Session creation failed"

- Check Edge can launch with debugging enabled
- Verify M365 authentication is valid
- Look for firewall blocking debugging ports
- Check node logs for CDP errors

### "Responses not captured"

- UI may have changed; report as an issue
- Try DevTools mode if using UI Automation
- Check for Copilot page structure changes

## Limitations

- No config editing (browser-based)
- No MCP server discovery
- Requires active M365 authentication
- Session reliability depends on Microsoft's UI
