//! Unified provider-plugin system.
//!
//! Every backend — a native LLM (OpenAI / Anthropic) or an external
//! agent runtime (Kilo Code / OpenCode invoked as a subprocess or over MCP) —
//! implements [`Provider`]. The agent loop only depends on this trait, so
//! backends are fully interchangeable.
//!
//! The wire format is the `xai-grok-sampling-types` vocabulary:
//! `ConversationRequest` in, `ConversationResponse` (assistant text +
//! [`ToolCall`](xai_grok_sampling_types::ToolCall)) out. That vocabulary is
//! deliberately backend-agnostic, so a subprocess provider that parses a CLI's
//! stdout and a native HTTP provider that streams SSE both satisfy the same
//! contract.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use xai_grok_sampling_types::{ConversationRequest, ConversationResponse};

/// Stable identity of a backend.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    /// External agent runtimes (Kilo Code, OpenCode, …) invoked as subprocesses.
    Subprocess,
    /// External agent runtimes exposing tools via MCP.
    Mcp,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Subprocess => "subprocess",
            ProviderKind::Mcp => "mcp",
        }
    }
}

/// How a provider is configured. Phase 2 supports native HTTP providers and
/// external agent runtimes spawned as subprocesses.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProviderConfig {
    Http {
        #[serde(rename = "baseUrl")]
        base_url: String,
        #[serde(rename = "apiKey", skip_serializing)]
        api_key: Option<String>,
    },
    Subprocess {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
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
