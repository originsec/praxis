# Configuration

Praxis uses LLMs for several features-semantic operations, tool discovery during recon, traffic summarization. You'll need to configure at least one provider to use these capabilities.

## LLM Providers

Open **Settings** (`Ctrl+S`) → **LLM** in the praxis TUI.

### Adding a Model

In **Settings** (`Ctrl+S`), stay on the **LLM** tab, use `↑`/`↓` to select
**Add Model**, and press `Enter`. In the form:

1. Choose a **Provider**
2. Enter your **API Key** (optional for local providers — Ollama and Custom)
3. For **Custom**, and optionally for **Ollama**, set a **Base URL**
4. Refresh the provider model list when supported, or enter a model name manually
5. Save with `Ctrl+S`

Use `Tab` / `Shift+Tab` to move between Settings tabs. The TUI status bar
lists the available form controls.

### Supported Providers

Anthropic, OpenAI, Google (Gemini), Groq, Cerebras, Mistral, xAI, NVIDIA, MiniMax, Moonshot, Fireworks AI, OpenRouter, Ollama (local), Custom (OpenAI-compatible).

### Local Model Providers

Two providers are designed for local or self-hosted inference:

**Ollama** — defaults to `http://localhost:11434/v1`, so if you are
running a stock Ollama install nothing else is needed. API key is
optional. Model discovery uses Ollama's native `/api/tags` endpoint, so
the model-refresh action works even though Ollama is strictly OpenAI-API
compatible for inference. Override the base URL on the model definition
if Ollama is listening elsewhere.

**Custom (OpenAI-Compatible)** — for vLLM, llama.cpp, LM Studio,
Text-Generation-Inference, or any endpoint that implements
`/v1/chat/completions`. You must set a base URL on the model definition;
API key is optional. Model discovery probes `/models` on the configured
base URL.

### Feature Assignment

Once you've added models, assign them to features under **Feature Selection**:

**Orchestrator** - Powers an interactive AI agent that can autonomously manage operations. It will drive target agents through multi-turn conversations to achieve a given task, so it needs a capable model that follows tool-calling instructions reliably. A companion **Max Tokens** setting caps how long its responses can run.

**Semantic Operations** - Used when executing operations through agents. Pick something capable.

**Semantic Parser** - Used during semantic recon to extract tool definitions from config files. Speed matters here since it runs multiple times; a fast model like Haiku or GPT-4o-mini works well.

**Traffic Parser** - Summarizes intercepted traffic. Again, speed is valuable; you don't need the most powerful model.

**Documentation Helper** - Powers the [Help Assistant](../usage/help-assistant.md) (`Ctrl+H`), which answers questions about using Praxis from the bundled documentation. Falls back to the Orchestrator model when unset.

### Speed vs. Capability

For parser features (Semantic Parser, Traffic Parser), we recommend providers with fast inference:

- **Cerebras** and **Groq** have very fast time-to-first-token and overall throughput
- This matters when you're running recon across multiple agents or parsing lots of traffic

For Orchestrator and Semantic Operations, capability matters more than raw speed. Use a model that's good at reasoning and tool use.

## Environment Variables

Most configuration is done through the praxis TUI, but some things are set via environment variables:

### Service

| Variable | Default | Description |
|----------|---------|-------------|
| `PRAXIS_DATABASE_URL` | SQLite in home dir | Database connection string |
| `PRAXIS_RABBITMQ_URL` | `amqp://praxis:praxis@localhost:5672` | RabbitMQ URL |
| `RUST_LOG` | `info` | Log level filter (tracing `EnvFilter` syntax) |

### Node

| Variable | Default | Description |
|----------|---------|-------------|
| `PRAXIS_RABBITMQ_URL` | `amqp://praxis:praxis@localhost:5672` | RabbitMQ URL |
| `RUST_LOG` | `info` | Log level filter (tracing `EnvFilter` syntax) |

### Database

By default, Praxis uses SQLite stored at `~/.praxis/operations.db`. For PostgreSQL and production deployments, see [Database Configuration](../deployment/database.md).

## Model Reference Format

When specifying models in operations or chains, use the format:

```
provider::model
```

For example:
- `anthropic::claude-sonnet-4-20250514`
- `openai::gpt-4o`
- `groq::llama-3.3-70b-versatile`

This lets you override the default model for specific operations that might need more (or less) capability.

## Next Steps

With LLMs configured, you're ready to:

- [Run through the quick start](./quick-start.md)
- [Enable semantic recon](../usage/recon.md) for deeper tool discovery
- [Execute semantic operations](../usage/semantic-operations.md)
