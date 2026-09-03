//! The provider-agnostic agent loop — the heart of the slice.
//!
//! It is intentionally small and free of any ACP / transport code. It composes
//! three vendored, ACP-free building blocks:
//!   * [`xai_chat_state::ChatStateHandle`] — the conversation store
//!   * [`xai_grok_tools::bridge::ToolBridge`] — the tool runtime / dispatcher
//!   * [`crate::providers::Provider`] — a pluggable model/agent backend
//!
//! The loop: push the user message -> build a `ConversationRequest` with the
//! current tool definitions -> ask the provider for a `ConversationResponse`
//! -> record the assistant turn -> if it emitted tool calls, execute each via
//! the `ToolBridge` and feed the results back -> repeat until the provider
//! returns no tool calls.

use std::sync::Arc;

use anyhow::Result;
use xai_chat_state::ChatStateHandle;
use xai_grok_sampling_types::{ConversationItem, ToolSpec};
use xai_grok_tools::bridge::ToolBridge;

use tokio::sync::RwLock;

use crate::agent_event::{AgentEvent, AgentEventSink};
use crate::providers::Provider;

pub struct AgentLoop {
    chat: ChatStateHandle,
    tools: ToolBridge,
    provider: RwLock<Arc<dyn Provider>>,
}

impl AgentLoop {
    pub fn new(chat: ChatStateHandle, tools: ToolBridge, provider: Arc<dyn Provider>) -> Self {
        Self {
            chat,
            tools,
            provider: RwLock::new(provider),
        }
    }

    pub async fn set_provider(&self, provider: Arc<dyn Provider>) {
        *self.provider.write().await = provider;
    }

    pub async fn provider(&self) -> Arc<dyn Provider> {
        self.provider.read().await.clone()
    }

    /// Send a user message and run the agent until it stops or hits the turn
    /// cap. Events are streamed to `sink`.
    pub async fn send_user_message(
        &self,
        text: &str,
        sink: Arc<dyn AgentEventSink>,
    ) -> Result<()> {
        self.chat.push_user_message(ConversationItem::user(text));

        let mut turn: u32 = 0;
        loop {
            turn += 1;
            if turn > 16 {
                anyhow::bail!("agent exceeded the maximum number of turns ({turn})");
            }

            let defs = self.tools.tool_definitions().await;
            let mut specs: Vec<ToolSpec> = defs.into_iter().map(ToolSpec::from).collect();
            let Some(request) = self
                .chat
                .build_request(specs, None, false, None, "conv-1".to_string(), format!("req-{turn}"))
                .await
            else {
                anyhow::bail!("chat actor is dead");
            };

            sink.on_event(AgentEvent::TurnStarted { turn }).await;

            let provider = self.provider.read().await.clone();
            let response = match provider.complete(request).await {
                Ok(r) => r,
                Err(e) => {
                    sink.on_event(AgentEvent::Error {
                        message: e.to_string(),
                    })
                    .await;
                    anyhow::bail!(e);
                }
            };

            // Stream the assistant text to the UI.
            let text = response.assistant_text();
            if !text.is_empty() {
                sink.on_event(AgentEvent::AssistantText { text }).await;
            }

            // Record the assistant turn (trailing Assistant item).
            if let Some(assistant) = response.assistant().cloned() {
                self.chat
                    .push_assistant_response(ConversationItem::Assistant(assistant));
            }

            let calls = response.tool_calls().to_vec();
            if calls.is_empty() {
                break;
            }

            for call in calls {
                let name = call.name.clone();
                let id = call.id.to_string();
                sink.on_event(AgentEvent::ToolCallStarted {
                    name: name.clone(),
                    args: call.arguments.to_string(),
                })
                .await;

                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .unwrap_or(serde_json::Value::Null);

                let (prompt_text, _is_error) = match &*name {
                    "write_to_file" | "write_file" | "create_file" => {
                        let ws = crate::tools::resolve_workspace_dir();
                        match crate::tools::execute_write_to_file(&args, &ws) {
                            Ok(msg) => (msg, false),
                            Err(e) => (format!("Error executing write_to_file: {e}"), true),
                        }
                    }
                    _ => match self.tools.call(&name, args, &id).await {
                        Ok(res) => (res.prompt_text, false),
                        Err(e) => (format!("Error: tool `{name}` failed: {e}"), true),
                    },
                };

                sink.on_event(AgentEvent::ToolCallCompleted {
                    name: name.clone(),
                    output: prompt_text.clone(),
                })
                .await;

                self.chat.push_tool_result(ConversationItem::tool_result(
                    id,
                    prompt_text,
                ));
            }
        }

        sink.on_event(AgentEvent::TurnCompleted).await;
        Ok(())
    }
}
