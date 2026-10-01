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
//!
//! Turn budget (three states, replacing the old hard bail at turn 16):
//!   1. Normal turns — full tool set, up to the warning turn.
//!      `BudgetWarning` event is emitted so the UI can surface it.
//!   3. Wrap-up turn — no tools are offered (the model is forced into a text
//!      answer) and wrap-up instructions are injected. The loop then ends
//!      naturally when the response carries no tool calls.
//!
//!   A hard ceiling exists only as a last resort if a model emits tool calls
//!   despite being offered none.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use xai_chat_state::ChatStateHandle;
use xai_grok_sampling_types::{ConversationItem, ReasoningEffort, ToolSpec};
use xai_grok_tools::bridge::ToolBridge;
use xai_grok_tools::implementations::grok_build::todo::{TodoState, TodoStatus};
use xai_grok_tools::types::resources::State;

use crate::agent_event::{AgentEvent, AgentEventSink};
use crate::compaction::CompactPolicy;
use crate::config::BudgetConfig;
use crate::providers::Provider;
use crate::subagents::{SUBAGENT_MAX_DEPTH, SUBAGENT_TOOL_NAME};

// ─── Turn budget (warn → wrap-up → hard ceiling) ─────────────────────────────
//
// Defaults; the live values come from [`BudgetConfig`] (config file `[budget]`,
// same defaults) so real-run measurements (§5.6-A) can tune them without a
// rebuild. Loops without a config manager (unit tests) use these directly.
// `pub(crate)` so `config.rs` derives its serde defaults from the same source.
pub(crate) const MAX_TURNS: u32 = 40;

/// Injected as a user turn on the wrap-up turn. Terse on purpose — it burns
/// tokens from the same budget it is trying to save.
const WRAP_UP_PROMPT: &str = "WRAP-UP: the tool budget for this segment is exhausted. Summarize what you \
found or completed, update your todo list to reflect current status, and state \
clearly what is left to do next. Do not attempt further tool calls.";

/// Injected as a user turn at the warning turn. Terse on purpose.
const BUDGET_WARNING_PROMPT: &str = "Note: you have roughly 3 tool-call turns remaining before this segment's \
budget runs out. Start wrapping up or checkpoint your progress in todos now. \
If any long-running commands are still active, leave them in the background \
(do not kill them unless stray), record their task_ids in your todos, and \
wrap up this segment.";

/// Marker prepended to failed tool results so the model can distinguish
/// errors from successes without a schema change. Reversible: a proper
/// `ToolResultItem` flag can replace it later without touching the loop.
const TOOL_ERROR_PREFIX: &str = "[TOOL_ERROR] ";

/// Tool results larger than this are head+tail truncated before entering
/// conversation history (8–16 KB range), so a single chatty tool call cannot
/// bloat every subsequent request.
const MAX_TOOL_OUTPUT_BYTES: usize = 12_000;

/// Hard cap on auto-continued segments per [`AgentLoop::send_user_message_auto`]
/// call. Default; the live value comes from [`BudgetConfig`]. `pub(crate)` so
/// `config.rs` derives its serde default from the same source.
///
/// Applies only when no `[budget].spend_cap_usd` is set; with a cap the run is
/// unbounded (stops on natural finish, the identical-wrap-up guard, the cap,
/// or cancel).
pub(crate) const MAX_SEGMENTS: u32 = 3;

/// Absolute backstop on auto-continued segments when a spend cap is set but
/// the provider never reports cost (so the cap can never trip). Far above any
/// realistic task; simply ends the run instead of looping forever.
const ABSOLUTE_MAX_SEGMENTS: u32 = 500;

/// Injected as the user message that opens each auto-continued segment (2..).
/// Terse on purpose.
const CONTINUE_PROMPT: &str = "Your previous segment hit the tool budget and wrapped up. Continue with \
the next pending todo from your list. Do not redo completed work.";

/// Why a [`AgentLoop::send_user_message`] run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// `true` iff the run ended via the budget wrap-up turn (no tools offered)
    /// rather than the model stopping on its own. Only a budget stop may
    /// trigger an auto-continued segment.
    pub hit_budget: bool,
    /// The wrap-up turn's assistant text (the model's summary), when the run
    /// ended in a wrap-up. Two identical consecutive summaries signal a
    /// broken loop to the segmented runner.
    pub wrap_up_text: Option<String>,
}

/// Throwaway sink for subagent children (§5.6-D): collects assistant text so
/// the parent can fold it into a compact summary. Raw child output never
/// reaches the parent context — only
/// [`render_subagent_summary`](crate::subagents::render_subagent_summary) does.
#[derive(Default)]
struct SummarySink {
    events: std::sync::Mutex<Vec<AgentEvent>>,
}

#[async_trait::async_trait]
impl AgentEventSink for SummarySink {
    async fn on_event(&self, event: AgentEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }
}

impl SummarySink {
    /// Last assistant text the child produced, if any.
    fn last_text(&self) -> Option<String> {
        self.events.lock().ok()?.iter().rev().find_map(|e| match e {
            AgentEvent::AssistantText { text } => Some(text.clone()),
            _ => None,
        })
    }
}

/// Mirrors a child (subagent) run's progress events to the parent sink so the
/// UI shows activity instead of going silent for the whole delegation.
///
/// Only transcript-level activity is forwarded: turn/segment/usage/terminal
/// events would rewrite the parent's status bar with the *child's* numbers,
/// and `AssistantText` would leak raw child output into the parent stream. The
/// child's own [`SummarySink`] still sees every event, so `last_text()` keeps
/// working.
struct MirroredSink {
    summary: Arc<SummarySink>,
    parent: Arc<dyn AgentEventSink>,
}

#[async_trait::async_trait]
impl AgentEventSink for MirroredSink {
    async fn on_event(&self, event: AgentEvent) {
        let forward = matches!(
            event,
            AgentEvent::ThinkingStep { .. }
                | AgentEvent::ToolCallStarted { .. }
                | AgentEvent::ToolCallCompleted { .. }
        );
        if forward {
            self.parent.on_event(event.clone()).await;
        }
        self.summary.on_event(event).await;
    }
}

// ─── Output shaping helpers ─────────────────────────────────────────────────

/// Head+tail truncate `text` to at most `limit` bytes, preserving the
/// beginning and end of the output. Char-boundary safe: byte slices are
/// walked back/forward to the nearest `char` boundary so multi-byte UTF-8
/// never panics the slice.
fn truncate_output_with(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let half = limit / 2;
    let head_end = floor_char_boundary(text, half);
    let tail_start = ceil_char_boundary(text, text.len() - half);
    let omitted = tail_start - head_end;
    format!(
        "{}\n... [truncated {omitted} bytes] ...\n{}",
        &text[..head_end],
        &text[tail_start..]
    )
}

