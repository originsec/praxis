# Claude for Chrome Connector

The Claude for Chrome connector finds Anthropic's
[Claude browser extension](https://chromewebstore.google.com/detail/claude/fcoeoabgfenejglbffodgkkbkcdhcgfn)
in Chromium-family browser profiles. Linux, macOS and Windows. **Recon-only.**

## Fingerprinting

The extension ID `fcoeoabgfenejglbffodgkkbkcdhcgfn` is taken from the Chrome Web
Store listing and [Claude Code's Chrome docs](https://code.claude.com/docs/en/chrome).
For every user home, the connector checks each profile (`Default`,
`Profile N`) of:

| Browser | Linux | macOS | Windows |
|---------|-------|-------|---------|
| Google Chrome (and Beta) | `~/.config/google-chrome` | `~/Library/Application Support/Google/Chrome` | `%LOCALAPPDATA%\Google\Chrome\User Data` |
| Chromium | `~/.config/chromium` | `~/Library/Application Support/Chromium` | `%LOCALAPPDATA%\Chromium\User Data` |
| Microsoft Edge | `~/.config/microsoft-edge` | `~/Library/Application Support/Microsoft Edge` | `%LOCALAPPDATA%\Microsoft\Edge\User Data` |
| Brave | `~/.config/BraveSoftware/Brave-Browser` | `~/Library/Application Support/BraveSoftware/Brave-Browser` | `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data` |

The agent counts as installed when
`<profile>/Extensions/<id>/<version>_N/manifest.json` exists. The version is
the manifest's `version`, and `process_path` is the manifest path.

## Reconnaissance

- `extension_manifest:<browser>/<profile>`: the extension manifest for each
  install
- `browser_preferences:<browser>/<profile>`: that profile's `Preferences`,
  which records the extension's granted permissions and settings
- `native_messaging_host:<browser>`: native messaging host manifests that let
  the extension launch local binaries:
  - `com.anthropic.claude_code_browser_extension.json` (Claude Code's browser
    integration)
  - `com.anthropic.claude_browser_extension.json` (Claude Desktop/Cowork)

  These live in `<user data dir>/NativeMessagingHosts/` on Linux and macOS.
  On Windows hosts are registered in the registry, which isn't scanned.

## Interception

There is no default intercept target. The extension talks to `claude.ai` and
`api.anthropic.com`, and its Claude Code bridge uses
`bridge.claudeusercontent.com`. Those endpoints overlap with the Claude Code
and Claude Desktop targets, so a default would make that traffic ambiguous.
Add a `[claudechrome]` section under **Settings → Intercept** if you need it.

## Sessions

Not supported. The side panel only works in the user's signed-in profile,
and Chrome 136+ ignores `--remote-debugging-port` on the default user-data
directory, so DevTools can't be attached to it.
