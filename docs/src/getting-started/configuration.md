# Configuration

Praxis uses LLMs for several features-semantic operations, tool discovery during recon, traffic summarization. You'll need to configure at least one provider to use these capabilities.

## LLM Providers

Go to **Settings** → **LLM Providers** in the web UI.

### Adding a Model

1. Click **Add Model**
2. Select a **Provider** (Anthropic, OpenAI, Google, etc.)
3. Enter the **Model** name (e.g., `claude-sonnet-4-20250514`)
4. Enter your **API Key**
5. Click **Save**

### Supported Providers

| Provider | Example Models | Notes |
|----------|---------------|-------|
| **Anthropic** | `claude-sonnet-4-20250514`, `claude-haiku-4-5-20241022` | Claude models |
| **OpenAI** | `gpt-4o`, `gpt-4o-mini` | GPT models |
| **Google** | `gemini-1.5-pro`, `gemini-1.5-flash` | Gemini models |
| **Groq** | `llama-3.3-70b-versatile` | Fast inference |
| **Cerebras** | `llama-3.3-70b` | Very fast inference |
| **Mistral** | `mistral-large-latest` | Mistral models |
| **xAI** | `grok-2-latest` | Grok models |
| **Ollama** | `llama3`, `codellama` | Local models |

### Feature Assignment

Once you've added models, assign them to features:

**Semantic Operations** - Used when executing operations through agents. This is the "brain" that orchestrates what the agent should do. Pick something capable.

**Semantic Parser** - Used during semantic recon to extract tool definitions from config files. Speed matters here since it runs multiple times; a fast model like Haiku or GPT-4o-mini works well.

**Traffic Parser** - Summarizes intercepted traffic. Again, speed is valuable; you don't need the most powerful model.

**Atlas** - Used for the Nexus multi-agent chat feature. Pick something capable that handles conversation well.

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

By default, Praxis uses SQLite stored at `~/.praxis_operations.db`. For production or multi-instance deployments, use PostgreSQL:

```bash
PRAXIS_DATABASE_URL=postgresql://user:pass@localhost:5432/praxis ./praxis_service
```

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
