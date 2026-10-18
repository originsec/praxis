use std::pin::Pin;

use super::wire::{self, Format};
use crate::ai::{ChatCompletionDelta, ChatCompletionRequest, ChatCompletionResponse};
use anyhow::Result;
use futures_core::Stream;

const API_URL: &str = "https://api.anthropic.com/v1/messages";

pub struct AnthropicClient {
    api_key: String,
    base_url: String,
    http_client: reqwest::Client,
}

impl AnthropicClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: API_URL.into(),
            http_client: reqwest::Client::new(),
        }
    }

    fn request(&self, request: &ChatCompletionRequest, streaming: bool) -> reqwest::RequestBuilder {
        let body = wire::request_body(request, Format::Anthropic, streaming);
        tracing::debug!(target: "common::ai::provider", provider = "Anthropic", body = %body, "AI request");
        self.http_client
            .post(&self.base_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
    }

    pub async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        let response = wire::checked(self.request(&request, false).send().await?).await?;
        let value = response.json().await?;
        wire::parse_response(value, &request.model, Format::Anthropic)
    }

    pub fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<ChatCompletionDelta>> + Send + '_>> {
        Box::pin(async_stream::try_stream! {
            let response = wire::checked(self.request(&request, true).send().await?).await?;
            for await delta in wire::stream(response, Format::Anthropic) {
                yield delta?;
            }
        })
    }
}
