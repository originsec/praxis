use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct GenerateResponse {
    pub payload: String,
    pub transform_used: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct JudgeResponse {
    pub success: bool,
    pub confidence: f64,
    pub reasoning: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Serialize)]
struct GenerateRequest<'a> {
    goal: &'a str,
    transform: &'a str,
    intensity: u8,
}

#[derive(Serialize)]
struct JudgeRequest<'a> {
    goal: &'a str,
    payload: &'a str,
    response: &'a str,
}

pub struct LlmMapClient {
    client: reqwest::Client,
    base_url: String,
}

impl LlmMapClient {
    pub fn new(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn generate(
        &self,
        goal: &str,
        transform: &str,
        intensity: u8,
    ) -> Result<GenerateResponse> {
        let url = format!("{}/api/generate", self.base_url);
        let body = GenerateRequest { goal, transform, intensity };

        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("LLMMap generate request failed ({}): {}", url, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "LLMMap generate returned {}: {}",
                status, body
            ));
        }

        resp.json::<GenerateResponse>()
            .await
            .map_err(|e| anyhow!("Failed to parse LLMMap generate response: {}", e))
    }

    pub async fn judge(
        &self,
        goal: &str,
        payload: &str,
        response: &str,
    ) -> Result<JudgeResponse> {
        let url = format!("{}/api/judge", self.base_url);
        let body = JudgeRequest { goal, payload, response };

        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("LLMMap judge request failed ({}): {}", url, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "LLMMap judge returned {}: {}",
                status, body
            ));
        }

        resp.json::<JudgeResponse>()
            .await
            .map_err(|e| anyhow!("Failed to parse LLMMap judge response: {}", e))
    }
}
