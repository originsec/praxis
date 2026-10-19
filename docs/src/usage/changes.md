# Change Ledger

Praxis can change agent configuration files on endpoints: through the recon
overlay editor (`Ctrl+E`), the ACP `_praxis/write_file` extension, the MCP
`write_file` / `change_apply` tools and the CLI. The **change ledger**
records every one of those writes on the service, so at the end of an
authorised engagement you know exactly what was changed where, and you can
put every endpoint back the way it was.

## What is recorded

Each config write produces one ledger entry with:

| Field | Meaning |
|-------|---------|
| change id | UUID; listings show the 8-character short form, and every command accepts a unique prefix |
| node / agent / path | Where the file lives. The agent and config type are taken from the stored recon result for that path when the caller does not say |
| file kind | `json`, `jsonc`, `toml`, `yaml`, `markdown` or `text` (from the extension unless overridden) |
| operation | `overwrite` or a structured operation (see below), plus a one-line summary such as `set /env/PRAXIS_DEMO` |
| operator | Who made the change: `<os-user>@tui`, `<os-user>@cli`, `<os-user>@mcp`, or `client:<id>` for other ACP clients |
| engagement | Free-text engagement label (optional) |
| note | Free-text reason (optional) |
| timestamp | When the write happened (UTC) |
| before / after | SHA-256 of the file before and after, the **full before content** (or "file did not exist") and the after content |
| undo steps | For structured changes: the inverse operations, used to undo just this change later |
| state | `active` until reverted, then `reverted` with a link to the revert entry |
| drift | Result of the last drift check |

Reverts are recorded as entries too (`kind = revert`, pointing at the change
they undid), so the ledger is a complete audit trail. The ledger lives in the
`config_changes` table (SQLite or PostgreSQL) and can be queried with the
[`ConfigChangeLogs`](./log-query.md#configchangelogs) log-query table.

### How a write happens

1. The service reads the file on the node in full (`_praxis/snapshot_files`):
   existence, SHA-256 and content. Files over 4 MiB or that are not UTF-8
   text are refused, because they could not be restored faithfully.
2. It computes the new content: the raw replacement for an overwrite, or a
   structured edit of the current content.
3. It writes the file with a compare-and-swap: the node only writes if the
   file still has the hash from step 1 (or still does not exist). If
   something changed the file in between, the service re-reads and retries
   (up to three times) instead of clobbering it.
4. It records the entry. If recording fails, the write is rolled back, so
   there is never an unrecorded change.

A write that would not change the file (same content, an entry that is
already in a list, a key already set to that value) is reported as a no-op
and not recorded.

## Structured changes

Instead of rewriting a whole file, a structured change edits one thing and
leaves everything else (other keys, comments, ordering, formatting) as it
was. Change types are generic descriptors — target file kind + key path +
operation; you supply the values.

| Operation | Files | Parameters | Effect |
|-----------|-------|------------|--------|
| `set` | JSON, JSONC, TOML, YAML | key path, value | Set the value at the key path, creating missing parent objects/tables |
| `merge` | JSON, JSONC, TOML, YAML | key path, object value | Deep-merge the object into the object at the key path |
| `remove` | JSON, JSONC, TOML, YAML | key path | Remove the key (error if absent) |
| `list_append` | JSON, JSONC, TOML, YAML | key path, value | Append the value to the list at the key path (created if missing); no-op if an equal entry exists |
| `list_remove` | JSON, JSONC, TOML, YAML | key path, value | Remove the first entry equal to the value (error if absent) |
| `append_block` | Markdown, text | content | Append the content wrapped in marker comments that carry the change id |
| `remove_block` | Markdown, text | block id | Remove the marker block written by change *block id* |
| `overwrite` | any | content | Replace the whole file |

**Key paths** use JSON Pointer syntax: `/mcpServers/demo`, `/env/FOO`,
`/permissions/allow`. Escape `/` inside a key as `~1` and `~` as `~0`; the
leading slash is optional. Keyed maps (MCP servers, env vars, hook tables)
are addressed with `set` / `remove` on `/<map>/<key>`; lists (permission
rules, hook entries) with `list_append` / `list_remove`.

**Values** are JSON. On the CLI, `--value` is parsed as JSON and falls back
to a plain string, so `--value bar` and `--value '{"command":"x"}'` both work.

**Marker blocks** look like this:

```markdown
<!-- praxis-change:begin 1a2b3c4d-0000-4000-8000-000000000000 -->
...content...
<!-- praxis-change:end 1a2b3c4d-0000-4000-8000-000000000000 -->
```

Removing the block (on revert, or with `remove_block`) restores the
surrounding text byte for byte, even if other parts of the file were edited
in the meantime.

### Format handling

- **JSON / JSONC** are edited through a concrete syntax tree, so comments,
  trailing commas, key order and indentation are preserved exactly.
- **TOML** is edited with a format-preserving TOML editor. Objects placed in
  a table become `[a.b]` sub-tables; values inside arrays and inline tables
  stay inline. TOML has no `null`, so null values are rejected.
- **YAML** keeps all data and key order, but the file is re-emitted, so
  comments and custom layout are **not** preserved. Use `overwrite` (or the
  recon editor) if a YAML file's comments matter.

Keys inside an object *value* you supply (for example a new MCP server
definition) are written in alphabetical order; existing keys in the file keep
their order.

Every structured edit is verified before it is written: the edited file is
parsed again and must equal the expected document. If it does not, the
change is refused rather than risk corrupting the endpoint's config.

### Example

A harmless change, adding an environment variable to Claude Code's settings
and then a comment-only marker block to its instructions file:

```bash
export PRAXIS_ENGAGEMENT=acme-2026q4

praxis change apply --node 3f2a --path /home/alice/.claude/settings.json \
  --op set --key /env/PRAXIS_DEMO --value 1 --note "ledger demo"

praxis change apply --node 3f2a --path /home/alice/.claude/CLAUDE.md \
  --op append_block --content "<!-- praxis ledger demo -->" --dry-run
```

`--dry-run` prints the diff without writing anything.

## Reverting

Reverting a change looks at the file as it is now:

- **Unchanged since Praxis wrote it** (hash matches the recorded after-hash):
  the original content is restored exactly. If the file did not exist before
  the change, it is deleted.
- **Modified since**: nothing is written by default. You get a conflict with
  a unified diff of the later modifications and choose:
  - `structured` — undo only this change on top of the current content (the
    recorded undo steps: restore the old value, remove the added key or list
    entry, strip the marker block). Not available for raw overwrites.
  - `force` — restore the recorded before-content, discarding the later edits.
- **Already back to the original**: recorded as reverted without a write.

Revert-all (by node, agent, engagement label, or an explicit list of change
ids) reverts the matching active changes **newest first**, so stacked changes
to the same file unwind in order. With the default conflict policy
(`abort`), a conflicting change is skipped and reported; an older change to
the same file then conflicts too, so nothing is ever restored out of order.
Resolve skipped changes individually. Revert-all refuses to run without a
filter unless `all` is set explicitly.

All reverts are cancellable: `Ctrl+C` in the CLI, `Ctrl+C` on the TUI
Changes tab, or cancelling the MCP tool call. Bulk reverts stop between
files.

## Drift check

The drift check re-reads every active change's file on its node (all nodes
in parallel) and reports:

