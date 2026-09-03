//! A stand-in provider for Phase 1: emits a bash tool call on the first turn,
//! then a plain text answer on the second, so the agent loop exercises the real
//! tool runtime end-to-end and terminates cleanly — without network access or
//! API keys.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use xai_grok_sampling_types::{
    AssistantItem, ConversationItem, ConversationRequest, ConversationResponse, ToolCall,
};

use super::provider::{Provider, ProviderCapabilities, ProviderKind};

/// Mock provider for Phase 1 — per-instance turn counter so parallel
/// sessions and tests don't bleed.
pub struct MockProvider {
    turn: AtomicU32,
}

impl MockProvider {
    pub fn new() -> Self {
        Self {
            turn: AtomicU32::new(0),
        }
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn id(&self) -> &str {
        "mock"
    }

    fn name(&self) -> &str {
        "Mock"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Subprocess
    }

    fn model(&self) -> &str {
        "mock"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tools: true,
            supports_native_schema: true,
            ..Default::default()
        }
    }

    async fn complete(&self, _request: ConversationRequest) -> Result<ConversationResponse> {
        let turn = self.turn.fetch_add(1, Ordering::SeqCst);

        // First turn: ask the agent to run a shell command.
        if turn == 0 {
            let tool_calls = vec![ToolCall {
                id: Arc::from("call_1"),
                name: "run_terminal_cmd".to_string(),
                arguments: Arc::from(
                    r#"{"command":"echo hello from mock provider","description":"introduce myself"}"#,
                ),
            }];

            let assistant = AssistantItem {
                content: Arc::from("Let me run a command to introduce myself."),
                tool_calls,
                model_id: Some("mock-model".to_string()),
                model_fingerprint: None,
                reasoning_effort: None,
            };

            return Ok(ConversationResponse {
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
            });
        }

        // Second turn: test file tools by writing a temp file then reading it.
        if turn == 1 {
            let tool_calls = vec![
                ToolCall {
                    id: Arc::from("call_2a"),
                    name: "search_replace".to_string(),
                    arguments: Arc::from(
                        r#"{"file_path":"./nexus_test.txt","old_string":"","new_string":"Hello from Nexus!\n"}"#,
                    ),
                },
                ToolCall {
                    id: Arc::from("call_2b"),
                    name: "read_file".to_string(),
                    arguments: Arc::from(r#"{"target_file":"./nexus_test.txt"}"#),
                },
            ];

            let assistant = AssistantItem {
                content: Arc::from("Let me create and read a test file."),
                tool_calls,
                model_id: Some("mock-model".to_string()),
                model_fingerprint: None,
                reasoning_effort: None,
            };

            return Ok(ConversationResponse {
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
            });
        }

        // Third turn: final response
        let assistant = AssistantItem {
            content: Arc::from(
                "Hi! I'm Nexus — the chat-first automation agent. I ran `echo hello from mock provider`, created a file, and read it back successfully.",
            ),
            tool_calls: vec![],
            model_id: Some("mock-model".to_string()),
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
