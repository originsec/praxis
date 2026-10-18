use std::pin::Pin;

use super::wire::{self, Format};
use crate::ai::{ChatCompletionDelta, ChatCompletionRequest, ChatCompletionResponse};
use anyhow::Result;
use futures_core::Stream;

const API_URL: &str = "https://api.openai.com/v1/chat/completions";

pub struct OpenAIClient {
    api_key: String,
    base_url: String,
    http_client: reqwest::Client,
}

impl OpenAIClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: API_URL.into(),
            http_client: reqwest::Client::new(),
        }
    }

    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        let base_url = base_url.trim_end_matches('/');
        let url = if base_url.ends_with("/chat/completions") {
            base_url.to_string()
        } else {
            format!("{base_url}/chat/completions")
        };
        Self {
            api_key,
            base_url: url,
            http_client: reqwest::Client::new(),
        }
    }

    fn request(&self, request: &ChatCompletionRequest, streaming: bool) -> reqwest::RequestBuilder {
        let body = wire::request_body(request, Format::OpenAI, streaming);
        tracing::debug!(target: "common::ai::provider", provider = "OpenAI", body = %body, "AI request");
        self.http_client
            .post(&self.base_url)
            .bearer_auth(&self.api_key)
            .json(&body)
    }

    pub async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        let response = wire::checked(self.request(&request, false).send().await?).await?;
        let value = response.json().await?;
        wire::parse_response(value, &request.model, Format::OpenAI)
    }

    pub fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<ChatCompletionDelta>> + Send + '_>> {
        Box::pin(async_stream::try_stream! {
            let response = wire::checked(self.request(&request, true).send().await?).await?;
            for await delta in wire::stream(response, Format::OpenAI) {
                yield delta?;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_urls_keep_chat_completions_endpoint() {
        for base in [
            "https://example.invalid/v1",
            "https://example.invalid/v1/",
            "https://example.invalid/v1/chat/completions",
            "https://example.invalid/v1/chat/completions/",
        ] {
            let client = OpenAIClient::with_base_url("test".into(), base.into());
            assert_eq!(
                client.base_url,
                "https://example.invalid/v1/chat/completions"
            );
        }
    }
}