| Status | Meaning |
|--------|---------|
| `in_sync` | File still matches what Praxis wrote |
| `superseded` | File matches a later active Praxis change to the same file |
| `modified` | File was changed by something else since Praxis wrote it |
| `missing` | File no longer exists |
| `unreachable` | Node offline or the read failed |

The result is stored on each entry, so listings show the last known status.

## Surfaces

| Task | TUI | CLI | MCP |
|------|-----|-----|-----|
| List / filter | Operations → **Changes** tab, `/` and `s` | `praxis change list` | `change_list` |
| Detail + diff | Changes tab detail pane | `praxis change show <id>` | `change_get` |
| Apply structured change | — (recon `Ctrl+E` for full-file edits) | `praxis change apply` | `change_apply` |
| Revert one | `Ctrl+Z` | `praxis change revert <id>` | `change_revert` |
| Revert many | `Ctrl+X` (everything active in view) | `praxis change revert-all` | `change_revert_all` |
| Drift check | `r` | `praxis change status` | `change_status` |
| Engagement label | `e` on the Changes tab | `--engagement` / `$PRAXIS_ENGAGEMENT` | `engagement` parameter |
| Query | Log Query: `ConfigChangeLogs` | — | — |

See [TUI](./tui.md#changes-tab), [CLI](./cli.md#available-commands) and
[MCP Server](./mcp.md#change-ledger) for the full key and parameter lists.

## Limits

- Only writes made through Praxis are recorded. Edits made directly on the
  endpoint (by the user, the agent, or an agent session you drive) show up
  only as drift.
- Connector-internal writes made by a Lua connector itself (for example the
  Claude Desktop connector enabling `allowDevTools` when a session starts)
  are not routed through the ledger.
- Files must be UTF-8 text no larger than 4 MiB.
- Remote (bridge) nodes do not implement the file extensions, so the ledger
  cannot change files on them.