fn floor_char_boundary(s: &str, mut index: usize) -> usize {
    while index > 0 && !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(s: &str, mut index: usize) -> usize {
    while index < s.len() && !s.is_char_boundary(index) {
        index += 1;
    }
    index
}

/// Prepend [`TOOL_ERROR_PREFIX`] to failed tool results. Idempotent so a
/// nested error can never double the marker.
fn format_tool_result(text: String, is_error: bool) -> String {
    if is_error && !text.starts_with(TOOL_ERROR_PREFIX) {
        format!("{TOOL_ERROR_PREFIX}{text}")
    } else {
        text
    }
}

pub struct AgentLoop {
    chat: ChatStateHandle,
    tools: ToolBridge,
    provider: RwLock<Arc<dyn Provider>>,
    /// Optional config store for compaction policy, per-tool output budgets,
    /// and turn/segment budgets. `None` in unit tests (defaults apply).
    config_manager: Option<Arc<crate::config::ConfigManager>>,
    /// Per-task token/cost ledger (§5.6-B). Shared so the `get_usage`
    /// command can snapshot mid-run.
    usage: Arc<Mutex<crate::usage::UsageLedger>>,
    /// Cancellation for the in-flight run (§5.6-C). Replaced at the start of
    /// every [`Self::send_user_message_auto`] so a stale cancel never leaks
    /// into the next task. `std` mutex: never held across `.await`.
    cancel: std::sync::Mutex<CancellationToken>,
    /// Delegation depth (§5.6-D): 0 for user sessions, ≥ 1 for subagent
    /// children (which are not offered the `subagent` tool).
    subagent_depth: u32,
}

impl AgentLoop {
    pub fn new(chat: ChatStateHandle, tools: ToolBridge, provider: Arc<dyn Provider>) -> Self {
        Self {
            chat,
            tools,
            provider: RwLock::new(provider),
            config_manager: None,
            usage: Arc::new(Mutex::new(crate::usage::UsageLedger::new())),
            cancel: std::sync::Mutex::new(CancellationToken::new()),
            subagent_depth: 0,
        }
    }

    /// Attach the config store (compaction policy + per-tool output budgets
    /// + turn/segment budgets).
    pub fn with_config_manager(mut self, manager: Arc<crate::config::ConfigManager>) -> Self {
        self.config_manager = Some(manager);
        self
    }

    /// Mark this loop as a subagent child (depth ≥ 1): the `subagent` tool
    /// is withheld from its tool set.
    pub fn with_subagent_depth(mut self, depth: u32) -> Self {
        self.subagent_depth = depth;
        self
    }

    pub async fn set_provider(&self, provider: Arc<dyn Provider>) {
        *self.provider.write().await = provider;
    }

    pub async fn provider(&self) -> Arc<dyn Provider> {
        self.provider.read().await.clone()
    }

    /// Cancel the in-flight run, if any (§5.6-C). Sync and idempotent;
    /// cancelling with no active run is a no-op. The loop notices at the top
    /// of the next turn (and aborts a pending provider call) and emits
    /// `AgentEvent::Cancelled`.
    pub fn cancel_current_run(&self) {
        if let Ok(guard) = self.cancel.lock() {
            guard.cancel();
        }
    }

    /// Snapshot the per-task token/cost ledger (§5.6-B).
    pub async fn usage_snapshot(&self) -> crate::usage::UsageSnapshot {
        self.usage.lock().await.snapshot()
    }

    /// Align the session's sampling-config window with the active provider
    /// (§5.6-E). Call after a provider switch so compaction keeps tracking
    /// the real model window.
    pub async fn sync_context_window(&self) -> bool {
        let window = self.provider.read().await.context_window();
        crate::tools::sync_chat_context_window(&self.chat, window).await
    }

    /// Apply (or clear) the reasoning effort for future requests. `effort` is
    /// a lowercase tier name (`none`…`max`, with `extra` aliased to `xhigh`).
    /// Clears the stored effort whenever the provider kind cannot accept
    /// `reasoning_effort` or no value is given, so a switch to an unsupported
    /// provider never forwards a stale effort upstream.
    pub async fn set_reasoning_effort(&self, effort: Option<&str>) -> Result<(), String> {
        let supported = self.provider.read().await.capabilities().supports_reasoning_effort;
        let parsed = match effort.map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) if supported => {
                let normalized = if raw.eq_ignore_ascii_case("extra") { "xhigh" } else { raw };
                Some(normalized.parse::<ReasoningEffort>()?)
            }
            _ => None,
        };
        if let Some(mut config) = self.chat.get_sampling_config().await {
            config.reasoning_effort = parsed;
            self.chat.update_sampling_config(config);
        }
        Ok(())
    }

    /// Effective turn/segment budgets: live config when attached, the
    /// `MAX_TURNS` / `MAX_SEGMENTS` defaults otherwise.
    async fn budget(&self) -> BudgetConfig {
        match &self.config_manager {
            Some(manager) => manager.budget_config().await,
            None => BudgetConfig::default(),
        }
    }

    /// Effective context window (§5.6-E): the chat handle's live sampling
    /// config wins (it tracks provider switches via `sync_chat_context_window`),
    /// then the active provider's native window, then the 128k default.
    async fn effective_context_window(&self) -> u64 {
        if let Some(cfg) = self.chat.get_sampling_config().await {
            return cfg.context_window.get();
        }
        self.provider.read().await.context_window()
    }

    fn current_token(&self) -> CancellationToken {
        self.cancel
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| CancellationToken::new())
    }

    /// Replace the run token with a fresh one and return it. Called at the
    /// start of every segmented run so a previous cancel never leaks across
    /// tasks.
    fn reset_cancel_token(&self) -> CancellationToken {
        let token = CancellationToken::new();
        if let Ok(mut guard) = self.cancel.lock() {
            *guard = token.clone();
        }
        token
    }

    /// Tool specs offered to the model this turn: the bridge definitions,
    /// plus the `subagent` delegation tool on parent loops only (§5.6-D).
    async fn tool_specs(&self) -> Vec<ToolSpec> {
        let mut specs: Vec<ToolSpec> = self
            .tools
            .tool_definitions()
            .await
            .into_iter()
            .map(ToolSpec::from)
            .collect();
        if self.subagent_depth < SUBAGENT_MAX_DEPTH {
            specs.push(crate::subagents::subagent_tool_spec());
        }
        specs
    }

    /// Send a user message and run the agent until the model stops on its own
    /// or the turn budget runs out (warn → wrap-up → hard ceiling). Events are
    /// streamed to `sink`. Single-segment entry point; the segmented runner
    /// ([`Self::send_user_message_auto`]) calls [`Self::run_segment`] per
    /// segment.
    pub async fn send_user_message(
        &self,
        text: &str,
        sink: Arc<dyn AgentEventSink>,
    ) -> Result<RunOutcome> {
        let budget = self.budget().await;
        let token = self.current_token();
        self.run_segment(text, sink, 1, None, &budget, &token).await
    }

    /// One budgeted segment: at most `turn_cap.unwrap_or(max_turns)` tool
    /// turns, then a forced no-tools wrap-up. `segment` numbers the segment
    /// inside the current task (1-based) for usage/cancel events.
    async fn run_segment(
        &self,
        prompt: &str,
        sink: Arc<dyn AgentEventSink>,
        segment: u32,
        turn_cap: Option<u32>,
        budget: &BudgetConfig,
        token: &CancellationToken,
    ) -> Result<RunOutcome> {
        self.chat.push_user_message(ConversationItem::user(prompt));
        self.usage.lock().await.begin_segment(segment);

        // Turn budget: config-driven, with the subagent child cap overriding
        // the per-segment budget when present (already clamped to ≤ 10).
        let max_turns = turn_cap.unwrap_or_else(|| budget.effective_max_turns());
        let warning_at = max_turns.saturating_sub(3).max(1);
        let wrap_up_at = max_turns + 1;
        let hard_ceiling = max_turns + 2;
        let remaining_at_warn = max_turns.saturating_sub(warning_at);

        let mut turn: u32 = 0;
        let hit_budget;
        let mut wrap_up_text: Option<String> = None;
        loop {
            if token.is_cancelled() {
                sink.on_event(AgentEvent::Cancelled { segment }).await;
                anyhow::bail!("run cancelled by user");
            }

            turn += 1;

            if turn > hard_ceiling {
                anyhow::bail!("hard turn ceiling reached ({turn})");
            }

            // Wrap-up turn and beyond: offer no tools so the model is
            // forced into a text answer instead of another tool call.
            // `>=` (not `==`) so the hard-ceiling turn also withholds
            // tools; hallucinated calls on these turns are refused below
            // instead of being executed.
            let wrap_up = turn >= wrap_up_at;
            let specs: Vec<ToolSpec> = if wrap_up {
                Vec::new()
            } else {
                self.tool_specs().await
            };
            let spec_count = specs.len();

            if wrap_up {
                self.chat
                    .push_user_message(ConversationItem::user(WRAP_UP_PROMPT));
            } else if turn == warning_at {
                self.chat
                    .push_user_message(ConversationItem::user(BUDGET_WARNING_PROMPT));
                sink.on_event(AgentEvent::BudgetWarning {
                    remaining: remaining_at_warn,
                })
                .await;
            }

            let Some(request) = self
                .chat
                .build_request(
                    specs,
                    None,
                    false,
                    None,
                    "conv-1".to_string(),
                    format!("req-{turn}"),
                )
                .await
            else {
                anyhow::bail!("chat actor is dead");
            };

            sink.on_event(AgentEvent::TurnStarted { turn }).await;
            sink.on_event(AgentEvent::ThinkingStarted { turn }).await;
            sink.on_event(AgentEvent::ThinkingStep {
                turn,
                text: format!("building request for turn {turn} ({spec_count} tool(s) offered)"),
            })
            .await;

            let provider = self.provider.read().await.clone();
            let response = tokio::select! {
                r = provider.complete(request) => match r {
                    Ok(r) => r,
                    Err(e) => {
                        sink.on_event(AgentEvent::Error {
                            message: e.to_string(),
                        })
                        .await;
                        anyhow::bail!(e);
                    }
                },
                () = token.cancelled() => {
                    sink.on_event(AgentEvent::Cancelled { segment }).await;
                    anyhow::bail!("run cancelled by user");
                }
            };

            sink.on_event(AgentEvent::ThinkingStep {
                turn,
                text: format!("provider returned {} item(s)", response.items.len()),
            })
            .await;

            self.record_usage(segment, turn, &response, &sink, budget)
                .await;
            if self.spend_capped(budget).await {
                // Backstop: mid-segment trips still bail (the segment-boundary
                // check in `send_user_message_auto` stops gracefully instead).
                let total = self.usage.lock().await.cost_usd();
                let cap = budget.spend_cap_usd.unwrap_or(0.0);
                let message = format!(
                    "spend cap exceeded: ${total:.4} used (cap ${cap:.4}); stopping further segments"
                );
                sink.on_event(AgentEvent::SpendCapReached {
                    spent_usd: total,
                    cap_usd: cap,
                })
                .await;
                // Deliberately *not* an `Error` event: hitting the cap is a
                // graceful stop. Emitting both finalized the run twice and the
                // UI overwrote "Stopped at cap" with "Failed after …".
                anyhow::bail!(message);
            }

            // Stream the assistant text to the UI.
            let text = response.assistant_text();
            if !text.is_empty() {
                sink.on_event(AgentEvent::AssistantText { text: text.clone() })
                    .await;
            }

            // Record the assistant turn (trailing Assistant item).
            if let Some(assistant) = response.assistant().cloned() {
                self.chat
                    .push_assistant_response(ConversationItem::Assistant(assistant));
            }

            let calls = response.tool_calls().to_vec();
            if calls.is_empty() {
                // On the wrap-up turn no tools were offered, so this break is
                // the natural end of a budgeted segment.
                hit_budget = wrap_up;
                if wrap_up {
                    wrap_up_text = (!text.is_empty()).then_some(text);
                }
                break;
            }

            // Wrap-up hardening: no tools were offered this turn, so tool
            // calls emitted anyway must NOT be executed. Refuse each with an
            // error tool result and end the segment recoverably instead of
            // running into the hard-ceiling `bail!`.
            if wrap_up {
                for call in &calls {
                    let name = call.name.clone();
                    let id = call.id.to_string();
                    sink.on_event(AgentEvent::ToolCallStarted {
                        name: name.clone(),
                        args: call.arguments.to_string(),
                    })
                    .await;
                    let refused =
                        format!("{TOOL_ERROR_PREFIX}no tools available this turn; answer in text");
                    sink.on_event(AgentEvent::ToolCallCompleted {
                        name: name.clone(),
                        output: refused.clone(),
                    })
                    .await;
                    self.chat
                        .push_tool_result(ConversationItem::tool_result(id, refused));
                }
                hit_budget = true;
                wrap_up_text = (!text.is_empty()).then_some(text);
                break;
            }

            for (idx, call) in calls.iter().enumerate() {
                // The assistant message carrying *every* tool call in this batch was
                // already persisted, so refuse the remainder instead of bailing with
                // dangling calls (the same shape the wrap-up hardening avoids).
                if token.is_cancelled() {
                    for pending in &calls[idx..] {
                        let pending_name = pending.name.clone();
                        let refused =
                            format!("{TOOL_ERROR_PREFIX}cancelled by user before execution");
                        sink.on_event(AgentEvent::ToolCallStarted {
                            name: pending_name.clone(),
                            args: pending.arguments.to_string(),
                        })
                        .await;
                        sink.on_event(AgentEvent::ToolCallCompleted {
                            name: pending_name.clone(),
                            output: refused.clone(),
                        })
                        .await;
                        self.chat
                            .push_tool_result(ConversationItem::tool_result(
                                pending.id.to_string(),
                                refused,
                            ));
                    }
                    sink.on_event(AgentEvent::Cancelled { segment }).await;
                    anyhow::bail!("run cancelled by user");
                }

                let name = call.name.clone();
                let id = call.id.to_string();
                sink.on_event(AgentEvent::ThinkingStep {
                    turn,
                    text: format!("executing tool `{name}`"),
                })
                .await;
                sink.on_event(AgentEvent::ToolCallStarted {
                    name: name.clone(),
                    args: call.arguments.to_string(),
                })
                .await;

                // Arg safety: malformed JSON becomes an error tool result for
                // this call instead of silently executing with `Null` args.
                let (result_text, is_error) =
                    match serde_json::from_str::<serde_json::Value>(&call.arguments) {
                        Err(e) => (format!("invalid tool arguments for `{name}`: {e}"), true),
                        Ok(args) => {
                            self.dispatch_tool_call(&name, &args, &id, token, &sink)
                                .await
                        }
                    };

                // Truncate before history, then mark errors — the stored (and
                // re-sent) tool result is the shaped one, not just the render.
                // Per-tool budgets (Phase 3 tuning) only ever shrink below the
                // 12 KB default; unlisted tools use the default.
                let tool_budget = match &self.config_manager {
                    Some(manager) => manager
                        .read()
                        .await
                        .context
                        .tool_output_budgets
                        .get(&*name)
                        .copied()
                        .filter(|&b| b > 0 && b < MAX_TOOL_OUTPUT_BYTES)
                        .unwrap_or(MAX_TOOL_OUTPUT_BYTES),
                    None => MAX_TOOL_OUTPUT_BYTES,
                };
                let result_text =
                    format_tool_result(truncate_output_with(&result_text, tool_budget), is_error);

                sink.on_event(AgentEvent::ToolCallCompleted {
                    name: name.clone(),
                    output: result_text.clone(),
                })
                .await;

                self.chat
                    .push_tool_result(ConversationItem::tool_result(id, result_text));
            }

            // Mid-segment compaction: after each turn's tool results land,
            // check estimated context occupancy against the `CompactPolicy`
            // threshold and checkpoint when crossed. Runs between turns so it
            // can never race an in-flight provider call; a short history or
            // disabled policy simply skips.
            self.maybe_compact(None).await;
        }

        sink.on_event(AgentEvent::TurnCompleted).await;
        Ok(RunOutcome {
            hit_budget,
            wrap_up_text,
        })
    }

    /// Dispatch one parsed tool call. The `subagent` delegation tool (§5.6-D)
    /// and the workspace `write_to_file` shim are handled in-loop; everything
    /// else goes to the bridge.
    async fn dispatch_tool_call(
        &self,
        name: &str,
        args: &serde_json::Value,
        id: &str,
        token: &CancellationToken,
        sink: &Arc<dyn AgentEventSink>,
    ) -> (String, bool) {
        if name == SUBAGENT_TOOL_NAME {
            return self.dispatch_subagent(args, token, sink).await;
        }
        match name {
            "write_to_file" | "write_file" | "create_file" => {
                let ws = crate::tools::resolve_workspace_dir();
                match crate::tools::execute_write_to_file(args, &ws) {
                    Ok(msg) => (msg, false),
                    Err(e) => (format!("Error executing write_to_file: {e}"), true),
                }
            }
            _ => {
                // Race the tool against the cancel token: a long `run_terminal_cmd`
                // or `web_fetch` would otherwise keep the run alive after Stop and
                // the remaining calls in this batch would still execute.
                tokio::select! {
                    called = self.tools.call(name, args.clone(), id) => match called {
                        Ok(res) => (res.prompt_text, false),
                        Err(e) => (format!("Error: tool `{name}` failed: {e}"), true),
                    },
                    () = token.cancelled() => {
                        (format!("cancelled by user before `{name}` completed"), true)
                    }
                }
            }
        }
    }

    /// Run a `subagent` delegation: parse args, spawn a bounded child loop,
    /// and return its compact summary as the tool result.
    async fn dispatch_subagent(
        &self,
        args: &serde_json::Value,
        token: &CancellationToken,
        sink: &Arc<dyn AgentEventSink>,
    ) -> (String, bool) {
        if self.subagent_depth >= SUBAGENT_MAX_DEPTH {
            return (
                "Error: nested subagents are not allowed; solve this directly instead.".to_string(),
                true,
            );
        }
        let parsed = match crate::subagents::parse_subagent_args(args) {
            Ok(parsed) => parsed,
            Err(e) => return (format!("Error: invalid subagent arguments: {e}"), true),
        };
        match self
            .spawn_subagent(&parsed.goal, parsed.budget_turns, token, sink)
            .await
        {
            Ok(summary) => (summary, false),
            Err(e) => (format!("Error: subagent failed: {e}"), true),
        }
    }

    /// Spawn a child loop with a fresh in-memory context and a clamped turn
    /// budget, run one segment, and fold its findings into a compact summary.
    /// The parent sees only the returned blob — never raw child output.
    async fn spawn_subagent(
        &self,
        goal: &str,
        budget_turns: u32,
        token: &CancellationToken,
        sink: &Arc<dyn AgentEventSink>,
    ) -> Result<String> {
        use crate::subagents::{SubagentTask, render_subagent_summary};

        let task = SubagentTask::new(goal, budget_turns);
        let chat = crate::tools::build_chat_handle("subagent-child", None)?;
        let child = AgentLoop {
            chat,
            tools: self.tools.clone(),
            provider: RwLock::new(self.provider.read().await.clone()),
            config_manager: self.config_manager.clone(),
            usage: Arc::new(Mutex::new(crate::usage::UsageLedger::new())),
            cancel: std::sync::Mutex::new(token.clone()),
            subagent_depth: self.subagent_depth + 1,
        };
        // Child budget: its own turn cap, single segment, no auto-continue.
        // Raw child output stays out of the parent context — the summary below
        // is the only thing that flows back — but the child's tool/thinking
        // events are mirrored to the parent sink, otherwise the UI looks hung
        // for the whole subagent run. Boxed: the child segment can dispatch
        // back into `spawn_subagent`, so the future must not grow unboundedly
        // (E0733).
        let collecting = Arc::new(SummarySink::default());
        let mirror: Arc<dyn AgentEventSink> = Arc::new(MirroredSink {
            summary: Arc::clone(&collecting),
            parent: Arc::clone(sink),
        });
        let child_budget = child.budget().await;
        let outcome = Box::pin(child.run_segment(
            &task.goal,
            mirror,
            1,
            Some(task.budget_turns),
            &child_budget,
            token,
        ))
        .await;
        // Merge child usage into the parent ledger so subagent spend counts
        // toward the parent's cap and `get_usage`. The child ledger is
        // local to this call and its segment has finished, so holding both
        // guards briefly here cannot deadlock against the parent paths
        // (which never touch a child ledger).
        let outcome = outcome;
        {
            let child_ledger = child.usage.lock().await;
            let mut parent_ledger = self.usage.lock().await;
            parent_ledger.merge(&child_ledger);
        }
        let outcome = outcome?;
        let findings = outcome
            .wrap_up_text
            .or_else(|| collecting.last_text())
            .unwrap_or_else(|| "(no findings returned)".to_string());
        Ok(render_subagent_summary(&task, &findings))
    }

    /// Record one provider turn in the usage ledger (§5.6-B) and emit
    /// `UsageUpdated` / one-shot `SpendWarning` events.
    async fn record_usage(
        &self,
        segment: u32,
        turn: u32,
        response: &xai_grok_sampling_types::ConversationResponse,
        sink: &Arc<dyn AgentEventSink>,
        budget: &BudgetConfig,
    ) {
        let mut ledger = self.usage.lock().await;
        ledger.record(response.usage.as_ref(), response.cost_usd_ticks);
        let snap = ledger.snapshot();
        let has_signal = response.usage.is_some() || response.cost_usd_ticks.is_some();
        let warn = ledger.check_spend_warning(budget.budget_tokens());
        let warn =
            warn.map(|percent_used| (percent_used, snap.total_tokens, budget.budget_tokens()));
        drop(ledger);
        if has_signal {
            sink.on_event(AgentEvent::UsageUpdated {
                segment,
                turn,
                prompt_tokens: snap.prompt_tokens,
                completion_tokens: snap.completion_tokens,
                total_tokens: snap.total_tokens,
                cost_usd: snap.cost_usd,
            })
            .await;
        }
        if let Some((percent_used, total_tokens, budget_tokens)) = warn {
            sink.on_event(AgentEvent::SpendWarning {
                percent_used,
                total_tokens,
                budget_tokens,
            })
            .await;
        }
    }

    /// True when the accumulated cost exceeds the configured spend cap.
    async fn spend_capped(&self, budget: &BudgetConfig) -> bool {
        self.usage.lock().await.exceeds_cap(budget.spend_cap_usd)
    }

    /// True when the tool-bridge todo resource holds pending or in-progress
    /// items. An absent resource (the model never called `todo_write`) counts
    /// as "nothing open".
    ///
    /// Advisory only: no longer a stop gate for the segmented runner (a model
    /// that never calls `todo_write` must still be able to run long tasks).
    /// Used by tests and retained for callers that want the signal.
    #[allow(dead_code)]
    async fn has_open_todos(&self) -> bool {
        !self.open_todo_labels().await.is_empty()
    }

    /// Labels of pending/in-progress todos (for compaction checkpoints).
    async fn open_todo_labels(&self) -> Vec<String> {
        self.tools
            .get_resource_cloned::<State<TodoState>>()
            .await
            .map(|state| {
                state
                    .0
                    .todo_items()
                    .filter(|t| matches!(t.status, TodoStatus::Pending | TodoStatus::InProgress))
                    .map(|t| format!("{} {}", t.status.tag(), t.content))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Re-inject the open-todo list as a system reminder at segment starts so
    /// the continued segment resumes with pending work in context even after
    /// a compaction checkpoint folded the raw history. No-op when no todos
    /// are open (keeps short chats clean).
    async fn reinject_todos(&self) {
        let open = self.open_todo_labels().await;
        if open.is_empty() {
            return;
        }
        let mut reminder = String::from("Outstanding todos carried into this segment:\n");
        for todo in &open {
            reminder.push_str("- ");
            reminder.push_str(todo);
            reminder.push('\n');
        }
        reminder.push_str("Continue with the next pending item. Do not redo completed work.");
        self.chat
            .push_user_message(ConversationItem::system_reminder(reminder));
    }

    /// Phase 3 checkpoint: when estimated context occupancy crosses the
    /// configured percentage of the context window, fold history above the
    /// newest tail into one summary item (system head + tail kept verbatim).
    /// Best-effort: disabled policy, a dead actor, or short history simply
    /// skips. Reads `context` from the config store (defaults when no config
    /// manager is attached) and the window from the live session (§5.6-E).
    async fn maybe_compact(&self, last_wrap_up: Option<String>) {
        let base = match &self.config_manager {
            Some(manager) => CompactPolicy::from(&manager.read().await.context),
            None => CompactPolicy::default(),
        };
        let policy = base.with_window(self.effective_context_window().await);
        let estimated = self.chat.get_estimated_total_tokens().await;
        if policy.decide(estimated) != crate::compaction::CompactDecision::Checkpoint {
            return;
        }
        let history = self.chat.get_conversation().await;
        let open = self.open_todo_labels().await;
        let wrap_up = last_wrap_up.as_deref();
        let summary = crate::compaction::build_checkpoint_summary(&open, wrap_up);
        if let Some(next) = crate::compaction::build_checkpoint_history_with(
            &history,
            summary,
            policy.tail_keep_items,
        ) {
            self.chat.replace_conversation_for_compaction(next);
        }
    }

    /// Segmented runner: run one segment, and while the run ended in a
    /// budget wrap-up, auto-continue with a fresh budget.
    ///
    /// When `[budget].spend_cap_usd` is set, segments are unbounded — the run
    /// stops only on natural finish, the identical-wrap-up guard, the spend
    /// cap, or cancel. When no cap is set, `max_segments` (config `[budget]`,
    /// default 3) remains the safety limit so an uncapped run can never burn
    /// unbounded money by default. `auto_continue: false` forces one segment.
    ///
    /// Todos are advisory only (re-injected at segment starts and embedded in
    /// compaction summaries) — a model that never calls `todo_write` no
    /// longer ends the run after segment 1. The identical-wrap-up guard stays.
    ///
    /// Stop conditions: the model finished a segment on its own (no budget
    /// hit), identical consecutive wrap-ups (broken-loop guard), the spend
    /// cap (graceful `SpendCapReached` stop at a segment boundary, hard bail
    /// only as a mid-segment backstop), `max_segments` when no cap is set, or
    /// the user cancels (see [`Self::cancel_current_run`]).
    /// Between segments a Phase 3 compaction checkpoint folds history
    /// above the configured threshold into one summary item (system head +
    /// the newest tail kept verbatim), and the open-todo list is re-injected
    /// so the next segment resumes without re-reading raw dumps.
    pub async fn send_user_message_auto(
        &self,
        text: &str,
        sink: Arc<dyn AgentEventSink>,
    ) -> Result<()> {
        let budget = self.budget().await;
        let token = self.reset_cancel_token();
        // Per-message ledger: a tripped cap must not poison later messages on
        // this session.
        self.usage.lock().await.reset();
        // Unbounded running is an explicit `spend_cap_usd` opt-in *and* an
        // explicit auto-continue opt-in: a configured `auto_continue: false`
        // means one segment even when a cap is set.
        let capped = budget.spend_cap_usd.is_some();
        let unbounded = budget.auto_continue && capped;
        let max_segments = if budget.auto_continue {
            budget.effective_max_segments()
        } else {
            1
        };
        let mut last_wrap_up: Option<String> = None;
        // Guard state kept separately from `last_wrap_up`: an empty wrap-up is
        // stored as `None`, so comparing `Option`s directly can never see "two
        // empty ones in a row" even though the comment below promises it.
        let mut prev_wrap_up: Option<String> = None;
        let mut segment: u32 = 1;
        loop {
            if segment > 1 {
                sink.on_event(AgentEvent::SegmentBoundary {
                    segment,
                    max_segments,
                })
                .await;
                self.maybe_compact(last_wrap_up.clone()).await;
                self.reinject_todos().await;
            }
            // Graceful cap stop at a segment boundary: emit the event and end
            // the run with `Ok`, never a mid-run `bail!`.
            if self.spend_capped(&budget).await {
                let spent = self.usage.lock().await.cost_usd();
                let cap = budget.spend_cap_usd.unwrap_or(0.0);
                sink.on_event(AgentEvent::SpendCapReached {
                    spent_usd: spent,
                    cap_usd: cap,
                })
                .await;
                break;
            }
            let prompt = if segment == 1 { text } else { CONTINUE_PROMPT };
            let outcome = self
                .run_segment(prompt, sink.clone(), segment, None, &budget, &token)
                .await?;

            // Natural completion: nothing to continue.
            if !outcome.hit_budget {
                break;
            }
            // Broken-loop guard: two identical wrap-up outcomes in a row
            // (including two empty ones) mean the model is spinning; stop
            // instead of paying for another segment. Normalise the empty case
            // to `""` so `None` == `None` actually compares as equal.
            let current_wrap_up = outcome.wrap_up_text.clone().unwrap_or_default();
            if let Some(prev) = &prev_wrap_up {
                if *prev == current_wrap_up {
                    break;
                }
            }
            prev_wrap_up = Some(current_wrap_up);
            last_wrap_up = outcome.wrap_up_text;
            // Segment cap applies only in the unbounded case above (auto-continue
            // with a spend cap is the explicit opt-in). Without a cost signal the
            // cap can never trip, so also stop at an absolute backstop to avoid
            // looping forever on providers that report no cost.
            if segment >= max_segments && (!unbounded || segment >= ABSOLUTE_MAX_SEGMENTS) {
                break;
            }
            segment = segment.saturating_add(1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ProviderCapabilities, ProviderKind};
    use xai_grok_sampling_types::{
        AssistantItem, ConversationRequest, ConversationResponse, StopReason, ToolCall,
    };

    const UNKNOWN_TOOL: &str = "definitely_not_a_registered_tool";

    use xai_grok_tools::implementations::grok_build::todo::{
        TodoItem, TodoPriority, TodoState, TodoStatus,
    };
    use xai_grok_tools::types::resources::State as TodoStateResource;

    // ── Test infrastructure ─────────────────────────────────────────────

    #[derive(Default)]
    struct CollectingSink {
        events: std::sync::Mutex<Vec<AgentEvent>>,
    }

    #[async_trait::async_trait]
    impl AgentEventSink for CollectingSink {
        async fn on_event(&self, event: AgentEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    impl CollectingSink {
        fn events(&self) -> Vec<AgentEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    #[derive(Clone)]
    struct ScriptedTurn {
        text: &'static str,
        /// (tool name, raw JSON arguments) pairs emitted as client tool calls.
        tool_calls: Vec<(&'static str, &'static str)>,
    }

    fn response_for_full(
        text: &str,
        tool_calls: Vec<ToolCall>,
        usage: Option<xai_grok_sampling_types::TokenUsage>,
        cost_usd_ticks: Option<i64>,
    ) -> ConversationResponse {
        ConversationResponse {
            items: vec![ConversationItem::Assistant(AssistantItem {
                content: text.into(),
                tool_calls,
                model_id: Some("scripted".into()),
                model_fingerprint: None,
                reasoning_effort: None,
            })],
            stop_reason: Some(StopReason::Stop),
            usage,
            cost_usd_ticks,
            message_chunks_emitted: 0,
            doom_loop_signals: Vec::new(),
            stop_message: None,
            message_id: None,
            raw_stop_reason: None,
            stop_sequence: None,
        }
    }

    fn scripted_usage(prompt: u32, completion: u32) -> xai_grok_sampling_types::TokenUsage {
        xai_grok_sampling_types::TokenUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
            cache_creation_prompt_tokens: 0,
        }
    }

    /// Provider that replays scripted turns (repeating its last turn once the
    /// script is exhausted). Whenever the request offers no tools — the
    /// wrap-up turn — it answers with plain text only, honoring the
    /// "no tools offered ⇒ forced text answer" contract.
    ///
    /// `distinct_wrap_ups` controls whether successive wrap-up answers carry a
    /// counter (so the runner's broken-loop guard does not fire) or the same
    /// canned text (so the guard does fire).
    struct ScriptedProvider {
        turns: std::sync::Mutex<Vec<ScriptedTurn>>,
        wrap_up_counter: std::sync::Mutex<u32>,
        distinct_wrap_ups: bool,
        /// Answer wrap-up turns with an empty string, which the runner records
        /// as `wrap_up_text == None`.
        empty_wrap_up: bool,
        usage: Option<xai_grok_sampling_types::TokenUsage>,
        cost_usd_ticks: Option<i64>,
    }

    impl ScriptedProvider {
        fn new(turns: Vec<ScriptedTurn>) -> Self {
            Self::new_distinct(turns, false)
        }

        fn new_distinct(turns: Vec<ScriptedTurn>, distinct_wrap_ups: bool) -> Self {
            Self::with_telemetry(turns, distinct_wrap_ups, None, None)
        }

        fn with_telemetry(
            turns: Vec<ScriptedTurn>,
            distinct_wrap_ups: bool,
            usage: Option<xai_grok_sampling_types::TokenUsage>,
            cost_usd_ticks: Option<i64>,
        ) -> Self {
            Self {
                turns: std::sync::Mutex::new(turns),
                wrap_up_counter: std::sync::Mutex::new(0),
                distinct_wrap_ups,
                empty_wrap_up: false,
                usage,
                cost_usd_ticks,
            }
        }

        fn with_empty_wrap_up(mut self) -> Self {
            self.empty_wrap_up = true;
            self
        }
    }

    #[async_trait::async_trait]
    impl Provider for ScriptedProvider {
        fn id(&self) -> &str {
            "scripted"
        }
        fn name(&self) -> &str {
            "Scripted"
        }
        fn kind(&self) -> ProviderKind {
            ProviderKind::Xai
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::default()
        }

        async fn complete(&self, request: ConversationRequest) -> Result<ConversationResponse> {
            let next = {
                let mut turns = self.turns.lock().unwrap();
                if turns.len() > 1 {
                    turns.remove(0)
                } else {
                    turns[0].clone()
                }
            };
            let (usage, cost) = (self.usage.clone(), self.cost_usd_ticks);
            if request.tools.is_empty() {
                if self.empty_wrap_up {
                    return Ok(response_for_full("", vec![], usage, cost));
                }
                let text = if self.distinct_wrap_ups {
                    let mut n = self.wrap_up_counter.lock().unwrap();
                    *n += 1;
                    format!("WRAP-UP: budget exhausted; segment summary #{n}.")
                } else {
                    "WRAP-UP: budget exhausted; here is the summary.".to_string()
                };
                return Ok(response_for_full(&text, vec![], usage, cost));
            }
            let calls = next
                .tool_calls
                .iter()
                .enumerate()
                .map(|(i, (name, args))| ToolCall {
                    id: format!("call-{i}").into(),
                    name: (*name).to_string(),
                    arguments: (*args).into(),
                })
                .collect();
            Ok(response_for_full(next.text, calls, usage, cost))
        }
    }

    async fn run(
        script: Vec<ScriptedTurn>,
    ) -> (Result<RunOutcome>, Arc<CollectingSink>, ChatStateHandle) {
        let chat = crate::tools::build_chat_handle("phase1-test", None).expect("chat handle");
        let tools = test_bridge("p1").await;
        let provider: Arc<dyn Provider> = Arc::new(ScriptedProvider::new(script));
        let sink = Arc::new(CollectingSink::default());
        let agent = AgentLoop::new(chat.clone(), tools, provider);
        let result = agent
            .send_user_message("run the scripted scenario", sink.clone())
            .await;
        (result, sink, chat)
    }

    /// Build an agent over the scripted provider and return it with the sink,
    /// so tests can seed tool-bridge resources before driving a run.
    async fn build_agent(
        script: Vec<ScriptedTurn>,
        distinct_wrap_ups: bool,
    ) -> (Arc<AgentLoop>, Arc<CollectingSink>) {
        build_agent_with_telemetry(script, distinct_wrap_ups, None, None).await
    }

    /// [`build_agent`] plus per-turn usage/cost signals for telemetry tests.
    async fn build_agent_with_telemetry(
        script: Vec<ScriptedTurn>,
        distinct_wrap_ups: bool,
        usage: Option<xai_grok_sampling_types::TokenUsage>,
        cost_usd_ticks: Option<i64>,
    ) -> (Arc<AgentLoop>, Arc<CollectingSink>) {
        let chat = crate::tools::build_chat_handle("phase2-test", None).expect("chat handle");
        let tools = test_bridge("p2").await;
        let provider: Arc<dyn Provider> = Arc::new(ScriptedProvider::with_telemetry(
            script,
            distinct_wrap_ups,
            usage,
            cost_usd_ticks,
        ));
        let sink = Arc::new(CollectingSink::default());
        let agent = Arc::new(AgentLoop::new(chat, tools, provider));
        (agent, sink)
    }

    /// A tool bridge with a per-call unique state dir, so todo resources never
    /// leak between tests (they would otherwise share
    /// `temp/maverick-app/resources_state.json`, which the real app also uses).
    async fn test_bridge(tag: &str) -> ToolBridge {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("maverick-test-{tag}-{n}"));
        crate::tools::build_tool_bridge_with_app_data(Some(dir))
            .await
            .expect("tool bridge")
    }

    /// Attach a per-test config dir + a context policy to an agent, returning
    /// the manager so tests can mutate policy mid-test.
    async fn attach_config(
        agent: Arc<AgentLoop>,
        context: crate::config::ContextConfig,
    ) -> (Arc<AgentLoop>, Arc<crate::config::ConfigManager>) {
        let (agent, manager) = rebuild_with_fresh_manager(agent).await;
        manager
            .set_context_config(context)
            .await
            .expect("set context");
        (agent, manager)
    }

    /// Attach a per-test config dir + a turn/segment budget to an agent.
    async fn attach_budget(
        agent: Arc<AgentLoop>,
        budget: crate::config::BudgetConfig,
    ) -> (Arc<AgentLoop>, Arc<crate::config::ConfigManager>) {
        let (agent, manager) = rebuild_with_fresh_manager(agent).await;
        manager.set_budget_config(budget).await.expect("set budget");
        (agent, manager)
    }

    /// Rebuild the agent over the same chat/tools/provider with a fresh
    /// per-test config manager (unique temp dir, so tests never share state).
    async fn rebuild_with_fresh_manager(
        agent: Arc<AgentLoop>,
    ) -> (Arc<AgentLoop>, Arc<crate::config::ConfigManager>) {
        static NEXT_CFG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT_CFG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("maverick-test-cfg-{n}"));
        let _ = std::fs::create_dir_all(&dir);
        let manager = Arc::new(crate::config::ConfigManager::new(dir).expect("config manager"));
        // Rebuild the agent over the same chat/tools/provider with config.
        let provider = agent.provider.read().await.clone();
        let agent = Arc::new(AgentLoop {
            chat: agent.chat.clone(),
            tools: agent.tools.clone(),
            provider: RwLock::new(provider),
            config_manager: Some(manager.clone()),
            usage: Arc::new(Mutex::new(crate::usage::UsageLedger::new())),
            cancel: std::sync::Mutex::new(CancellationToken::new()),
            subagent_depth: 0,
        });
        (agent, manager)
    }

    /// Seed the bridge's todo resource with `todos` (merging over the
    /// current state, creating it if absent).
    async fn seed_todos(agent: &AgentLoop, todos: Vec<(&str, TodoStatus)>) {
        let mut state = agent
            .tools
            .get_resource_cloned::<TodoStateResource<TodoState>>()
            .await
            .unwrap_or_else(|| TodoStateResource(TodoState::default()));
        for (id, status) in todos {
            state.0.push(
                id.to_string(),
                TodoItem {
                    content: format!("todo {id}"),
                    priority: TodoPriority::Medium,
                    status,
                    meta: None,
                },
            );
        }
        agent.tools.update_resource(state).await;
    }

    fn turns_of(events: &[AgentEvent]) -> Vec<u32> {
        events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::TurnStarted { turn } => Some(*turn),
                _ => None,
            })
            .collect()
    }

    async fn history_json(chat: &ChatStateHandle) -> String {
        chat.get_conversation()
            .await
            .iter()
            .map(|i| serde_json::to_string(i).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ── Fake-provider loop tests ────────────────────────────────────────────

    /// A provider that always emits one tool call must end in a wrap-up text
    /// answer at `MAX_TURNS + 1` — not a bail — and no turn may run past the
    /// wrap-up on the happy path.
    #[tokio::test]
    async fn scripted_tool_caller_ends_in_wrap_up_not_bail() {
        let script = vec![ScriptedTurn {
            text: "calling tool",
            tool_calls: vec![(UNKNOWN_TOOL, "{}")],
        }];
        let (result, sink, _chat) = run(script).await;
        result.expect("loop must end gracefully in a wrap-up, not bail");

        let events = sink.events();
        let turns = turns_of(&events);
        assert_eq!(
            turns.last(),
            Some(&(MAX_TURNS + 1)),
            "wrap-up must fire exactly at MAX_TURNS+1"
        );
        assert!(
            turns.iter().all(|t| *t <= MAX_TURNS + 1),
            "no turns beyond the wrap-up on the happy path"
        );

        // Exactly one warning, at warning_at, with the right remaining count.
        let warnings = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::BudgetWarning { .. }))
            .count();
        assert_eq!(warnings, 1, "exactly one budget warning");
        let expected_remaining = MAX_TURNS - BudgetConfig::default().warning_at();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AgentEvent::BudgetWarning { remaining }
                    if *remaining == expected_remaining)),
            "warning carries remaining = max_turns - warning_at"
        );

        // Final visible answer is the forced wrap-up text, and every failed
        // bridge call was marked as an error in history.
        let last_text = events
            .iter()
            .rev()
            .find_map(|e| match e {
                AgentEvent::AssistantText { text } => Some(text.clone()),
                _ => None,
            })
            .expect("wrap-up text present");
        assert!(last_text.contains("WRAP-UP:"), "got: {last_text}");

        let history = history_json(&_chat).await;
        assert!(
            history.contains(TOOL_ERROR_PREFIX),
            "errors marked in history"
        );
        assert!(
            history.contains("WRAP-UP:"),
            "wrap-up prompt present in history"
        );
    }

    /// Regression: a normal 2-turn conversation must show zero warnings, zero
    /// wrap-up/warning injection, and successful tool results without the
    /// error prefix.
    #[tokio::test]
    async fn short_conversation_has_no_warning_or_wrap_up() {
        let script = vec![
            ScriptedTurn {
                text: "writing file",
                tool_calls: vec![(
                    "write_to_file",
                    r#"{"path":"maverick_phase1_test.txt","content":"hello phase 1"}"#,
                )],
            },
            ScriptedTurn {
                text: "done, all finished.",
                tool_calls: vec![],
            },
        ];
        let (result, sink, chat) = run(script).await;
        result.expect("short conversation must succeed");

        let events = sink.events();
        assert_eq!(turns_of(&events).len(), 2, "exactly two turns");
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, AgentEvent::BudgetWarning { .. })),
            "no budget warnings on short chats"
        );

        let written = crate::tools::resolve_workspace_dir().join("maverick_phase1_test.txt");
        assert!(written.exists(), "write_to_file executed");
        let _ = std::fs::remove_file(&written);

        let history = history_json(&chat).await;
        assert!(
            history.contains("Successfully wrote"),
            "successful tool result recorded in history"
        );
        assert!(
            !history.contains(TOOL_ERROR_PREFIX),
            "no error prefix on successes"
        );
        assert!(!history.contains("WRAP-UP:"), "no wrap-up injection");
        assert!(
            !history.contains("roughly 3 tool-call turns"),
            "no budget-warning injection"
        );
    }

    /// Invalid JSON arguments must surface as a `[TOOL_ERROR]` tool result
    /// (round-tripping through history) instead of silently executing with
    /// `Null` args or killing the loop.
    #[tokio::test]
    async fn invalid_json_arguments_become_tool_error() {
        let script = vec![
            ScriptedTurn {
                text: "bad args",
                tool_calls: vec![(UNKNOWN_TOOL, "{not json")],
            },
            ScriptedTurn {
                text: "recovered",
                tool_calls: vec![],
            },
        ];
        let (result, sink, chat) = run(script).await;
        result.expect("invalid args must not kill the loop");

        let events = sink.events();
        assert!(
            events.iter().any(|e| matches!(e, AgentEvent::ToolCallCompleted { output, .. }
                if output.starts_with(TOOL_ERROR_PREFIX) && output.contains("invalid tool arguments"))),
            "invalid-arguments error surfaced as a tool result"
        );

        let history = history_json(&chat).await;
        assert!(
            history.contains("invalid tool arguments"),
            "error round-trips through history"
        );
        assert!(
            history.contains(TOOL_ERROR_PREFIX),
            "[TOOL_ERROR] prefix survives the round-trip"
        );
    }

    // ── Unit tests: output shaping helpers ──────────────────────────────────

    #[test]
    fn truncation_preserves_head_and_tail_at_boundary() {
        // Short inputs — including the exact limit — pass through untouched.
        assert_eq!(
            truncate_output_with("short", MAX_TOOL_OUTPUT_BYTES),
            "short"
        );
        assert_eq!(
            truncate_output_with(&"x".repeat(MAX_TOOL_OUTPUT_BYTES), MAX_TOOL_OUTPUT_BYTES),
            "x".repeat(MAX_TOOL_OUTPUT_BYTES)
        );

        // Head cut lands inside a multi-byte 🦀 (must not panic) and the tail
        // is preserved verbatim.
        let text = format!("a{}{}", "🦀".repeat(2000), "b".repeat(9000));
        let out = truncate_output_with(&text, MAX_TOOL_OUTPUT_BYTES);
        assert!(out.contains("[truncated 5004 bytes]"), "got: {out}");
        assert!(out.starts_with("a🦀"), "head intact: {out}");
        assert!(out.ends_with("bbb"), "tail preserved");

        // Tail cut lands inside a multi-byte 🦀 (must not panic).
        let text = format!("a{}{}", "🦀".repeat(3000), "b".repeat(2001));
        let out = truncate_output_with(&text, MAX_TOOL_OUTPUT_BYTES);
        assert!(out.contains("[truncated 2008 bytes]"), "got: {out}");
        assert!(out.starts_with("a🦀"), "head intact: {out}");
        assert!(out.ends_with('b'), "tail preserved");
    }

    /// A per-tool budget below the loop default still truncates, and a budget
    /// at/above the default is a no-op (budgets only ever shrink).
    #[test]
    fn per_tool_budget_truncates_below_default_only() {
        let text = "z".repeat(2000);
        // Budget larger than the input: untouched.
        assert_eq!(truncate_output_with(&text, 4000), text);
        // Budget smaller than the input: head+tail with a marker.
        let out = truncate_output_with(&text, 400);
        assert!(out.contains("truncated"), "got: {out}");
        assert!(out.len() < text.len());
    }

    #[test]
    fn error_prefix_is_idempotent() {
        let once = format_tool_result("boom".to_string(), true);
        assert_eq!(once, format!("{TOOL_ERROR_PREFIX}boom"));
        assert_eq!(format_tool_result(once.clone(), true), once);
        assert_eq!(format_tool_result("ok".to_string(), false), "ok");
    }

    // ── Phase 2: segments & auto-continue ───────────────────────────────────

    /// `has_open_todos`: true with pending/in-progress items, false when all
    /// completed/cancelled, and false when the resource is absent entirely.
    #[tokio::test]
    async fn has_open_todos_reflects_todo_state() {
        let chat = crate::tools::build_chat_handle("phase2-todos", None).expect("chat handle");
        let tools = test_bridge("p2-todos").await;
        let provider: Arc<dyn Provider> = Arc::new(ScriptedProvider::new(vec![]));
        let agent = AgentLoop::new(chat, tools, provider);

        // Absent resource -> false.
        assert!(!agent.has_open_todos().await);

        // Pending item -> true.
        seed_todos(&agent, vec![("a", TodoStatus::Pending)]).await;
        assert!(agent.has_open_todos().await);

        // In-progress item -> true.
        seed_todos(&agent, vec![("b", TodoStatus::InProgress)]).await;
        assert!(agent.has_open_todos().await);

        // Everything completed/cancelled -> false.
        seed_todos(
            &agent,
            vec![("a", TodoStatus::Completed), ("b", TodoStatus::Cancelled)],
        )
        .await;
        assert!(!agent.has_open_todos().await);
    }

    /// Provider always tool-calls, todos stay open: the runner must cross two
    /// segment boundaries, hit `MAX_SEGMENTS`, and end gracefully — no bail.
    #[tokio::test]
    async fn always_tool_calling_provider_runs_max_segments() {
        let script = vec![ScriptedTurn {
            text: "still working",
            tool_calls: vec![(UNKNOWN_TOOL, "{}")],
        }];
        let (agent, sink) = build_agent(script, true).await;
        seed_todos(&agent, vec![("work", TodoStatus::InProgress)]).await;
        let result = agent
            .send_user_message_auto("run the scripted scenario", sink.clone())
            .await;
        result.expect("segmented run must end gracefully");

        let events = sink.events();
        let boundaries = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::SegmentBoundary { .. }))
            .count();
        assert_eq!(
            boundaries,
            (MAX_SEGMENTS - 1) as usize,
            "one boundary event per auto-continued segment"
        );
        assert!(
            events.iter().any(
                |e| matches!(e, AgentEvent::SegmentBoundary { segment, max_segments }
                    if *segment == MAX_SEGMENTS && *max_segments == MAX_SEGMENTS)
            ),
            "final segment boundary carries the cap"
        );
        // Three independent budgets were consumed.
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, AgentEvent::BudgetWarning { .. }))
                .count(),
            MAX_SEGMENTS as usize,
            "one budget warning per segment"
        );
        // No hard-ceiling bail anywhere in the event stream.
        assert!(
            !events.iter().any(|e| matches!(e, AgentEvent::Error { .. })),
            "no error events expected"
        );
    }

    /// Natural completion with todos still open: exactly one segment, no
    /// continuation — the `!hit_budget` gate.
    #[tokio::test]
    async fn natural_completion_does_not_continue() {
        let script = vec![
            ScriptedTurn {
                text: "doing work",
                tool_calls: vec![(UNKNOWN_TOOL, "{}")],
            },
            ScriptedTurn {
                text: "all done.",
                tool_calls: vec![],
            },
        ];
        let (agent, sink) = build_agent(script, true).await;
        seed_todos(&agent, vec![("left", TodoStatus::Pending)]).await;
        let result = agent
            .send_user_message_auto("run the scripted scenario", sink.clone())
            .await;
        result.expect("run must succeed");
        assert!(agent.has_open_todos().await, "todos remain open");

        let boundaries = sink
            .events()
            .iter()
            .filter(|e| matches!(e, AgentEvent::SegmentBoundary { .. }))
            .count();
        assert_eq!(boundaries, 0, "no continuation after natural completion");
    }

    /// Two identical wrap-up summaries in a row: the broken-loop guard must
    /// stop after the second segment instead of paying for the third.
    #[tokio::test]
    async fn identical_wrap_ups_stop_the_runner() {
        let script = vec![ScriptedTurn {
            text: "stuck but talking",
            tool_calls: vec![(UNKNOWN_TOOL, "{}")],
        }];
        // distinct_wrap_ups = false: every wrap-up answer is byte-identical.
        let (agent, sink) = build_agent(script, false).await;
        seed_todos(&agent, vec![("stuck", TodoStatus::Pending)]).await;
        let result = agent
            .send_user_message_auto("run the scripted scenario", sink.clone())
            .await;
        result.expect("run must end gracefully");

        let boundaries = sink
            .events()
            .iter()
            .filter(|e| matches!(e, AgentEvent::SegmentBoundary { .. }))
            .count();
        // Segment 1 wraps up; todos are open so the runner continues into
        // segment 2 (1 boundary). Segment 2's wrap-up is identical to
        // segment 1's, so the broken-loop guard fires BEFORE segment 3.
        // Without the guard this would be MAX_SEGMENTS - 1 boundaries.
        assert_eq!(boundaries, 1, "guard stopped the runner after segment 2");
    }

    /// Regression: a short natural chat is one segment with no boundary
    /// events, and `RunOutcome` reports a natural stop.
    #[tokio::test]
    async fn run_outcome_reports_natural_stop() {
        let script = vec![ScriptedTurn {
            text: "plain answer, no tools",
            tool_calls: vec![],
        }];
        let chat = crate::tools::build_chat_handle("phase2-outcome", None).expect("chat handle");
        let tools = test_bridge("p2-outcome").await;
        let provider: Arc<dyn Provider> = Arc::new(ScriptedProvider::new(script));
        let sink = Arc::new(CollectingSink::default());
        let agent = AgentLoop::new(chat, tools, provider);
        let outcome = agent
            .send_user_message("hello", sink.clone())
            .await
            .expect("run succeeds");
        assert!(!outcome.hit_budget);
        assert_eq!(outcome.wrap_up_text, None);
    }

    // ── Phase 3: context management ─────────────────────────────────────────

    /// `maybe_compact` is a no-op below threshold: history untouched, no
    /// compaction flag set.
    #[tokio::test]
    async fn compact_skips_below_threshold() {
        let script = vec![ScriptedTurn {
            text: "plain answer",
            tool_calls: vec![],
        }];
        let (agent, sink) = build_agent(script, true).await;
        agent
            .send_user_message_auto("hello", sink)
            .await
            .expect("run succeeds");
        let history = agent.chat.get_conversation().await;
        assert!(
            !history.iter().any(|item| {
                serde_json::to_string(item)
                    .unwrap_or_default()
                    .contains(crate::compaction::COMPACTION_SUMMARY_MARKER)
            }),
            "no checkpoint below threshold"
        );
    }

    /// Disabled policy never compacts, even at max occupancy.
    #[tokio::test]
    async fn compact_disabled_policy_never_fires() {
        let script = vec![ScriptedTurn {
            text: "plain answer",
            tool_calls: vec![],
        }];
        let (agent, _) = build_agent(script, true).await;
        let (agent, _) = attach_config(
            agent,
            crate::config::ContextConfig {
                auto_compact_enabled: false,
                ..Default::default()
            },
        )
        .await;
        agent.maybe_compact(None).await;
        let history = agent.chat.get_conversation().await;
        assert!(
            !history.iter().any(|item| {
                serde_json::to_string(item)
                    .unwrap_or_default()
                    .contains(crate::compaction::COMPACTION_SUMMARY_MARKER)
            }),
            "disabled policy must not checkpoint"
        );
    }

    /// Per-tool output budgets shrink below the 12 KB default: a tool result
    /// over the configured budget lands in history truncated to it.
    #[tokio::test]
    async fn per_tool_output_budget_applies() {
        // `read_file` on a missing path returns a short deterministic error
        // (~40 bytes) — small enough to check the raw text survives at the
        // default budget, large enough to exceed the 8-byte test budget.
        let script = vec![
            ScriptedTurn {
                text: "reading file",
                tool_calls: vec![("read_file", r#"{"path":"nonexistent-xyz.txt"}"#)],
            },
            ScriptedTurn {
                text: "done",
                tool_calls: vec![],
            },
        ];
        let (agent, sink) = build_agent(script, true).await;
        let (agent, _) = attach_config(
            agent,
            crate::config::ContextConfig {
                // Absurdly small on purpose: any real tool output exceeds it,
                // so the truncation path is exercised deterministically.
                tool_output_budgets: [("read_file".to_string(), 8)].into_iter().collect(),
                ..Default::default()
            },
        )
        .await;
        agent
            .send_user_message_auto("read it", sink)
            .await
            .expect("run succeeds");
        let history = history_json(&agent.chat).await;
        assert!(
            history.contains("truncated"),
            "per-tool budget truncated the stored result; got {history}"
        );
    }

    /// Re-injection posts a system reminder listing open todos, and stays
    /// silent when nothing is open.
    #[tokio::test]
    async fn reinject_todos_posts_reminder_only_when_open() {
        let chat = crate::tools::build_chat_handle("phase3-reinject", None).expect("chat handle");
        let tools = test_bridge("p3-reinject").await;
        let provider: Arc<dyn Provider> = Arc::new(ScriptedProvider::new(vec![]));
        let agent = AgentLoop::new(chat, tools, provider);

        // Nothing open: no reminder.
        let before = agent.chat.get_conversation().await.len();
        agent.reinject_todos().await;
        assert_eq!(agent.chat.get_conversation().await.len(), before);

        // Open todo: one reminder containing the label.
        seed_todos(&agent, vec![("t1", TodoStatus::Pending)]).await;
        agent.reinject_todos().await;
        let history = history_json(&agent.chat).await;
        assert!(
            history.contains("Outstanding todos carried into this segment"),
            "reminder injected; got {history}"
        );
        assert!(
            history.contains("todo t1"),
            "todo label present; got {history}"
        );
    }

    // ── §5.6-F: budget config ────────────────────────────────────────────────

    #[test]
    fn budget_defaults_match_legacy_hardcoded_values() {
        let budget = crate::config::BudgetConfig::default();
        assert_eq!(budget.max_turns, MAX_TURNS);
        assert_eq!(budget.max_segments, MAX_SEGMENTS);
        assert!(budget.auto_continue);
        assert_eq!(budget.spend_cap_usd, None);
        assert_eq!(budget.warning_at(), MAX_TURNS - 3);
        assert_eq!(budget.wrap_up_at(), MAX_TURNS + 1);
        assert_eq!(budget.hard_ceiling(), MAX_TURNS + 2);
        assert_eq!(
            budget.budget_tokens(),
            MAX_SEGMENTS as u64 * MAX_TURNS as u64 * 2000
        );
    }

    #[test]
    fn tiny_budgets_saturate_instead_of_underflowing() {
        let budget = crate::config::BudgetConfig {
            max_turns: 1,
            max_segments: 0,
            ..Default::default()
        };
        assert_eq!(budget.effective_max_turns(), 1);
        assert_eq!(budget.effective_max_segments(), 1);
        assert_eq!(budget.warning_at(), 1);
        assert_eq!(budget.wrap_up_at(), 2);
    }

    #[test]
    fn budget_config_survives_toml_round_trip_with_legacy_default() {
        let cfg = crate::config::MaverickConfig::default();
        assert_eq!(cfg.budget.max_turns, MAX_TURNS);
        assert_eq!(cfg.budget.max_segments, MAX_SEGMENTS);
        let toml = toml::to_string(&cfg).expect("serialize");
        let back: crate::config::MaverickConfig = toml::from_str(&toml).expect("deserialize");
        assert_eq!(back.budget.max_turns, MAX_TURNS);
        // Old config files without `[budget]` still load (serde default).
        let legacy: crate::config::MaverickConfig =
            toml::from_str("[ui]\ntheme = \"dark\"\n").expect("legacy deserialize");
        assert_eq!(legacy.budget.max_turns, MAX_TURNS);
        assert!(legacy.budget.auto_continue);
    }

    /// A custom `max_turns` moves the wrap-up turn, and a custom
    /// `max_segments` caps the segmented run — budgets are live, not baked in.
    #[tokio::test]
    async fn custom_budgets_drive_wrap_up_and_segments() {
        let script = vec![ScriptedTurn {
            text: "still working",
            tool_calls: vec![(UNKNOWN_TOOL, "{}")],
        }];
        let (agent, sink) = build_agent(script, true).await;
        let (agent, _) = attach_budget(
            agent,
            crate::config::BudgetConfig {
                max_turns: 5,
                max_segments: 2,
                ..Default::default()
            },
        )
        .await;
        seed_todos(&agent, vec![("work", TodoStatus::InProgress)]).await;
        agent
            .send_user_message_auto("run the scripted scenario", sink.clone())
            .await
            .expect("segmented run must end gracefully");

        let events = sink.events();
        let turns = turns_of(&events);
        assert_eq!(turns.last(), Some(&6), "wrap-up fires at max_turns+1 = 6");
        assert!(
            turns.iter().all(|t| *t <= 6),
            "no turns beyond the custom wrap-up"
        );
        let boundaries = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::SegmentBoundary { .. }))
            .count();
        assert_eq!(boundaries, 1, "custom max_segments = 2 → one boundary");
        assert!(
            events.iter().any(
                |e| matches!(e, AgentEvent::SegmentBoundary { segment, max_segments }
                    if *segment == 2 && *max_segments == 2)
            ),
            "boundary carries the custom cap"
        );
    }

    /// `auto_continue: false` stops after the first segment even on a budget
    /// hit with open todos (K=1 with graceful wrap-up).
    #[tokio::test]
    async fn auto_continue_off_stops_after_one_segment() {
        let script = vec![ScriptedTurn {
            text: "still working",
            tool_calls: vec![(UNKNOWN_TOOL, "{}")],
        }];
        let (agent, sink) = build_agent(script, true).await;
        let (agent, _) = attach_budget(
            agent,
            crate::config::BudgetConfig {
                auto_continue: false,
                ..Default::default()
            },
        )
        .await;
        seed_todos(&agent, vec![("work", TodoStatus::InProgress)]).await;
        agent
            .send_user_message_auto("run the scripted scenario", sink.clone())
            .await
            .expect("run must end gracefully");
        assert_eq!(
            sink.events()
                .iter()
                .filter(|e| matches!(e, AgentEvent::SegmentBoundary { .. }))
                .count(),
            0,
            "no continuation when auto_continue is off"
        );
        let last_text = sink
            .events()
            .iter()
            .rev()
            .find_map(|e| match e {
                AgentEvent::AssistantText { text } => Some(text.clone()),
                _ => None,
            })
            .expect("wrap-up text present");
        assert!(last_text.contains("WRAP-UP:"), "got: {last_text}");
    }

    /// A spend cap must not silently re-enable continuation: `auto_continue: false`
    /// is an explicit one-segment opt-out and holds even when a cap is set (which
    /// otherwise makes segments unbounded).
    #[tokio::test]
    async fn auto_continue_off_wins_over_spend_cap() {
        let mut script = vec![ScriptedTurn {
            text: "still working",
            tool_calls: vec![(UNKNOWN_TOOL, "{}")],
        }; 6];
        script.push(ScriptedTurn {
            text: "all done.",
            tool_calls: vec![],
        });
        let (agent, sink) = build_agent(script, true).await;
        let (agent, _) = attach_budget(
            agent,
            crate::config::BudgetConfig {
                auto_continue: false,
                max_turns: 2,
                max_segments: 5,
                spend_cap_usd: Some(1000.0),
                ..Default::default()
            },
        )
        .await;
        agent
            .send_user_message_auto("run the scripted scenario", sink.clone())
            .await
            .expect("run must end gracefully");
        let boundaries = sink
            .events()
            .iter()
            .filter(|e| matches!(e, AgentEvent::SegmentBoundary { .. }))
            .count();
        assert_eq!(
            boundaries, 0,
            "auto_continue: false must hold even with a spend cap set"
        );
    }

    /// Two budget hits whose wrap-up text is *empty* must also trip the
    /// broken-loop guard. An empty wrap-up is recorded as `None`, so the old
    /// `Option`-comparison could never see "two empty ones in a row" and kept
    /// paying for more segments up to the absolute backstop.
    #[tokio::test]
    async fn empty_wrap_ups_stop_the_runner() {
        let script = vec![ScriptedTurn {
            text: "still working",
            tool_calls: vec![(UNKNOWN_TOOL, "{}")],
        }];
        let chat = crate::tools::build_chat_handle("empty-wrapup", None).expect("chat handle");
        let tools = test_bridge("empty-wrapup").await;
        let provider: Arc<dyn Provider> =
            Arc::new(ScriptedProvider::new(script).with_empty_wrap_up());
        let sink = Arc::new(CollectingSink::default());
        let agent = Arc::new(AgentLoop::new(chat, tools, provider));
        let (agent, _) = attach_budget(
            agent,
            crate::config::BudgetConfig {
                max_segments: 5,
                ..Default::default()
            },
        )
        .await;

        agent
            .send_user_message_auto("run the scripted scenario", sink.clone())
            .await
            .expect("run must end gracefully");

        let boundaries = sink
            .events()
            .iter()
            .filter(|e| matches!(e, AgentEvent::SegmentBoundary { .. }))
            .count();
        assert_eq!(
            boundaries, 1,
            "guard must stop after two identical empty wrap-ups"
        );
    }

    // ── §5.6-B: usage telemetry ──────────────────────────────────────────────

    /// Turns reporting usage emit `UsageUpdated` and accumulate into the
    /// `usage_snapshot` served to the UI.
    #[tokio::test]
    async fn usage_events_accumulate_into_snapshot() {
        let script = vec![
            ScriptedTurn {
                text: "doing work",
                tool_calls: vec![(UNKNOWN_TOOL, "{}")],
            },
            ScriptedTurn {
                text: "all done.",
                tool_calls: vec![],
            },
        ];
        // $0.05/turn, 1000 prompt + 200 completion per turn.
        let (agent, sink) = build_agent_with_telemetry(
            script,
            true,
            Some(scripted_usage(1000, 200)),
            Some(500_000_000),
        )
        .await;
        agent
            .send_user_message("run it", sink.clone())
            .await
            .expect("run succeeds");

        let usage_events = sink
            .events()
            .iter()
            .filter_map(|e| match e {
                AgentEvent::UsageUpdated {
                    total_tokens,
                    cost_usd,
                    ..
                } => Some((*total_tokens, *cost_usd)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(usage_events.len(), 2, "one event per reporting turn");
        assert_eq!(usage_events[0].0, 1200);
        assert_eq!(usage_events[1].0, 2400);

        let snap = agent.usage_snapshot().await;
        assert_eq!(snap.prompt_tokens, 2000);
        assert_eq!(snap.completion_tokens, 400);
        assert_eq!(snap.total_tokens, 2400);
        assert!((snap.cost_usd - 0.1).abs() < 1e-9, "got {}", snap.cost_usd);
        assert_eq!(snap.turns, 2);
        assert_eq!(snap.segments, 1);
    }

    /// Silent providers (no usage/cost) count turns without spamming events.
    #[tokio::test]
    async fn silent_provider_counts_turns_without_usage_events() {
        let script = vec![ScriptedTurn {
            text: "plain answer, no tools",
            tool_calls: vec![],
        }];
        let (agent, sink) = build_agent(script, true).await;
        agent
            .send_user_message("hello", sink.clone())
            .await
            .expect("run succeeds");
        assert!(
            sink.events()
                .iter()
                .all(|e| !matches!(e, AgentEvent::UsageUpdated { .. })),
            "no usage events without usage signals"
        );
        assert_eq!(agent.usage_snapshot().await.turns, 1);
    }

    /// The 80%-of-budget warning fires exactly once per task.
    #[tokio::test]
    async fn spend_warning_fires_once_at_80_percent() {
        let script = vec![ScriptedTurn {
            text: "still working",
            tool_calls: vec![(UNKNOWN_TOOL, "{}")],
        }];
        // Budget 2 segs × 5 turns × 100 avg = 1000 tokens → warn at 800.
        // 500 tokens/turn → warning lands on turn 2 of segment 1.
        let (agent, sink) =
            build_agent_with_telemetry(script, true, Some(scripted_usage(400, 100)), None).await;
        let (agent, _) = attach_budget(
            agent,
            crate::config::BudgetConfig {
                max_turns: 5,
                max_segments: 2,
                avg_tokens_per_turn: 100,
                ..Default::default()
            },
        )
        .await;
        seed_todos(&agent, vec![("work", TodoStatus::InProgress)]).await;
        agent
            .send_user_message_auto("run it", sink.clone())
            .await
            .expect("run succeeds");

        let warnings = sink
            .events()
            .iter()
            .filter_map(|e| match e {
                AgentEvent::SpendWarning {
                    percent_used,
                    total_tokens,
                    budget_tokens,
                } => Some((*percent_used, *total_tokens, *budget_tokens)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(warnings.len(), 1, "one-shot warning, got {warnings:?}");
        assert_eq!(warnings[0].2, 1000);
        assert_eq!(warnings[0].1, 1000);
        assert!(warnings[0].0 >= 80, "got {:?}", warnings[0]);
    }

    /// A spend cap aborts the run once accumulated cost exceeds it.
    #[tokio::test]
    async fn spend_cap_aborts_the_run() {
        let script = vec![ScriptedTurn {
            text: "expensive answer",
            tool_calls: vec![],
        }];
        // $0.10 on the first turn against a $0.05 cap.
        let (agent, sink) = build_agent_with_telemetry(
            script,
            true,
            Some(scripted_usage(10, 10)),
            Some(1_000_000_000),
        )
        .await;
        let (agent, _) = attach_budget(
            agent,
            crate::config::BudgetConfig {
                spend_cap_usd: Some(0.05),
                ..Default::default()
            },
        )
        .await;
        let err = agent
            .send_user_message("run it", sink.clone())
            .await
            .expect_err("spend cap must abort the run");
        assert!(err.to_string().contains("spend cap exceeded"), "got: {err}");
        assert!(
            sink.events().iter().any(|e| matches!(
                e,
                AgentEvent::SpendCapReached { spent_usd, cap_usd }
                    if (*spent_usd - 0.1).abs() < 1e-6 && (*cap_usd - 0.05).abs() < 1e-6
            )),
            "cap surfaced as SpendCapReached"
        );
        assert!(
            sink.events().iter().all(|e| !matches!(e, AgentEvent::Error { .. })),
            "a cap stop is graceful — it must not also emit an Error event"
        );
    }

    // ── §5.6-C: run interrupt ────────────────────────────────────────────────

    /// A pre-cancelled run bails immediately with a `Cancelled` event and no
    /// model turns.
    #[tokio::test]
    async fn pre_cancelled_run_emits_cancelled_without_turns() {
        let script = vec![ScriptedTurn {
            text: "plain answer",
            tool_calls: vec![],
        }];
        let (agent, sink) = build_agent(script, true).await;
        agent.cancel_current_run();
        let err = agent
            .send_user_message("hello", sink.clone())
            .await
            .expect_err("cancelled run must bail");
        assert!(err.to_string().contains("cancelled"), "got: {err}");
        let events = sink.events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AgentEvent::Cancelled { segment: 1 })),
            "Cancelled event present"
        );
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, AgentEvent::TurnStarted { .. })),
            "no model turns after cancel"
        );
    }

    /// A stale cancel never leaks into the next task: the segmented runner
    /// mints a fresh token per user message.
    #[tokio::test]
    async fn auto_run_resets_a_stale_cancel() {
        let script = vec![ScriptedTurn {
            text: "plain answer",
            tool_calls: vec![],
        }];
        let (agent, sink) = build_agent(script, true).await;
        agent.cancel_current_run();
        agent
            .send_user_message_auto("hello", sink.clone())
            .await
            .expect("fresh run must succeed despite the earlier cancel");
        assert!(
            sink.events()
                .iter()
                .any(|e| matches!(e, AgentEvent::TurnStarted { turn: 1 })),
            "the fresh run executed a turn"
        );
    }

    // ── §5.6-D: live subagents ───────────────────────────────────────────────

    /// The parent loop offers `subagent`; a depth-1 child does not.
    #[tokio::test]
    async fn subagent_tool_offered_to_parent_only() {
        let script = vec![ScriptedTurn {
            text: "plain answer",
            tool_calls: vec![],
        }];
        let (agent, _) = build_agent(script, true).await;
        let specs = agent.tool_specs().await;
        assert!(
            specs
                .iter()
                .any(|s| s.name == crate::subagents::SUBAGENT_TOOL_NAME),
            "parent offers the subagent tool"
        );

        let provider = agent.provider.read().await.clone();
        let child = AgentLoop::new(agent.chat.clone(), agent.tools.clone(), provider)
            .with_subagent_depth(1);
        assert!(
            child
                .tool_specs()
                .await
                .iter()
                .all(|s| s.name != crate::subagents::SUBAGENT_TOOL_NAME),
            "child must not re-delegate"
        );
    }

    /// A child run folds its findings into the `[SUBAGENT …]` summary blob.
    #[tokio::test]
    async fn spawn_subagent_returns_compact_summary() {
        let script = vec![ScriptedTurn {
            text: "child findings: X is true.",
            tool_calls: vec![],
        }];
        let (agent, sink) = build_agent(script, true).await;
        let token = agent.current_token();
        let child_sink: Arc<dyn AgentEventSink> = sink.clone();
        let summary = agent
            .spawn_subagent("research X", 5, &token, &child_sink)
            .await
            .expect("subagent succeeds");
        assert!(summary.starts_with("[SUBAGENT"), "got: {summary}");
        assert!(summary.contains("research X"), "got: {summary}");
        assert!(summary.contains("child findings"), "got: {summary}");
    }

    /// Nested delegation fails closed as an error tool result, never a
    /// grandchild loop.
    #[tokio::test]
    async fn nested_subagent_call_fails_closed() {
        let script = vec![
            ScriptedTurn {
                text: "trying to delegate",
                tool_calls: vec![(
                    crate::subagents::SUBAGENT_TOOL_NAME,
                    r#"{"goal":"nested work"}"#,
                )],
            },
            ScriptedTurn {
                text: "fine, doing it myself.",
                tool_calls: vec![],
            },
        ];
        let (agent, sink) = build_agent(script, true).await;
        let provider = agent.provider.read().await.clone();
        let child = AgentLoop::new(agent.chat.clone(), agent.tools.clone(), provider)
            .with_subagent_depth(1);
        child
            .send_user_message("go", sink.clone())
            .await
            .expect("run succeeds");
        assert!(
            sink.events().iter().any(
                |e| matches!(e, AgentEvent::ToolCallCompleted { name, output }
                    if name == crate::subagents::SUBAGENT_TOOL_NAME
                        && output.contains("nested subagents"))
            ),
            "nested call rejected as a tool error"
        );
        let history = history_json(&child.chat).await;
        assert!(
            history.contains("[TOOL_ERROR]"),
            "error marked in history; got {history}"
        );
    }

    /// End-to-end delegation: the parent's tool result is the compact summary,
    /// and raw child output never enters the parent history except via it.
    #[tokio::test]
    async fn parent_delegation_flows_summary_only() {
        let script = vec![
            ScriptedTurn {
                text: "delegating",
                tool_calls: vec![(
                    crate::subagents::SUBAGENT_TOOL_NAME,
                    r#"{"goal":"research X","budget_turns":3}"#,
                )],
            },
            // Consumed by the child as its answer (never shown to the parent
            // raw — only folded into the summary blob below).
            ScriptedTurn {
                text: "child reports Y.",
                tool_calls: vec![],
            },
            ScriptedTurn {
                text: "parent wraps up.",
                tool_calls: vec![],
            },
        ];
        let (agent, sink) = build_agent(script, true).await;
        agent
            .send_user_message("research please", sink.clone())
            .await
            .expect("run succeeds");
        let summary_output = sink
            .events()
            .iter()
            .find_map(|e| match e {
                AgentEvent::ToolCallCompleted { name, output }
                    if name == crate::subagents::SUBAGENT_TOOL_NAME =>
                {
                    Some(output.clone())
                }
                _ => None,
            })
            .expect("subagent tool result present");
        assert!(
            summary_output.starts_with("[SUBAGENT"),
            "got: {summary_output}"
        );
        assert!(
            summary_output.contains("research X"),
            "got: {summary_output}"
        );
        assert!(
            summary_output.contains("child reports Y."),
            "got: {summary_output}"
        );
        // The child's answer reaches the parent only inside the summary blob.
        let history = history_json(&agent.chat).await;
        assert_eq!(
            history.matches("child reports Y.").count(),
            1,
            "child output appears only via the summary; got {history}"
        );
    }

    // ── §5.6-E: live context window ──────────────────────────────────────────

    /// The effective window prefers the chat handle's live sampling config
    /// and tracks provider switches.
    #[tokio::test]
    async fn effective_window_prefers_live_chat_config() {
        let script = vec![ScriptedTurn {
            text: "plain answer",
            tool_calls: vec![],
        }];
        let (agent, _) = build_agent(script, true).await;
        assert_eq!(agent.effective_context_window().await, 128_000);

        let mut cfg = agent
            .chat
            .get_sampling_config()
            .await
            .expect("sampling config");
        cfg.context_window = std::num::NonZeroU64::new(200_000).unwrap();
        agent.chat.update_sampling_config(cfg);
        assert_eq!(agent.effective_context_window().await, 200_000);

        assert!(agent.sync_context_window().await, "sync succeeds");
        assert_eq!(
            agent.effective_context_window().await,
            agent.provider.read().await.context_window()
        );
    }

    // ── Long-horizon autonomy ─────────────────────────────────────────────

    /// With a spend cap set, segments are unbounded: the run continues past
    /// `max_segments` and stops only on natural finish (here: the script runs
    /// out of tool turns). No todos are seeded, proving todos are advisory —
    /// a model that never calls `todo_write` no longer ends the run after
    /// segment 1.
    #[tokio::test]
    async fn unbounded_segments_when_cap_set_without_todos() {
        // Six tool turns then a plain answer. `max_turns: 2` wraps every
        // segment on its 3rd turn, so the script spans 4 segments (3
        // boundaries) while `max_segments` is 2.
        let mut script = vec![
            ScriptedTurn {
                text: "still working",
                tool_calls: vec![(UNKNOWN_TOOL, "{}")],
            };
            6
        ];
        script.push(ScriptedTurn {
            text: "all done.",
            tool_calls: vec![],
        });
        let (agent, sink) = build_agent(script, true).await;
        let (agent, _) = attach_budget(
            agent,
            crate::config::BudgetConfig {
                max_turns: 2,
                max_segments: 2,
                spend_cap_usd: Some(1000.0),
                ..Default::default()
            },
        )
        .await;
        // No todos seeded: the todo stop gate is gone.
        agent
            .send_user_message_auto("run the scripted scenario", sink.clone())
            .await
            .expect("unbounded run must end gracefully on natural finish");

        let boundaries = sink
            .events()
            .iter()
            .filter(|e| matches!(e, AgentEvent::SegmentBoundary { .. }))
            .count();
        assert!(
            boundaries > (2 - 1),
            "run continued past max_segments = 2; got {boundaries} boundaries"
        );
        assert!(
            sink.events()
                .iter()
                .all(|e| !matches!(e, AgentEvent::Error { .. })),
            "no error events expected"
        );
    }

    /// Mid-segment compaction: when estimated context occupancy crosses the
    /// `CompactPolicy` threshold after a turn's tool results land, history is
    /// checkpointed between turns — without waiting for a segment boundary.
    #[tokio::test]
    async fn mid_segment_compaction_fires_when_threshold_crossed() {
        let script = vec![
            ScriptedTurn {
                text: "doing work",
                tool_calls: vec![(UNKNOWN_TOOL, "{}")],
            },
            ScriptedTurn {
                text: "all done.",
                tool_calls: vec![],
            },
        ];
        let (agent, sink) = build_agent(script, true).await;
        // Hair-trigger policy (1% of a 1000-token window = 10 tokens) with a
        // tiny tail so the checkpoint has something to fold.
        let (agent, _) = attach_config(
            agent,
            crate::config::ContextConfig {
                auto_compact_threshold_percent: 1,
                tail_keep_items: 2,
                ..Default::default()
            },
        )
        .await;
        let mut cfg = agent
            .chat
            .get_sampling_config()
            .await
            .expect("sampling config");
        cfg.context_window = std::num::NonZeroU64::new(1000).unwrap();
        agent.chat.update_sampling_config(cfg);

        agent
            .send_user_message_auto("run it", sink)
            .await
            .expect("run succeeds");
        let history = history_json(&agent.chat).await;
        assert!(
            history.contains(crate::compaction::COMPACTION_SUMMARY_MARKER),
            "mid-segment checkpoint must fire; got {history}"
        );
    }

    /// Provider that emits a tool call on EVERY turn, including the wrap-up
    /// turn where no tools are offered — simulating a model that hallucinates
    /// tool calls despite the no-tools contract.
    struct WrapUpHallucinatingProvider;

    #[async_trait::async_trait]
    impl Provider for WrapUpHallucinatingProvider {
        fn id(&self) -> &str {
            "hallucinating"
        }
        fn name(&self) -> &str {
            "Hallucinating"
        }
        fn kind(&self) -> ProviderKind {
            ProviderKind::Xai
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::default()
        }

        async fn complete(&self, request: ConversationRequest) -> Result<ConversationResponse> {
            let text = if request.tools.is_empty() {
                "wrapping up, but hallucinating a call"
            } else {
                "working"
            };
            let calls = vec![ToolCall {
                id: "call-h".into(),
                name: UNKNOWN_TOOL.to_string(),
                arguments: "{}".into(),
            }];
            Ok(response_for_full(text, calls, None, None))
        }
    }

    /// Hallucinated wrap-up tool calls are refused with an error tool result,
    /// never executed — the run ends gracefully instead of hitting the hard
    /// turn-ceiling `bail!`.
    #[tokio::test]
    async fn wrap_up_hallucinated_tool_calls_are_refused_not_executed() {
        let chat = crate::tools::build_chat_handle("hallucinate-test", None).expect("chat handle");
        let tools = test_bridge("hallucinate").await;
        let provider: Arc<dyn Provider> = Arc::new(WrapUpHallucinatingProvider);
        let sink = Arc::new(CollectingSink::default());
        let agent = Arc::new(AgentLoop::new(chat, tools, provider));
        let (agent, _) = attach_budget(
            agent,
            crate::config::BudgetConfig {
                max_turns: 3,
                ..Default::default()
            },
        )
        .await;
        let outcome = agent
            .send_user_message("go", sink.clone())
            .await
            .expect("wrap-up hallucination must end gracefully, not bail");
        assert!(outcome.hit_budget);
        assert!(
            outcome
                .wrap_up_text
                .as_deref()
                .unwrap_or_default()
                .contains("hallucinating"),
            "wrap-up text captured; got {:?}",
            outcome.wrap_up_text
        );

        let events = sink.events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AgentEvent::ToolCallCompleted { output, .. }
                    if output.contains("no tools available this turn"))),
            "refusal surfaced as a tool result"
        );
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, AgentEvent::Error { .. })),
            "no error events expected"
        );
        let history = history_json(&agent.chat).await;
        assert!(
            history.contains("no tools available this turn"),
            "refusal round-trips through history; got {history}"
        );
    }

    /// Child (subagent) token/cost totals merge into the parent ledger.
    #[tokio::test]
    async fn subagent_usage_merges_into_parent_ledger() {
        let script = vec![
            ScriptedTurn {
                text: "delegating",
                tool_calls: vec![(
                    crate::subagents::SUBAGENT_TOOL_NAME,
                    r#"{"goal":"research X","budget_turns":3}"#,
                )],
            },
            // Consumed by the child as its answer.
            ScriptedTurn {
                text: "child reports Y.",
                tool_calls: vec![],
            },
            ScriptedTurn {
                text: "parent wraps up.",
                tool_calls: vec![],
            },
        ];
        // 500 tokens + $0.05 per turn; parent runs 2 turns, the child 1.
        let (agent, sink) = build_agent_with_telemetry(
            script,
            true,
            Some(scripted_usage(400, 100)),
            Some(500_000_000),
        )
        .await;
        agent
            .send_user_message("research please", sink)
            .await
            .expect("run succeeds");
        let snap = agent.usage_snapshot().await;
        assert_eq!(snap.turns, 3, "2 parent turns + 1 merged child turn");
        assert_eq!(snap.total_tokens, 1500, "got {snap:?}");
        assert_eq!(snap.prompt_tokens, 1200, "got {snap:?}");
        assert_eq!(snap.completion_tokens, 300, "got {snap:?}");
        assert!(
            (snap.cost_usd - 0.15).abs() < 1e-9,
            "child cost merged; got {}",
            snap.cost_usd
        );
        assert_eq!(snap.segments, 1, "got {snap:?}");
    }

    /// The spend cap is per-message: a tripped cap aborts the current message
    /// (backstop bail) but the next message on the same session starts from a
    /// fresh ledger instead of aborting at the segment boundary.
    #[tokio::test]
    async fn spend_cap_is_per_message_not_per_session() {
        let script = vec![ScriptedTurn {
            text: "expensive answer",
            tool_calls: vec![],
        }];
        // $0.10 on the first turn against a $0.05 cap.
        let (agent, sink) = build_agent_with_telemetry(
            script,
            true,
            Some(scripted_usage(10, 10)),
            Some(1_000_000_000),
        )
        .await;
        let (agent, manager) = attach_budget(
            agent,
            crate::config::BudgetConfig {
                spend_cap_usd: Some(0.05),
                ..Default::default()
            },
        )
        .await;
        let err = agent
            .send_user_message_auto("first", sink)
            .await
            .expect_err("cap must trip on the first message");
        assert!(err.to_string().contains("spend cap exceeded"), "got: {err}");

        // Raise the cap: the second message must reflect only its own turn —
        // a stale per-session ledger would show double.
        manager
            .set_budget_config(crate::config::BudgetConfig {
                spend_cap_usd: Some(1000.0),
                ..Default::default()
            })
            .await
            .expect("raise cap");
        let sink2 = Arc::new(CollectingSink::default());
        agent
            .send_user_message_auto("second", sink2)
            .await
            .expect("second message must run on a fresh ledger");
        let snap = agent.usage_snapshot().await;
        assert_eq!(snap.total_tokens, 20, "only the second run; got {snap:?}");
        assert_eq!(snap.turns, 1, "got {snap:?}");
    }

    /// A mid-segment cap trip (the backstop bail) emits `SpendCapReached` in
    /// addition to the error event.
    #[tokio::test]
    async fn spend_cap_trip_emits_spend_cap_reached() {
        let script = vec![ScriptedTurn {
            text: "expensive answer",
            tool_calls: vec![],
        }];
        // $0.10 on the first turn against a $0.05 cap.
        let (agent, sink) = build_agent_with_telemetry(
            script,
            true,
            Some(scripted_usage(10, 10)),
            Some(1_000_000_000),
        )
        .await;
        let (agent, _) = attach_budget(
            agent,
            crate::config::BudgetConfig {
                spend_cap_usd: Some(0.05),
                ..Default::default()
            },
        )
        .await;
        agent
            .send_user_message_auto("run it", sink.clone())
            .await
            .expect_err("cap must trip");
        assert!(
            sink.events().iter().any(
                |e| matches!(e, AgentEvent::SpendCapReached { spent_usd, cap_usd }
                    if (*spent_usd - 0.10).abs() < 1e-9 && (*cap_usd - 0.05).abs() < 1e-9)
            ),
            "SpendCapReached emitted with spent/cap amounts"
        );
    }
}
