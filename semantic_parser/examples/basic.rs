//! Basic usage example for the semantic parser

use semantic_parser::{ParserConfig, Provider, SemanticParser};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY environment variable must be set");

    let config = ParserConfig {
        provider: Provider::Anthropic,
        api_key,
        model: "claude-haiku-4-5-20241022".to_string(),
        max_retries: 3,
        max_tokens: Some(4096),
        base_url: None,
    };

    let parser = SemanticParser::new(config)?;

    let schema = r#"{
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "Person's name" },
            "age": { "type": "integer", "description": "Person's age" },
            "occupation": { "type": "string", "description": "Person's job or profession" },
            "location": { "type": "string", "description": "Where the person lives" }
        },
        "required": ["name"]
    }"#;

    let text = "John Smith is a 35-year-old software engineer living in San Francisco. \
                He has been working at a tech startup for the past 5 years.";

    let prompt = "Extract information about the person from the text.";

    println!("Parsing text...");
    let result = parser.parse(text, prompt, schema).await?;

    println!("Parsed result:");
    println!("{}", result);

    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    println!("\nExtracted fields:");
    if let Some(name) = parsed.get("name") {
        println!("  Name: {}", name);
    }
    if let Some(age) = parsed.get("age") {
        println!("  Age: {}", age);
    }
    if let Some(occupation) = parsed.get("occupation") {
        println!("  Occupation: {}", occupation);
    }
    if let Some(location) = parsed.get("location") {
        println!("  Location: {}", location);
    }

    Ok(())
}
