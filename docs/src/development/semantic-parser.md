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

### Traffic Analysis

When traffic parsing is enabled, the parser analyzes LLM traffic:

```
Input: Intercepted request/response
Schema: { "prompt_summary": string, "tool_calls": [...] }
Output: Structured analysis
```

### Session Analysis

Parsing session transcripts for capability discovery:

```
Input: Session history file
Schema: { "capabilities": [...], "sensitive_data": [...] }
Output: Extracted information
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
| Google | `google` | Gemini models |
| Groq | `groq` | Fast inference |
| Cerebras | `cerebras` | Fast inference |
| Mistral | `mistral` | Mistral models |
| xAI | `xai` | Grok models |
| NVIDIA | `nvidia` | NIM models |
| Ollama | `ollama` | Local models |

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
3. If invalid, retry with feedback
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

## Configuration in Praxis

The semantic parser LLM is configured in Settings:

1. Go to **Settings** → **LLM Providers**
2. Configure **Semantic Parser** provider and model
3. Save

The service uses this configuration for all parsing operations.

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

It's designed to be a general-purpose LLM parsing library.
