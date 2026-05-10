# Contributing

Praxis is open source and welcomes contributions. This guide covers the codebase structure and how to get involved.

## Repository Structure

```
praxis/
├── common/              # Shared types and utilities
├── node/                # Node component (runs on targets)
├── service/             # Service component (backend)
├── cli/                 # CLI / TUI (first-party client)
├── semantic_parser/     # LLM-based text parsing library
├── docs/                # This documentation
├── .github/             # CI/CD workflows
└── docker-compose.yml   # Local development setup
```

## Components

### Common (`common/`)

Shared code used by all components:
- Message types and serialization
- RabbitMQ utilities
- AI client abstraction
- Logging macros

When adding functionality needed by multiple components, put it here.

### Node (`node/`)

The agent that runs on target machines:
- Agent connectors (Claude Code, Gemini, etc.)
- Traffic interception
- Session management
- Terminal handling

```
node/src/
├── agent_connectors/    # Per-agent implementations
│   └── lua/             # Lua connector runtime + CDP helpers
├── intercept/           # Traffic interception
├── terminal/            # PTY terminal
└── runtime.rs           # Main event loop
```

Lua-based agent scripts (Claude Code, Codex, Cursor, Gemini, M365 Copilot) live in `agents/` at the project root and are embedded into the binary at build time.

### Service (`service/`)

The backend that coordinates everything:
- Node tracking
- Semantic operations
- Chain execution
- Database persistence

```
service/src/
├── semantic_ops/        # Operation execution
├── chain_execution/     # Chain runner
├── database/            # Persistence layer
└── config/              # Service configuration
```

### CLI (`cli/`)

The first-party Praxis client:
- Interactive terminal UI (Ratatui)
- Non-interactive subcommands for scripting
- ACP bridge mode (stdin/stdout) for external tooling
- RabbitMQ-based connection to the service

```
cli/
└── src/
    ├── app/             # TUI windows (orchestrator, nodes, intercept, ...)
    ├── components/      # Shared widgets
    └── main.rs          # Entry point
```

### Semantic Parser (`semantic_parser/`)

Standalone library for LLM-based parsing:
- Schema-based extraction
- Multi-provider support
- Retry logic

See [Semantic Parser](semantic-parser.md) for details.

## Development Workflow

### Setup

1. Install Rust
2. Start RabbitMQ: `docker compose up rabbitmq`
3. Build: `cargo build`
4. Run service: `cargo run --bin praxis_service`
5. Run node: `cargo run --bin praxis_node`
6. Run TUI: `cargo run --bin praxis_cli`

### Environment Variables for Development

| Variable | Default (debug) | Description |
|----------|----------------|-------------|
| `PRAXIS_IGNORE_SERVICE_AGENTS` | `1` | When `1`, node ignores Lua scripts pushed from the service and uses only embedded scripts. Set to `0` to test service-managed agent scripts. |
| `PRAXIS_DATABASE_URL` | SQLite in home dir | Database connection string |
| `PRAXIS_RABBITMQ_URL` | `amqp://praxis:praxis@localhost:5672` | RabbitMQ connection |

### Making Changes

1. Create a branch
2. Make changes
3. Run tests: `cargo test`
4. Build: `cargo build`
5. Test manually
6. Submit PR

### Code Style

- Follow existing patterns
- Use `common::log_*` macros for logging (except in `node/src/runtime.rs` event forwarder-use `tracing::*` there to avoid recursion)
- Prefer explicit over clever
- Comment non-obvious blocks

### Adding Agent Connectors

See [Adding New Connectors](../connectors/adding-new.md). Prefer Lua-based connectors for CLI agents — they can be developed and tested at runtime via the TUI's Settings → Agents tab without recompiling.

Lua agent scripts live in `agents/` at the project root and are embedded into binaries at build time. Shared libraries are at `node/src/agent_connectors/lua/lib/` (`helpers.lua` for common utilities, `devtools.lua` for CDP/DevTools support).

### Adding Operations

Operations are JSON definitions. Add to the library via the TUI's
Operations window (`Ctrl+P`) or directly to the database.

## Testing

### Unit Tests

```bash
cargo test
```

### Integration Tests

Run the full stack and test manually. Automated integration tests are on the roadmap.

### Testing Connectors

1. Install the target agent
2. Run a node
3. Verify fingerprinting
4. Test session creation
5. Test interception

## Pull Requests

### Before Submitting

- [ ] Code builds without warnings
- [ ] Tests pass
- [ ] Changes are documented
- [ ] Commit messages are clear

### PR Process

1. Open a PR against `main`
2. Describe the change
3. Wait for review
4. Address feedback
5. Merge when approved

## Feature Requests

Open an issue with:
- What you want
- Why it's useful
- Any implementation ideas

## Bug Reports

Open an issue with:
- What happened
- What you expected
- Steps to reproduce
- Logs if available

## Contact

- Issues: GitHub Issues
- Email: david.kaplan@preludesecurity.com
- Twitter: @depletionmode
