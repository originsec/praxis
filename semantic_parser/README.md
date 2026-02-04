# Semantic Parser

A Rust library for extracting structured data from unstructured text using AI models.

## Overview

`semantic_parser` provides a simple API for parsing natural language text into structured JSON data using AI language models.

## Features

- **Provider Agnostic**: Works with multiple AI providers (Anthropic, OpenAI, Groq, etc.)
- **JSON Schema Validation**: Ensures AI output matches your expected schema
- **Automatic Retries**: Handles invalid JSON responses with configurable retry logic

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
semantic_parser = { path = "../semantic_parser" }
```

## Quick Start

```rust
use semantic_parser::{SemanticParser, ParserConfig, Provider};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create parser with your AI provider credentials
    let config = ParserConfig {
        provider: Provider::Anthropic,
        api_key: std::env::var("ANTHROPIC_API_KEY")?,
        model: "claude-haiku-4-5-20241022".to_string(),
        max_retries: 3,
        max_tokens: Some(4096),
    };

    let parser = SemanticParser::new(config)?;

    // Define your schema
    let schema = r#"{
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer" }
        }
    }"#;

    // Parse unstructured text
    let text = "John is 30 years old and lives in New York.";
    let prompt = "Extract the person's name and age from the text.";

    let result = parser.parse(text, prompt, schema).await?;
    println!("Parsed: {}", result);

    Ok(())
}
```

Output:
```
Parsed: {"name": "John", "age": 30}
```

## Examples

Run the example with:

```bash
ANTHROPIC_API_KEY=your_key cargo run --example basic_parse
```

## Configuration

The `ParserConfig` struct supports these options:

| Field | Type | Description |
|-------|------|-------------|
| `provider` | `Provider` | AI provider (Anthropic, OpenAI, Groq, etc.) |
| `api_key` | `String` | API key for the provider |
| `model` | `String` | Model to use for parsing |
| `max_retries` | `usize` | Max retry attempts for invalid JSON (default: 3) |
| `max_tokens` | `Option<u32>` | Max tokens in response (default: 4096) |

## Supported Providers

- Anthropic (Claude models)
- OpenAI (GPT models)
- xAI (Grok)
- Groq
- Mistral
- Cohere
- DeepSeek
- Google Gemini
- Cerebras (OpenAI-compatible)

## Error Handling

The library provides detailed error messages for common failure cases:

- Invalid JSON from AI model (after retries exhausted)
- API connection failures
- Invalid provider configuration

## License

Apache-2.0
