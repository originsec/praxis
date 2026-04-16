//! Example showing how to parse into typed Rust structures

use semantic_parser::{ParserConfig, Provider, SemanticParser};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Project {
    name: String,
    description: String,
    languages: Vec<String>,
    contributors: Vec<Contributor>,
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Contributor {
    name: String,
    role: String,
}

const PROJECT_SCHEMA: &str = r#"{
    "type": "object",
    "properties": {
        "name": { "type": "string", "description": "Project name" },
        "description": { "type": "string", "description": "Brief project description" },
        "languages": {
            "type": "array",
            "items": { "type": "string" },
            "description": "Programming languages used"
        },
        "contributors": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "role": { "type": "string" }
                },
                "required": ["name", "role"]
            },
            "description": "People who contributed to the project"
        },
        "tags": {
            "type": "array",
            "items": { "type": "string" },
            "description": "Keywords or tags describing the project"
        }
    },
    "required": ["name", "description", "languages", "contributors", "tags"]
}"#;

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

    let text = r#"
        The Quantum project is an open-source machine learning framework designed for
        real-time data processing. It's written primarily in Rust with some Python
        bindings and a small amount of C++ for performance-critical sections.

        The core team includes Alice Chen who serves as the lead architect, Bob Martinez
        handling the documentation and developer relations, and Carol Williams who
        focuses on the Python integration layer.

        The project is known for its speed, memory safety, and excellent documentation.
        It's particularly popular in the fintech and scientific computing communities.
    "#;

    let prompt = "Extract project information including all contributors and technologies used.";

    println!("Parsing project information...\n");
    let json = parser.parse(text, prompt, PROJECT_SCHEMA).await?;

    let project: Project = serde_json::from_str(&json)?;

    println!("Project: {}", project.name);
    println!("Description: {}", project.description);
    println!("\nLanguages:");
    for lang in &project.languages {
        println!("  - {}", lang);
    }
    println!("\nContributors:");
    for contributor in &project.contributors {
        println!("  - {} ({})", contributor.name, contributor.role);
    }
    println!("\nTags:");
    for tag in &project.tags {
        println!("  - {}", tag);
    }

    Ok(())
}
