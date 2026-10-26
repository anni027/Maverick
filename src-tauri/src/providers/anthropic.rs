//! Native Anthropic provider — drives the real `xai-grok-sampler` actor with
//! the Messages backend (`/v1/messages`, Anthropic-shaped).

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

use xai_grok_sampler::{RetryPolicy, SamplerActor, SamplerConfig, SamplerHandle, SamplingEvent};
use xai_grok_sampling_types::{ConversationRequest, ConversationResponse};

use super::provider::{DeltaPump, Provider, ProviderCapabilities, ProviderKind};

pub struct AnthropicProvider {
    handle: SamplerHandle,
    model: String,
    pump: DeltaPump,
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
        let (event_tx, event_rx) = mpsc::unbounded_channel::<SamplingEvent>();
        let handle = SamplerActor::spawn(config, RetryPolicy::default(), event_tx);
        Self {
            handle,
            model,
            pump: DeltaPump::attach(event_rx),
        }
    }

    async fn run_collect(
        &self,
        mut request: ConversationRequest,
        delta_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<ConversationResponse> {
        request.model = Some(self.model.clone());
        let request_id = xai_grok_sampler::types::RequestId::random();
        // Drop guard: unregisters on return AND on cancel-drop of this future.
        let _pump_guard = delta_tx.as_ref().map(|tx| {
            super::provider::PumpGuard::register(&self.pump, request_id.as_str(), tx.clone())
        });
        let collected = self
            .handle
            .submit_and_collect_with_metadata(request_id.clone(), request)
            .await;
        let (response, _stats) = collected
            .result
            .map_err(|e| anyhow::anyhow!("anthropic sampling failed: {e}"))?;
        Ok(response)
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

    async fn complete(&self, request: ConversationRequest) -> Result<ConversationResponse> {
        self.run_collect(request, None).await
    }

    async fn complete_streaming(
        &self,
        request: ConversationRequest,
        delta_tx: mpsc::UnboundedSender<String>,
    ) -> Result<ConversationResponse> {
        self.run_collect(request, Some(delta_tx)).await
    }
}
