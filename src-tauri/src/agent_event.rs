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
    TurnStarted {
        turn: u32,
    },
    /// Emitted just before the provider is sampled for `turn`. The frontend
    /// opens a collapsible "Thought for N seconds" block and appends the
    /// [`AgentEvent::ThinkingStep`]s that follow until the turn resolves.
    ThinkingStarted {
        turn: u32,
    },
    /// One line of the visible reasoning timeline. The loop emits these as it
    /// moves through the turn (build request → sample → parse calls), so the
    /// UI always has something to show even when the provider does not stream
    /// a native reasoning channel.
    ThinkingStep {
        turn: u32,
        text: String,
    },
    AssistantText {
        text: String,
    },
    ToolCallStarted {
        name: String,
        args: String,
    },
    ToolCallCompleted {
        name: String,
        output: String,
    },
    TurnCompleted,
    /// Emitted once per `send_user_message` when the turn budget is nearly
    /// exhausted (`remaining` = turns left before the forced wrap-up). The
    /// frontend may ignore this; it is informational.
    BudgetWarning {
        remaining: u32,
    },
    /// Emitted by the segmented runner when an auto-continued segment starts
    /// (`segment` ≥ 2). `max_segments` is the runner's hard cap. Informational.
    SegmentBoundary {
        segment: u32,
        max_segments: u32,
    },
    /// Per-turn token telemetry (§5.6-B): emitted after every provider turn
    /// that reports usage. Cumulative totals live in the `get_usage` snapshot.
    UsageUpdated {
        segment: u32,
        turn: u32,
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
        cost_usd: f64,
    },
    /// Once per run when accumulated tokens cross 80% of the configured
    /// token budget (`max_segments × max_turns × avg_tokens_per_turn`).
    SpendWarning {
        percent_used: u32,
        total_tokens: u64,
        budget_tokens: u64,
    },
    /// The run was cancelled via `cancel_message` before completing.
    Cancelled {
        segment: u32,
    },
    /// The per-message spend cap tripped at a segment boundary: the run
    /// stopped gracefully (not an error bail). Mid-segment trips still bail
    /// as a backstop (see `run_segment`).
    SpendCapReached {
        spent_usd: f64,
        cap_usd: f64,
    },
    Error {
        message: String,
    },
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
            AgentEvent::ThinkingStarted { turn } => println!("[thinking] turn {turn} started"),
            AgentEvent::ThinkingStep { turn, text } => {
                println!("[thinking] turn {turn}: {text}")
            }
            AgentEvent::AssistantText { text } => println!("assistant: {text}"),
            AgentEvent::ToolCallStarted { name, args } => println!("tool> {name} {args}"),
            AgentEvent::ToolCallCompleted { name, output } => {
                println!("tool< {name}: {output}")
            }
            AgentEvent::TurnCompleted => println!("[turn completed]"),
            AgentEvent::BudgetWarning { remaining } => {
                println!("[budget] {remaining} turns remaining before wrap-up")
            }
            AgentEvent::SegmentBoundary {
                segment,
                max_segments,
            } => println!("[segment] starting {segment}/{max_segments}"),
            AgentEvent::UsageUpdated {
                segment,
                turn,
                total_tokens,
                cost_usd,
                ..
            } => println!(
                "[usage] seg {segment} turn {turn}: {total_tokens} tokens (${cost_usd:.4})"
            ),
            AgentEvent::SpendWarning {
                percent_used,
                total_tokens,
                budget_tokens,
            } => println!(
                "[spend] {percent_used}% of token budget used ({total_tokens}/{budget_tokens})"
            ),
            AgentEvent::Cancelled { segment } => println!("[cancelled] segment {segment}"),
            AgentEvent::SpendCapReached { spent_usd, cap_usd } => println!(
                "[spend-cap] ${spent_usd:.4} used (cap ${cap_usd:.4}); stopping gracefully"
            ),
            AgentEvent::Error { message } => println!("error: {message}"),
        }
    }
}

pub mod tauri_sink;
pub use tauri_sink::TauriSink;
