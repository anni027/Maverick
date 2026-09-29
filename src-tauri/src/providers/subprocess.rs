//! Subprocess provider — drives an external agent runtime (Kilo Code /
//! OpenCode / …) as a `Provider`.
//!
//! The provider spawns the CLI once, parses its stdout framing, and maps each
//! emitted fragment onto the same `SamplingEvent` / `ConversationResponse`
//! wire format the native HTTP providers use. The agent loop never knows the
//! difference: a subprocess provider looks identical to `OpenAiProvider`.
//!
//! Phase 2 ships a subprocess provider that spawns an external CLI
//! (Kilo Code / OpenCode) and maps its output to the ConversationResponse
//! wire format. The template here runs a command and emits stdout as the reply.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::process::Command;

use xai_grok_sampling_types::{
    AssistantItem, ConversationItem, ConversationRequest, ConversationResponse,
};

use super::provider::{Provider, ProviderCapabilities, ProviderConfig, ProviderKind};

/// A minimal subprocess provider that runs a shell command and reports its
/// stdout as the assistant's reply. This is the template for real external
/// agents (Kilo Code / OpenCode).
pub struct SubprocessProvider {
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

impl SubprocessProvider {
    pub fn from_config(config: &ProviderConfig) -> Option<Self> {
        match config {
            ProviderConfig::Subprocess { command, args, env } => Some(Self {
                command: command.clone(),
                args: args.clone(),
                env: env.clone(),
            }),
            _ => None,
        }
    }
}

#[async_trait]
impl Provider for SubprocessProvider {
    fn id(&self) -> &str {
        "subprocess"
    }

    fn name(&self) -> &str {
        "Subprocess"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Subprocess
    }

    fn model(&self) -> &str {
        &self.command
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tools: true,
            ..Default::default()
        }
    }

    async fn complete(&self, _request: ConversationRequest) -> Result<ConversationResponse> {
        let mut cmd = Command::new(&self.command);
        for arg in &self.args {
            cmd.arg(arg);
        }
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn `{}`: {e}", self.command))?;
        let output = child
            .wait_with_output()
            .await
            .map_err(|e| anyhow::anyhow!("subprocess `{}` failed: {e}", self.command))?;

        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let text = if text.is_empty() {
            "(no output)".to_string()
        } else {
            text
        };

        let assistant = AssistantItem {
            content: Arc::from(text),
            tool_calls: vec![],
            model_id: Some(self.command.clone()),
            model_fingerprint: None,
            reasoning_effort: None,
        };

        Ok(ConversationResponse {
            items: vec![ConversationItem::Assistant(assistant)],
            stop_reason: None,
            usage: None,
            cost_usd_ticks: None,
            message_chunks_emitted: 0,
            doom_loop_signals: vec![],
            stop_message: None,
            message_id: None,
            raw_stop_reason: None,
            stop_sequence: None,
        })
    }
}
