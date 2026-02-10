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

## Documentation

- Documentation lives in `docs/` and is built with mdBook.
- When making code changes, update the corresponding documentation in `docs/src/`.
- Key documentation files:
  - `docs/src/architecture/` - Node architecture
  - `docs/src/connectors/` - Agent connector documentation
- But look through entire docs/src to locate any areas that may need updates
- Don't make changes to CLAUDE.md unless specifically instructed to
