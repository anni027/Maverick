//! Native Anthropic provider — drives the real `xai-grok-sampler` actor with
//! the Messages backend (`/v1/messages`, Anthropic-shaped).

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

use xai_grok_sampler::{RetryPolicy, SamplerActor, SamplerConfig, SamplerHandle, SamplingEvent};
use xai_grok_sampling_types::{ConversationRequest, ConversationResponse};

use super::provider::{Provider, ProviderCapabilities, ProviderConfig, ProviderKind};

pub struct AnthropicProvider {
    handle: SamplerHandle,
    model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: Option<String>, base_url: String, model: String) -> Self {
        let config = SamplerConfig {
            api_key,
            base_url,
            model: model.clone(),
            api_backend: xai_grok_sampler::ApiBackend::Messages,
            context_window: 200_000,
            stream_tool_calls: true,
            force_http1: false,
            ..Default::default()
        };
        let (event_tx, _event_rx) = mpsc::unbounded_channel::<SamplingEvent>();
        let handle = SamplerActor::spawn(config, RetryPolicy::default(), event_tx);
        Self { handle, model }
    }

    pub fn from_config(config: &ProviderConfig, model: String) -> Self {
        match config {
            ProviderConfig::Http { base_url, api_key } => {
                Self::new(api_key.clone(), base_url.clone(), model)
            }
            ProviderConfig::Subprocess { .. } => {
                Self::new(None, "https://api.anthropic.com/v1".to_string(), model)
            }
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn name(&self) -> &str {
        "Anthropic"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn context_window(&self) -> u64 {
        200_000
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_native_schema: true,
            supports_tools: true,
            supports_reasoning_effort: true,
            ..Default::default()
        }
    }

    async fn complete(&self, mut request: ConversationRequest) -> Result<ConversationResponse> {
        request.model = Some(self.model.clone());
        let request_id = xai_grok_sampler::types::RequestId::random();
        let collected = self
            .handle
            .submit_and_collect_with_metadata(request_id, request)
            .await;
        let (response, _stats) = collected
            .result
            .map_err(|e| anyhow::anyhow!("anthropic sampling failed: {e}"))?;
        Ok(response)
    }
}
