# Configuration

Praxis uses LLMs for several features-semantic operations, tool discovery during recon, traffic summarization. You'll need to configure at least one provider to use these capabilities.

## LLM Providers

Go to **Settings** → **LLM Providers** in the web UI.

### Adding a Model

1. Click **Add Model**
2. Select a **Provider**
3. Enter your **API Key** (optional for local providers — Ollama and Custom)
4. For **Custom**, and optionally for **Ollama**, set a **Base URL**
5. Click the refresh button to pull available models from the provider (not supported by all providers), or enter the model name manually
6. Click **Save**

### Supported Providers

Anthropic, OpenAI, Google (Gemini), Groq, Cerebras, Mistral, xAI, NVIDIA, MiniMax, Moonshot, Fireworks AI, OpenRouter, Ollama (local), Custom (OpenAI-compatible).

### Local Model Providers

Two providers are designed for local or self-hosted inference:

**Ollama** — defaults to `http://localhost:11434/v1`, so if you are
running a stock Ollama install nothing else is needed. API key is
optional. Model discovery uses Ollama's native `/api/tags` endpoint, so
the refresh button works even though Ollama is strictly OpenAI-API
compatible for inference. Override the base URL on the model definition
if Ollama is listening elsewhere.

**Custom (OpenAI-Compatible)** — for vLLM, llama.cpp, LM Studio,
Text-Generation-Inference, or any endpoint that implements
`/v1/chat/completions`. You must set a base URL on the model definition;
API key is optional. Model discovery probes `/models` on the configured
base URL.

### Feature Assignment

Once you've added models, assign them to features:

**Semantic Operations** - Used when executing operations through agents. This is the "brain" that orchestrates what the agent should do. Pick something capable.

**Semantic Parser** - Used during semantic recon to extract tool definitions from config files. Speed matters here since it runs multiple times; a fast model like Haiku or GPT-4o-mini works well.

**Traffic Parser** - Summarizes intercepted traffic. Again, speed is valuable; you don't need the most powerful model.

### Speed vs. Capability

For parser features (Semantic Parser, Traffic Parser), we recommend providers with fast inference:

- **Cerebras** and **Groq** have very fast time-to-first-token and overall throughput
- This matters when you're running recon across multiple agents or parsing lots of traffic

For Semantic Operations, capability matters more than raw speed. Use a model that's good at reasoning and tool use.

## Environment Variables

Most configuration is done through the web UI, but some things are set via environment variables:

### Service

| Variable | Default | Description |
|----------|---------|-------------|
| `PRAXIS_DATABASE_URL` | SQLite in home dir | Database connection string |
| `PRAXIS_RABBITMQ_URL` | `amqp://praxis:praxis@localhost:5672` | RabbitMQ URL |

### Node

| Variable | Default | Description |
|----------|---------|-------------|
| `PRAXIS_RABBITMQ_URL` | `amqp://praxis:praxis@localhost:5672` | RabbitMQ URL |

### Database

By default, Praxis uses SQLite stored at `~/.praxis_operations.db`. For PostgreSQL and production deployments, see [Database Configuration](../deployment/database.md).

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
