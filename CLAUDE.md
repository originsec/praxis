# Best practices for working with the praxis code

## Code comments

- Source files do not need a comment at the start.
- No need to be overly verbose with comments. If the code is self-explanatory, don't add a comment.
- Comment, rather, larger blocks where it's necessary to understand what's going on.
- Single-line comments should be rare - and mostly used when necessary inline (i.e. same line as code, after the code).
- Comments should be descriptive and explanatory, not just stating an obvious fact.
- Generally, comments should be in the following format (note: newlines before & after):

```
<newline>
//
// Comment here followed by full stop.
// Multiline is ok but wrap to 80 characters. An example is this line you can
// see how I've wrapped it.
//
<newline>
```

## Architecture

- Main components are: Node, Service, Web
- semantic_parser is a component that is eventually to be packaged separately
- Do not re-implement the same code in different components. Favour sharing code.
- Shared code goes in common/
- Even within a component, favour identifying opportunities for shared code and sensible abstractions.
- A sensible abstraction is one where it is likely that there could be an expansion over a single consumer of any subcomponent/interface.

## Logging

- Never use `common::log_*` macros in `node/src/runtime.rs` event log forwarder task. These macros send to the event log channel, which the forwarder processes, creating an infinite recursion loop when RabbitMQ fails. Use `tracing::*` directly instead.

## Database Migrations

- Any DB schema changes must include a migration in `service/src/database/mod.rs` `run_migrations()`.
- Migrations must be idempotent (safe to run multiple times).
- Migrations must work for both SQLite and PostgreSQL.
- For SQLite, use `ALTER TABLE ... ADD COLUMN` and ignore errors (no `IF NOT EXISTS` support for columns).
- For PostgreSQL, use `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`.
- The `service_config` table can store version tracking keys (e.g., `builtin_scripts_version`) to coordinate data migrations across service upgrades.

## Hunting Tables

- Any hunting table changes (additions, removals, or field changes) require updating the table schemas in all three locations:
  - `service/src/hunting/tables.rs` — backend column definitions and `resolve_table()`
  - `service/src/hunting/sql.rs` — SQL config for DB-backed tables
  - `web/frontend/src/components/hunting/KqlCodeEditor.tsx` — `TABLE_SCHEMAS` for autocomplete
  - `web/frontend/src/pages/HuntingPage.tsx` — `TABLES` schema sidebar
  - `web/frontend/src/components/command/HuntingModal.tsx` — `TABLES` schema sidebar (command center)
  - `docs/src/usage/hunting.md` — documentation
- All table lists must be in **alphabetical order** by table name.

## CLI

- The CLI (`cli/`) provides both an interactive terminal UI (default mode) and non-interactive subcommands.
- Non-interactive commands (via `-C` or direct subcommands) must be kept in sync with the interactive UI capabilities where applicable.
- The MCP SSE server (exposed by the service) provides tool access for AI agents. Common code in `common/src/mcp/ops.rs` is used by both the CLI and MCP.

## Documentation

- Documentation lives in `docs/` and is built with mdBook.
- Documentation must always be kept up to date to reflect any code changes. When adding, removing, or modifying features, update the corresponding documentation in `docs/src/`.
- Key documentation files:
  - `docs/src/usage/cli.md` - CLI terminal UI and non-interactive commands
  - `docs/src/usage/recon.md` - Reconnaissance features
  - `docs/src/architecture/` - Node architecture
  - `docs/src/connectors/` - Agent connector documentation
- But look through entire docs/src to locate any areas that may need updates
- Don't make changes to CLAUDE.md unless specifically instructed to

## Pull Requests

- PR descriptions must be excellent and comprehensive — they are the primary record of what changed and why.
- Group changes by area/theme with `###` subheadings (e.g. "MCP Server", "Node Runtime", "TUI", "Web Frontend").
- Each item should explain **what** changed and **why**, not just list files. Use bold for key items.
- Include a test plan with a checklist of verification steps.
- Look at ALL commits in the branch (not just the latest) to ensure nothing is missed.
- Keep the PR title short (under 70 characters); put detail in the body.

## Release Process

- **Do not initiate any release steps unless explicitly asked to.**

1. **Version bump**: Ensure the version is updated on the `prerelease` branch. It should be a minor bump over the current `main` version (unless otherwise specified).
2. **Create PR**: Create a PR from `prerelease` into `main` with a clear, well-written description summarizing all changes since the last release.
3. **Squash merge**: Squash-merge the PR into `main`.
4. **GitHub release**: Create a GitHub release on `main` with an excellent changelog covering all notable changes, new features, fixes, and breaking changes.
5. **Tag and push**: Create a version tag matching the version number set on the `prerelease` branch (e.g., `v0.10.0`), push the tag, and push the release.
