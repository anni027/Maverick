//! UI-agnostic event stream emitted by the [`crate::AgentLoop`].
//!
//! The agent loop never talks to the UI directly; it pushes [`AgentEvent`]s
//! to an [`AgentEventSink`]. In Phase 6 the Tauri layer implements a sink that
//! forwards these over `app.emit(...)` to the webview.

use async_trait::async_trait;
use serde::Serialize;

/// Events produced while running a turn. The frontend renders these as
/// streamed text + tool-call cards.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    TurnStarted { turn: u32 },
    AssistantText { text: String },
    ToolCallStarted { name: String, args: String },
    ToolCallCompleted { name: String, output: String },
    TurnCompleted,
    Error { message: String },
}

/// Anything that can receive [`AgentEvent`]s. Implemented by the demo printer,
/// tests, and (later) the Tauri event forwarder.
#[async_trait]
pub trait AgentEventSink: Send + Sync {
    async fn on_event(&self, event: AgentEvent);
}

/// Prints events to stdout. Used by the headless demo and tests.
pub struct PrintSink;

#[async_trait]
impl AgentEventSink for PrintSink {
    async fn on_event(&self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStarted { turn } => println!("[turn {turn}] started"),
            AgentEvent::AssistantText { text } => println!("assistant: {text}"),
            AgentEvent::ToolCallStarted { name, args } => println!("tool> {name} {args}"),
            AgentEvent::ToolCallCompleted { name, output } => {
                println!("tool< {name}: {output}")
            }
            AgentEvent::TurnCompleted => println!("[turn completed]"),
            AgentEvent::Error { message } => println!("error: {message}"),
        }
    }
}

pub mod tauri_sink;
pub use tauri_sink::TauriSink;
