# Endpoint Posture

Posture answers two questions before you run an operation against a node:
**will my change stick**, and **is anyone watching**. For every
fingerprinted agent on a node it reports:

1. **Managed / enterprise policy** — whether a higher-precedence managed
   policy exists (so a user-level config edit to an enforced key will not
   take effect), which keys it enforces, and the raw policy content.
2. **Telemetry** — whether the agent is configured to export telemetry
   (OpenTelemetry and vendor equivalents) and to which endpoint. Only the
   destination host is reported; URLs are reduced to `host[:port]` and
   header/token values are redacted.
3. **Sandbox and permission mode** — the agent's sandbox/approval settings,
   with the value that wins at effective precedence marked.

It also reports **host security products** (EDR/XDR, monitoring and
AI-agent-governance agents) detected on the node by process, service and
install-path signatures.

Posture is read-only: the node only reads files, managed preferences,
registry values and the process table.

## Running a posture scan

- **TUI** — open the recon overlay for an agent (Nodes window, `→` into
  the detail pane, select an agent, `r`) and switch to the **Posture**
  tab (`4` or `Tab`). A stored result is shown immediately; if none is
  stored the node is scanned. Press `r` on the tab to re-scan.
- **CLI** — `praxis node posture <node-prefix> [--agent <name>] [--refresh] [--json]`
  (see [CLI](./cli.md#available-commands)).
- **MCP** — `posture_get` (see [MCP Server](./mcp.md#reconnaissance)).
- **Log query** — the `PostureLogs` table (see
  [Log Query](./log-query.md#posturelogs)).

Scans run on the node via the `_praxis/posture` ACP extension. The service
issues the request (adding the security-product signatures and the node's
fingerprinted agent list), stores the result per node, and returns it.
Each node keeps only its latest result.

## Reading the result

Each setting is reported once per source that sets it, with its scope:

| Scope | Meaning |
|-------|---------|
| `managed` | Organisation-managed policy (system file, MDM profile, registry policy) |
| `user` | Per-user config in a user's home directory (evaluated for every home on the node) |
| `defaults` | System-wide defaults that user config overrides (Gemini `system-defaults.json`) |
| `process` | The node's own process environment, which agent sessions launched by the node inherit |

A value is **effective** when no higher-precedence source sets the same
key for that user; otherwise it is **overridden**. So "will my change
stick?" comes down to: if the key you plan to edit in a user file is
already set by a `managed` source, the edit will not take effect.

Praxis evaluates precedence **per key**. Some agents combine managed
sources differently (for example, Claude Code applies only the
highest-priority managed source by default, and Copilot CLI combines
`sandbox`/`permissions` most-restrictively); these caveats are shown as
notes on the agent's posture. Server-side policy (claude.ai console,
Cursor team dashboard, Codex cloud requirements, GitHub `.github-private`
settings) is not visible on the endpoint.

Managed policy files also appear in the recon **Config** tab as
`managed_policy` (or `system_defaults`) items, and can be read there even
though they live outside user home directories. They cannot be written.

AI-agent governance products often work by installing **hooks** in an
agent's managed settings that call a vendor binary, rather than by running
a recognisable process. Check the managed policy's `hooks` keys as well as
the Host Security list.

## Policy locations checked

Paths come from vendor documentation; the definitions live in
`node/src/posture/agents.toml` (with source links) and are keyed by agent
`short_name`.

| Agent | Managed / system sources (highest first) | User source |
|-------|-------------------------------------------|-------------|
| Claude Code (`claudecode`) | macOS MDM `com.anthropic.claudecode`; `HKLM\SOFTWARE\Policies\ClaudeCode\Settings`; `managed-settings.json`, `managed-settings.d/*.json` and `managed-mcp.json` in `/Library/Application Support/ClaudeCode/` (macOS), `/etc/claude-code/` (Linux), `C:\Program Files\ClaudeCode\` (Windows); `HKCU\SOFTWARE\Policies\ClaudeCode\Settings` | `~/.claude/settings.json` |
| Claude Desktop (`claudedesktop`) | macOS MDM `com.anthropic.claudefordesktop`; `HKLM`/`HKCU\SOFTWARE\Policies\Claude`; `/etc/claude-desktop/managed-settings.json` (Linux) | — |
| Codex (`codex`) | macOS MDM `com.openai.codex` (`requirements_toml_base64`, `config_toml_base64`); `/etc/codex/requirements.toml` (Unix) or `%ProgramData%\OpenAI\Codex\requirements.toml`; `/etc/codex/managed_config.toml` (Unix) or `~/.codex/managed_config.toml` (Windows) | `~/.codex/config.toml` |
| Cursor (`cursor`) | Enterprise hooks `/Library/Application Support/Cursor/hooks.json`, `/etc/cursor/hooks.json`, `C:\ProgramData\Cursor\hooks.json`; Linux editor policy `~/.cursor/policy.json` | `~/.cursor/sandbox.json`, `~/.cursor/cli-config.json`, `~/.cursor/permissions.json` |
| Droid (`droid`) | `settings.json` in `/Library/Application Support/Factory/`, `/etc/factory/`, `C:\Program Files\Factory\` | `~/.factory/settings.json` |
| Gemini CLI (`gemini`) | System `settings.json` in `/etc/gemini-cli/`, `/Library/Application Support/GeminiCli/`, `C:\ProgramData\gemini-cli\` (or `GEMINI_CLI_SYSTEM_SETTINGS_PATH`) | `~/.gemini/settings.json`, then `system-defaults.json` (or `GEMINI_CLI_SYSTEM_DEFAULTS_PATH`) as lowest |
| Antigravity CLI (`agy`) | none documented (Google Cloud console only) | `~/.gemini/antigravity-cli/settings.json` |
| Pi (`pi`) | none documented | `~/.pi/agent/settings.json` |
| GitHub Copilot CLI (`copilot`) | macOS MDM `com.github.copilot`; `HKLM\SOFTWARE\Policies\GitHubCopilot`; `managed-settings.json` in `/etc/github-copilot/`, `/Library/Application Support/GitHubCopilot/`, `%ProgramFiles%\GitHubCopilot\` | `~/.copilot/settings.json` |

Agents without a definition (for example `m365copilot`) are listed with a
"No posture definition" note. Project-level config files are not
evaluated.

## Security product signatures

Signatures are a TOML virtual file stored on the service, like intercept
targets. Edit it from **Settings → Posture** in the TUI (`Enter` on
"Edit virtual file in `$EDITOR`"; "Reset to defaults" restores the
shipped list). The service validates the file before saving and sends the
parsed list with every posture request, so edits apply on the next scan
without restarting nodes.

```toml
[crowdstrike-falcon]
name = "CrowdStrike Falcon"
category = "edr"                      # edr, monitoring, ai_governance, ...
processes = { windows = ["CSFalconService.exe"], linux = ["falcon-sensor"] }
services = { windows = ["CSAgent"], linux = ["falcon-sensor"], macos = ["com.crowdstrike.falcon.Agent"] }
paths = { linux = ["/opt/CrowdStrike"], macos = ["/Applications/Falcon.app"] }
source = "documentation link"
```

- **processes** — matched case-insensitively against running process
  names; `.exe` is optional. A match marks the product **running**.
- **services** — Windows service key under
  `HKLM\SYSTEM\CurrentControlSet\Services`, a systemd unit (`.service`
  implied) or `/etc/init.d` script on Linux, a launchd plist label in
  `/Library/LaunchDaemons` or `/Library/LaunchAgents` on macOS.
- **paths** — install paths; `%VAR%` / `${VAR}` expand from the node's
  environment.

The defaults cover Microsoft Defender for Endpoint, CrowdStrike Falcon,
SentinelOne, Cortex XDR, Carbon Black Cloud, Sophos (Linux), Elastic
Agent/Defend, Wazuh, osquery and the Nightfall AI agent. Each entry links
the documentation it is based on; entries marked "third-party" rely on
non-vendor sources because the vendor docs require a login. An empty
product list only means none of the configured signatures matched —
`signatures_evaluated` in the result says how many were checked.
