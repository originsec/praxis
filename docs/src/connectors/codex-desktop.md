# Codex Desktop Connector

The Codex Desktop connector fingerprints OpenAI's Codex desktop app (now
branded as the ChatGPT desktop app) and inventories the state it adds on top
of Codex CLI. **Experimental / untested**: it was built from OpenAI's docs and
the `openai/codex` source without access to a machine running the app.
**Recon-only.**

## Relationship to the Codex CLI connector

The app embeds the Codex binary (`codex app-server`) and shares `~/.codex`
with Codex CLI. The [`codex`](./codex.md) connector already reports
`config.toml`, `auth.json`, MCP servers, prompts and the session transcripts
in `~/.codex/sessions`. Threads created in the app are stored there too. To
avoid double-reporting, this connector only reports app-specific state.

## Fingerprinting

| Platform | Detection |
|----------|-----------|
| macOS | `Codex.app` or `ChatGPT.app` in `/Applications` or `~/Applications`, accepted only if `CFBundleIdentifier` is `com.openai.codex` (the legacy ChatGPT app is `com.openai.chat`). Executable from `CFBundleExecutable`, version from `CFBundleShortVersionString`. |
| Windows | MSIX package `OpenAI.Codex` (Store family `OpenAI.Codex_2p2nqsd0c76g0`) via `Get-AppxPackage`. Reports the install location and package version. |
| Linux | Not fingerprinted. The `chatgpt` Linux app doesn't document its install paths, and verifying it would mean launching the GUI. |

Sources: `codex-rs/cli/src/desktop_app/{mac,windows}.rs` in
[openai/codex](https://github.com/openai/codex) and
[Windows deployment](https://learn.chatgpt.com/docs/enterprise/windows-deployment.md).

## Interception

There is no default intercept target. The app's model traffic comes from the
embedded Codex binary (to `chatgpt.com/backend-api/codex` for ChatGPT sign-in,
or `api.openai.com/v1` for API keys). That binary has the same CA-trust
limitation as Codex CLI (see [Codex CLI](./codex.md#interception)).

## Reconnaissance

| Item | Config type |
|------|-------------|
| `~/.codex/.codex-global-state.json` (app projects and selected project) | `app_global_state` |
| `~/.codex/sqlite/codex-dev.db` (automations / scheduled tasks) | `automations_database` |
| macOS: `~/Library/Logs/com.openai.codex/YYYY/MM/DD/*` | `log` |
| macOS: `~/Library/Application Support/Codex/*.json` | `app_settings` |
| Windows: `%LOCALAPPDATA%\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Roaming\Codex\*.json` | `app_settings` |
| macOS MDM: `/Library/Managed Preferences/[<user>/]com.openai.codex.plist` | `managed_preferences` |

App worktrees under `~/.codex/worktrees/` are reported as project paths.

The global-state file, the automations database location and both Electron
userData locations come from community reports and haven't been verified on a
real install.

## Sessions

Not supported. The app is Electron, and community tools launch it with
`--remote-debugging-port`, but there are no sourced DOM selectors to drive a
DevTools session. Run Codex sessions through the [`codex`](./codex.md)
connector instead, which uses the same `~/.codex`.
