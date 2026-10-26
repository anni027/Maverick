//! Unified provider-plugin system.
//!
//! Every backend implements [`Provider`] and the agent loop depends only on
//! that trait, so backends are interchangeable.
//!
//! The wire format is the `xai-grok-sampling-types` vocabulary:
//! `ConversationRequest` in, `ConversationResponse` (assistant text +
//! [`ToolCall`](xai_grok_sampling_types::ToolCall)) out.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::mpsc;
use xai_grok_sampler::{SamplingChannel, SamplingEvent};
use xai_grok_sampling_types::{ConversationRequest, ConversationResponse};

/// Stable identity of a backend.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
}

/// How a provider is configured.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProviderConfig {
    Http {
        #[serde(rename = "baseUrl")]
        base_url: String,
        #[serde(rename = "apiKey", skip_serializing)]
        api_key: Option<String>,
    },
}

/// What a provider can do.
#[derive(Clone, Debug, Default)]
pub struct ProviderCapabilities {
    pub supports_native_schema: bool,
    pub supports_tools: bool,
    pub supports_reasoning_effort: bool,
}

/// The contract every backend implements. Reuses the `xai-grok-sampling-types`
/// wire format as the stable input/output vocabulary.
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn kind(&self) -> ProviderKind;
    fn model(&self) -> &str {
        ""
    }
    fn capabilities(&self) -> ProviderCapabilities;
    /// Native context window in tokens (§5.6-E). Used for compaction
    /// thresholds when the chat handle has no live sampling config yet.
    fn context_window(&self) -> u64 {
        128_000
    }
    async fn complete(&self, request: ConversationRequest) -> Result<ConversationResponse>;

    /// Like [`complete`](Self::complete), but forwards assistant text deltas
    /// to `delta_tx` as they arrive. The default discards them and falls back
    /// to `complete` — background callers (compaction, extraction) are fine
    /// without streaming; the main turn loop uses this for live token output.
    async fn complete_streaming(
        &self,
        request: ConversationRequest,
        delta_tx: mpsc::UnboundedSender<String>,
    ) -> Result<ConversationResponse> {
        drop(delta_tx);
        self.complete(request).await
    }
}

/// Poison-tolerant lock: a panic in an unrelated task must not disable
/// streaming for every request that comes after it.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Forwards assistant-text deltas from a sampler's event rail to the
/// per-request receivers registered by the streaming providers.
pub(crate) struct DeltaPump {
    sinks: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
}

impl DeltaPump {
    /// Consume `rx` on a spawned task. Without a current tokio runtime (a
    /// provider built in a plain `#[test]`) the receiver is dropped instead:
    /// the sampler's `let _ =` sends then fail like the discard rail they
    /// replaced, costing only streaming in contexts that could not drive it.
    pub(crate) fn attach(mut rx: mpsc::UnboundedReceiver<SamplingEvent>) -> Self {
        let sinks: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        if tokio::runtime::Handle::try_current().is_ok() {
            let task_sinks = sinks.clone();
            tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    if let SamplingEvent::ChannelToken {
                        request_id,
                        channel: SamplingChannel::Text,
                        text,
                        ..
                    } = event
                    {
                        let _ = lock(&task_sinks)
                            .get(request_id.as_str())
                            .map(|tx| tx.send(text));
                    }
                }
            });
        }
        Self { sinks }
    }

    pub(crate) fn register(&self, request_id: &str, tx: mpsc::UnboundedSender<String>) {
        lock(&self.sinks).insert(request_id.to_string(), tx);
    }

    pub(crate) fn unregister(&self, request_id: &str) {
        lock(&self.sinks).remove(request_id);
    }
}

/// RAII guard: unregisters a pump sink when `run_collect` returns *or* when
/// its future is dropped (user cancel drops the provider future mid-await,
/// which used to skip the manual `unregister` and leak the entry forever).
pub(crate) struct PumpGuard<'a> {
    pump: &'a DeltaPump,
    request_id: String,
}

impl<'a> PumpGuard<'a> {
    pub(crate) fn register(pump: &'a DeltaPump, request_id: &str, tx: mpsc::UnboundedSender<String>) -> Self {
        pump.register(request_id, tx);
        Self {
            pump,
            request_id: request_id.to_string(),
        }
    }
}

impl Drop for PumpGuard<'_> {
    fn drop(&mut self) {
        self.pump.unregister(&self.request_id);
    }
}

/// A registered provider plus its config + display info.
#[derive(Clone)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub provider: Arc<dyn Provider>,
    pub config: ProviderConfig,
}

/// DTO for serialization to the frontend — `ProviderInfo` contains `Arc<dyn Provider>`
/// which is not serializable.
#[derive(Clone, Debug, Serialize)]
pub struct ProviderInfoDto {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub config: ProviderConfig,
    /// Effective model slug for this provider (e.g. `grok-4`, `gpt-4o`).
    /// Used by the UI to label the model selector and to show the active model.
    pub model: String,
    /// Provider kind accepts a `reasoning_effort` request field.
    pub supports_reasoning_effort: bool,
}

impl From<&ProviderInfo> for ProviderInfoDto {
    fn from(info: &ProviderInfo) -> Self {
        Self {
            id: info.id.clone(),
            name: info.name.clone(),
            kind: info.kind,
            config: info.config.clone(),
            model: info.provider.model().to_string(),
            supports_reasoning_effort: info.provider.capabilities().supports_reasoning_effort,
        }
    }
}
