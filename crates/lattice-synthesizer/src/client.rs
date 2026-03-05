//! LLM client for AI-driven code synthesis.
//!
//! Defines the [`LlmProvider`] trait for generating code from prompts,
//! and [`LlmClient`] which implements it via the Anthropic Claude API.

use crate::error::SynthesisError;

/// Trait for LLM-based code generation, abstracted for testability.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generate code from a structured prompt.
    async fn generate(&self, prompt: &str) -> Result<String, SynthesisError>;
}

/// Blanket implementation so `Arc<P>` can be used as a provider.
#[async_trait::async_trait]
impl<P: LlmProvider> LlmProvider for std::sync::Arc<P> {
    async fn generate(&self, prompt: &str) -> Result<String, SynthesisError> {
        (**self).generate(prompt).await
    }
}

/// HTTP client for the Anthropic Claude API.
pub struct LlmClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl LlmClient {
    /// Create a new client, reading `ANTHROPIC_API_KEY` from the environment.
    pub fn new() -> Result<Self, SynthesisError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| SynthesisError::ApiError("ANTHROPIC_API_KEY not set".into()))?;
        Ok(Self {
            api_key,
            model: "claude-sonnet-4-20250514".into(),
            client: reqwest::Client::new(),
        })
    }

    /// Override the default model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

#[async_trait::async_trait]
impl LlmProvider for LlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, SynthesisError> {
        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": self.model,
                "max_tokens": 4096,
                "messages": [{
                    "role": "user",
                    "content": prompt
                }]
            }))
            .send()
            .await
            .map_err(|e| SynthesisError::ApiError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SynthesisError::ApiError(format!("{status}: {body}")));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| SynthesisError::ApiError(e.to_string()))?;

        body["content"][0]["text"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| SynthesisError::ApiError("unexpected response format".into()))
    }
}
