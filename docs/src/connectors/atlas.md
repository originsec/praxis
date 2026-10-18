# ChatGPT Atlas Connector

The Atlas connector fingerprints and inventories OpenAI's ChatGPT Atlas
browser on macOS. **Experimental / untested. Recon-only.**

OpenAI discontinued Atlas on 2026-08-09, and it never shipped for Windows.
Installs and their browsing profiles can outlive the service, so the
connector exists to find leftover installs rather than to drive the browser.

## Fingerprinting

`ChatGPT Atlas.app` in `/Applications` or `~/Applications` with bundle id
`com.openai.atlas` (or `com.openai.atlas.beta`). Version from
`CFBundleShortVersionString`.

## Reconnaissance

- `~/Library/Preferences/com.openai.atlas.plist` and
  `com.openai.atlas.web.plist` (`app_preferences`)
- The Chromium profile root
  `~/Library/Application Support/com.openai.atlas/browser-data/host/`:
  `Local State` plus each profile's preferences and extension manifests
  (same item types as [Comet](./comet.md#reconnaissance))

Paths are from the Homebrew cask's uninstall stanza and OpenAI's former
`atlas` skill in [openai/skills](https://github.com/openai/skills).

## Interception and Sessions

Not supported. Atlas embeds Chromium out-of-process (OWL), and the service it
talked to is shut down.
