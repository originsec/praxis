# Perplexity Comet Connector

The Comet connector covers [Perplexity Comet](https://www.perplexity.ai/comet),
the Chromium-based agentic browser. macOS and Windows (Comet has no Linux
build). **Experimental / untested**: it was built from public sources without
access to a machine running Comet.

## Fingerprinting

| Platform | Detection |
|----------|-----------|
| macOS | `Comet.app` in `/Applications` or `~/Applications` with bundle id `ai.perplexity.comet` (from the Homebrew cask) |
| Windows | `comet.exe` under `%LOCALAPPDATA%\Perplexity\Comet\Application\`, then `Program Files` and `Program Files (x86)`. Version from the file's `ProductVersion`. |

## Interception

The default intercept target `[comet]` captures `www.perplexity.ai` /
`perplexity.ai` requests whose path matches `rest/sse/perplexity_ask`, the
answer stream as reverse-engineered by third-party clients. Chromium uses the
OS trust store on macOS and Windows.

## Reconnaissance

From the Comet user-data directory (`~/Library/Application Support/Comet` on
macOS, `%LOCALAPPDATA%\Perplexity\Comet\User Data` on Windows):

- `Local State` (`browser_local_state`)
- Per profile (`Default`, `Profile N`): `Preferences`
  (`browser_preferences`), `Secure Preferences`
  (`browser_secure_preferences`), and the manifest of every unpacked
  extension (`browser_extension:Comet/<profile>/<id>`). Comet's built-in
  assistant extension is reported to be `mcjlamohcooanphmebaiigheeeoplihb`.

No MCP servers, skills or session history are collected.

## Sessions

Sessions drive the Perplexity assistant in a Comet tab over the Chrome
DevTools Protocol:

1. Any running Comet is closed. A profile can only be open once, and Comet
   must be relaunched with remote debugging.
2. Comet is launched with `--remote-debugging-port=<random>` and
   `https://www.perplexity.ai/` on the user's own profile, so the session is
   signed in as that user. Upstream Chrome 136+ ignores remote debugging on
   the default profile. The community `comet-mcp` project attaches to Comet
   this way, which suggests Comet doesn't enforce that restriction. This is
   unverified.
3. Each prompt is typed into the composer (`[contenteditable="true"]`, or a
   textarea on older builds) and submitted with Enter. The reply is the last
   `main [class*="prose"]` element once no button labelled "Stop" remains.

The selectors come from [comet-mcp](https://github.com/hanzili/comet-mcp)
(`src/comet-ai.ts`) and will need adjusting against a real build. The
transact loop and the `extra_args` launch path were tested against Chromium
on Linux with a page that mimics that DOM. On Windows the browser runs on a
hidden desktop like the other DevTools connectors.
