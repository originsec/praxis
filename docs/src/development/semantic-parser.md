# Semantic Parser

The semantic parser is a standalone library for extracting structured data from unstructured text using LLMs. It's used throughout Praxis for various parsing tasks.

## What It Does

Given:
- Raw text (config files, transcripts, logs)
- A JSON schema
- Parsing instructions

The semantic parser returns structured JSON matching the schema.

## Usage in Praxis

### Semantic Recon

When running semantic reconnaissance, the parser extracts tool definitions from config files:

```
Input: Claude Code mcp.json file contents
Schema: { "tools": [{ "name": string, "description": string }] }
Output: Structured tool list
```

## Library API

### Basic Usage

```rust
use semantic_parser::{SemanticParser, ParserConfig, Provider};

// Configure the parser
let config = ParserConfig {
    provider: Provider::Anthropic,
    api_key: "sk-...".to_string(),
    model: "claude-haiku-4-5-20241022".to_string(),
    max_retries: 3,
    max_tokens: Some(4096),
    base_url: None,
};

// Create parser
let parser = SemanticParser::new(config)?;

// Parse text
let schema = r#"{"name": "string", "version": "string"}"#;
let prompt = "Extract the package name and version";
let text = "This is mypackage version 1.2.3";

let result = parser.parse(text, prompt, schema).await?;
// Returns: {"name": "mypackage", "version": "1.2.3"}
```

## Provider Support

The parser supports multiple LLM providers:

| Provider | ID | Notes |
|----------|----|----|
| Anthropic | `anthropic` | Claude models |
| OpenAI | `openai` | GPT models |
| Google | `gemini` | Gemini models |
| Groq | `groq` | Fast inference |
| Cerebras | `cerebras` | Fast inference |
| Mistral | `mistral` | Mistral models |
| xAI | `xai` | Grok models |
| NVIDIA | `nvidia` | NIM models |
| MiniMax | `minimax` | MiniMax models |
| Moonshot AI | `moonshot` | Moonshot models |
| Fireworks AI | `fireworksai` | Fireworks-hosted models |
| OpenRouter | `openrouter` | Multi-provider routing |
| Ollama | `ollama` | Local models |
| Custom | `custom` | Any OpenAI-compatible endpoint |

## Model Selection

For parsing tasks, use fast, cheap models:

**Recommended:**
- `claude-haiku-4-5-20241022` (Anthropic)
- `gpt-4o-mini` (OpenAI)
- `gemini-1.5-flash` (Google)
- `llama-3.3-70b-versatile` (Groq)

Fast inference providers like Groq and Cerebras work well since parsing typically requires many sequential calls.

## Schema Format

Schemas are JSON Schema-like strings:

```json
{
  "tools": [
    {
      "name": "string",
      "description": "string",
      "parameters": {}
    }
  ],
  "config_path": "string"
}
```

The parser attempts to return valid JSON matching this structure.

## Retry Logic

The parser includes built-in retry logic:

1. Send request to LLM
2. Parse response as JSON
3. If invalid, retry with the identical prompt (the previous error is logged but not fed back into the next attempt - retries are blind repeats, not feedback-guided)
4. Return result or error after max retries

Default: 3 retries.

## Error Handling

The parser returns `Result<String>`:

- **Success**: Valid JSON string
- **Error**: Parsing failed after retries, or API error

```rust
match parser.parse(text, prompt, schema).await {
    Ok(json) => process_result(&json),
    Err(e) => log::warn!("Parsing failed: {}", e),
}
```

### Non-Throwing Alternative

`try_parse` offers the same parsing behavior without returning a `Result`. It always returns a `ParseResult` struct instead of an error:

```rust
pub struct ParseResult {
    pub success: bool,
    pub json: Option<String>,
    pub error: Option<String>,
}

let result = parser.try_parse(text, prompt, schema).await;
if result.success {
    process_result(&result.json.unwrap());
} else {
    log::warn!("Parsing failed: {}", result.error.unwrap_or_default());
}
```

## Configuration in Praxis

The semantic parser LLM is configured in Settings:

1. Open **Settings** (`Ctrl+S`) → **LLM**
2. Configure **Semantic Parser** provider and model
3. Save

This configures only the Semantic Parser feature slot. Praxis has 5 independently configurable LLM feature slots (Semantic Parser, Traffic Parser, Semantic Ops, Orchestrator, Doc Helper), each set separately in Settings - the Semantic Parser slot governs only its own request handler, not the other LLM-backed features.

## Performance Considerations

**Latency**: Each parse call makes an LLM request. For bulk parsing, consider batching.

**Cost**: Fast models are cheaper. Choose based on parsing complexity.

**Accuracy**: More capable models produce better results for complex extractions.

## Examples

### Parse MCP Config

```rust
let schema = r#"{
  "servers": [{
    "name": "string",
    "command": "string",
    "args": ["string"],
    "env": {}
  }]
}"#;

let result = parser.parse(
    &mcp_json_contents,
    "Extract all MCP server configurations",
    schema
).await?;
```

### Parse Session Transcript

```rust
let schema = r#"{
  "files_accessed": ["string"],
  "commands_run": ["string"],
  "api_keys_mentioned": ["string"]
}"#;

let result = parser.parse(
    &transcript,
    "Extract file paths, commands, and any API keys from this conversation",
    schema
).await?;
```

### Parse Traffic

```rust
let schema = r#"{
  "model": "string",
  "prompt_preview": "string",
  "token_count": "number",
  "has_tool_calls": "boolean"
}"#;

let result = parser.parse(
    &request_body,
    "Extract LLM request metadata",
    schema
).await?;
```

## Standalone Use

The semantic parser can be used outside of Praxis:

```toml
[dependencies]
semantic_parser = { path = "../semantic_parser" }
```

It's designed to be a general-purpose LLM parsing library. Note that `semantic_parser` has a mandatory path dependency on Praxis's internal `common` (`praxis_common`) crate, so copying just the `semantic_parser` directory into an unrelated project won't build on its own - `common` needs to come with it.
