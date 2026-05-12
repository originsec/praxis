use serde::Deserialize;

#[derive(Deserialize)]
struct OpenAIModelsResponse {
    data: Vec<OpenAIModel>,
}

#[derive(Deserialize)]
struct OpenAIModel {
    id: String,
}

#[derive(Deserialize)]
struct AnthropicModelsResponse {
    data: Vec<AnthropicModel>,
}

#[derive(Deserialize)]
struct AnthropicModel {
    id: String,
}

#[derive(Deserialize)]
struct GeminiModelsResponse {
    models: Vec<GeminiModel>,
}

#[derive(Deserialize)]
struct GeminiModel {
    //
    // Format: "models/gemini-1.5-pro".
    //
    name: String,
}

#[derive(Deserialize)]
struct OllamaModelsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}

/// Fetch models from OpenAI-compatible APIs (OpenAI, Groq, Mistral, xAI, Cerebras)
pub async fn fetch_openai_compatible_models(
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/models", base_url);

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, body));
    }

    let data: OpenAIModelsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(data.data.into_iter().map(|m| m.id).collect())
}

/// Fetch models from Anthropic API
pub async fn fetch_anthropic_models(api_key: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();

    let response = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, body));
    }

    let data: AnthropicModelsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(data.data.into_iter().map(|m| m.id).collect())
}

/// Fetch models from Gemini API
pub async fn fetch_gemini_models(api_key: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models?key={}",
        api_key
    );

    let response = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, body));
    }

    let data: GeminiModelsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    //
    // Strip "models/" prefix from names.
    //
    Ok(data
        .models
        .into_iter()
        .map(|m| {
            m.name
                .strip_prefix("models/")
                .unwrap_or(&m.name)
                .to_string()
        })
        .collect())
}

/// Probe an endpoint for OpenAI-compatible models (for discovery)
///
/// This is similar to fetch_openai_compatible_models but designed for probing
/// unknown endpoints:
/// - Short timeouts
/// - Optional API key
/// - Accepts invalid certificates for HTTPS probing
pub async fn probe_openai_compatible_endpoint(
    base_url: &str,
    api_key: Option<&str>,
    accept_invalid_certs: bool,
) -> Result<Vec<String>, String> {
    let client_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .connect_timeout(std::time::Duration::from_secs(3));

    let client = if accept_invalid_certs {
        client_builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
            .map_err(|e| format!("Failed to build client: {}", e))?
    } else {
        client_builder
            .build()
            .map_err(|e| format!("Failed to build client: {}", e))?
    };

    let url = format!("{}/models", base_url);

    let mut request = client.get(&url);
    if let Some(key) = api_key {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, body));
    }

    let data: OpenAIModelsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(data.data.into_iter().map(|m| m.id).collect())
}

/// Fetch models from a local Ollama instance.
///
/// Uses the Ollama-native `/api/tags` endpoint for model discovery.
/// The base_url should be the Ollama server root (e.g. `http://localhost:11434`),
/// not the OpenAI-compatible `/v1` path.
pub async fn fetch_ollama_models(base_url: Option<&str>) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();

    //
    // Strip /v1 suffix if present — the tags endpoint is on the native API.
    //
    let root = base_url.unwrap_or("http://localhost:11434");
    let root = root.trim_end_matches('/').trim_end_matches("/v1");
    let url = format!("{}/api/tags", root);

    let response = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Failed to connect to Ollama (is it running?): {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Ollama error {}: {}", status, body));
    }

    let data: OllamaModelsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(data.models.into_iter().map(|m| m.name).collect())
}

/// Fetch models for a given provider.
///
/// The optional `base_url` overrides the default endpoint for providers that
/// support it (Ollama, Custom, or any OpenAI-compatible provider).
pub async fn fetch_models_for_provider(
    provider: &str,
    api_key: &str,
    base_url: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut models = match provider {
        "anthropic" => fetch_anthropic_models(api_key).await,
        "openai" => {
            let url = base_url.unwrap_or("https://api.openai.com/v1");
            fetch_openai_compatible_models(url, api_key).await
        }
        "groq" => fetch_openai_compatible_models("https://api.groq.com/openai/v1", api_key).await,
        "mistral" => fetch_openai_compatible_models("https://api.mistral.ai/v1", api_key).await,
        "xai" => fetch_openai_compatible_models("https://api.x.ai/v1", api_key).await,
        "gemini" => fetch_gemini_models(api_key).await,
        "cerebras" => fetch_openai_compatible_models("https://api.cerebras.ai/v1", api_key).await,
        "nvidia" => {
            fetch_openai_compatible_models("https://integrate.api.nvidia.com/v1", api_key).await
        }
        "minimax" => fetch_openai_compatible_models("https://api.minimax.io/v1", api_key).await,
        "moonshot" => fetch_openai_compatible_models("https://api.moonshot.ai/v1", api_key).await,
        "fireworksai" | "fireworks" => {
            fetch_openai_compatible_models("https://api.fireworks.ai/inference/v1", api_key).await
        }
        "openrouter" => {
            fetch_openai_compatible_models("https://openrouter.ai/api/v1", api_key).await
        }
        "ollama" => fetch_ollama_models(base_url).await,
        "custom" => {
            let url = base_url.ok_or("Custom provider requires a base URL")?;
            let key = if api_key.is_empty() {
                None
            } else {
                Some(api_key)
            };
            probe_openai_compatible_endpoint(url, key, true).await
        }
        _ => Err(format!("Unknown or unsupported provider: {}", provider)),
    }?;

    //
    // Sort models alphabetically before returning.
    //
    models.sort();
    Ok(models)
}
