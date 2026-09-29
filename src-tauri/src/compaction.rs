//! Phase 3 context management: deterministic compaction checkpoints.
//!
//! The vendored `xai-grok-compaction` crate owns the LLM-summarization pass;
//! Maverick needs the cheaper half first — a deterministic checkpoint that
//! keeps the newest tail verbatim and folds the older prefix into one compact
//! summary item. Runs at segment boundaries (between auto-continue segments)
//! so it never races an in-flight turn.

use xai_grok_sampling_types::ConversationItem;

/// Policy resolved from [`crate::config::ContextConfig`] for one compaction
/// decision. Kept separate from the config type so tests can build it without
/// touching the config store.
#[derive(Debug, Clone)]
pub struct CompactPolicy {
    /// Master switch. When false, `decide` always returns `Keep`.
    pub enabled: bool,
    /// Percentage of the context window that triggers a checkpoint.
    pub threshold_percent: u32,
    /// Size of the session context window in tokens.
    pub context_window: u64,
    /// Newest conversation items always kept verbatim.
    pub tail_keep_items: usize,
}

/// Outcome of a compaction policy check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactDecision {
    /// History is fine as-is.
    Keep,
    /// Fold everything above the newest-tail into one summary item.
    Checkpoint,
}

impl CompactPolicy {
    /// Threshold in tokens: `context_window × threshold_percent / 100`.
    /// A zero threshold disables compaction entirely (never fires); values
    /// above 100 clamp to "window full".
    pub fn threshold_tokens(&self) -> u64 {
        if self.threshold_percent == 0 {
            return u64::MAX;
        }
        self.context_window
            .saturating_mul(self.threshold_percent.min(100) as u64)
            / 100
    }

    /// Decide whether `estimated_tokens` warrants a checkpoint.
    pub fn decide(&self, estimated_tokens: u64) -> CompactDecision {
        if !self.enabled || self.context_window == 0 {
            return CompactDecision::Keep;
        }
        if estimated_tokens >= self.threshold_tokens() {
            CompactDecision::Checkpoint
        } else {
            CompactDecision::Keep
        }
    }

    /// Mirror of [`xai_chat_state::types::AutoCompactTrigger::check`]: return
    /// the trigger (with utilization percent) when a checkpoint is warranted.
    pub fn trigger(
        &self,
        estimated_tokens: u64,
    ) -> Option<xai_chat_state::types::AutoCompactTrigger> {
        if self.decide(estimated_tokens) != CompactDecision::Checkpoint {
            return None;
        }
        xai_chat_state::types::AutoCompactTrigger::check(
            estimated_tokens,
            self.context_window,
            self.threshold_percent,
        )
    }
}

impl Default for CompactPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_percent: 85,
            context_window: 128_000,
            tail_keep_items: TAIL_KEEP_ITEMS,
        }
    }
}

impl From<&crate::config::ContextConfig> for CompactPolicy {
    fn from(cfg: &crate::config::ContextConfig) -> Self {
        Self {
            enabled: cfg.auto_compact_enabled,
            threshold_percent: cfg.auto_compact_threshold_percent,
            // The session window lives in the sampling config; 128k is the
            // Maverick default (`tools.rs`). Callers with a live session pass
            // the real window via `with_window`.
            context_window: 128_000,
            tail_keep_items: cfg.tail_keep_items.max(1),
        }
    }
}

impl CompactPolicy {
    /// Override the context window (e.g. from the live sampling config).
    pub fn with_window(mut self, window: u64) -> Self {
        self.context_window = window.max(1);
        self
    }
}

/// Conversation items newer than this are always kept verbatim.
/// (Default; the live value comes from [`CompactPolicy::tail_keep_items`].)
pub const TAIL_KEEP_ITEMS: usize = 24;

/// Estimated context occupancy (bytes/4) above which a checkpoint is taken.
/// 128k window x 85% ~= 108800 tokens. (Default; the live value comes from
/// [`CompactPolicy::threshold_tokens`].)
pub const COMPACT_THRESHOLD_TOKENS: u64 = 108_800;

/// Marker so checkpoints are recognizable in JSONL history.
pub const COMPACTION_SUMMARY_MARKER: &str = "[COMPACTION SUMMARY]";

/// Decide whether `estimated_tokens` warrants a checkpoint.
/// Legacy default-policy entry point (85% of 128k); prefer
/// [`CompactPolicy::decide`] for config-driven checks.
pub fn should_compact(estimated_tokens: u64) -> bool {
    CompactPolicy::default().decide(estimated_tokens) == CompactDecision::Checkpoint
}

/// Build the summary item text. Pure function so tests can assert the
/// auto-continue contract without a chat actor.
pub fn build_checkpoint_summary(open_todos: &[String], last_wrap_up: Option<&str>) -> String {
    let mut out = String::from(COMPACTION_SUMMARY_MARKER);
    out.push_str(" Earlier history compacted to save context. ");
    if open_todos.is_empty() {
        out.push_str("No open todos recorded. ");
    } else {
        out.push_str("Open todos:\n");
        for todo in open_todos {
            out.push_str("- ");
            out.push_str(todo);
            out.push('\n');
        }
    }
    match last_wrap_up {
        Some(text) if !text.trim().is_empty() => {
            out.push_str("Last segment summary: ");
            out.push_str(text.trim());
        }
        _ => out.push_str("No segment summary recorded."),
    }
    out
}

/// Snap `tail_start` back so the kept tail never opens on a tool result.
///
/// Cutting between an assistant message and the results of its tool calls
/// re-sends orphan tool results. The request path only repairs *dangling* tool
/// calls, never orphaned results — the shape the vendored chat-state crate
/// documents as bricking a session with provider 400s. Returns `None` when the
/// run of results reaches into the preserved head and no whole assistant
/// message is left to own them; the caller should skip the checkpoint.
fn snap_tail_start(
    history: &[ConversationItem],
    tail_start: usize,
    head_len: usize,
) -> Option<usize> {
    let mut start = tail_start;
    while matches!(
        history.get(start),
        Some(ConversationItem::ToolResult(_))
    ) {
        start = start.checked_sub(1)?;
        if start <= head_len {
            return None;
        }
    }
    Some(start)
}

/// Split `history` into `(has_system_head, head_len, tail_start)`.
///
/// The leading system item (if any) is always preserved; the newest
/// `TAIL_KEEP_ITEMS` items are kept verbatim; everything between is folded
/// into the summary. Returns `None` when there is nothing worth folding.
pub fn split_for_checkpoint(history: &[ConversationItem]) -> Option<(bool, usize, usize)> {
    let has_system_head = matches!(history.first(), Some(ConversationItem::System(_)));
    let head_len = usize::from(has_system_head);
    if history.len() <= head_len + TAIL_KEEP_ITEMS {
        return None;
    }
    let tail_start = snap_tail_start(history, history.len() - TAIL_KEEP_ITEMS, head_len)?;
    Some((has_system_head, head_len, tail_start))
}

/// Build the replacement history for a checkpoint. Keeps the system head
/// (if any) and the newest `tail_keep` items verbatim. Returns `None` when no
/// checkpoint is needed.
pub fn build_checkpoint_history_with(
    history: &[ConversationItem],
    summary_text: String,
    tail_keep: usize,
) -> Option<Vec<ConversationItem>> {
    let tail_keep = tail_keep.max(1);
    let has_system_head = matches!(history.first(), Some(ConversationItem::System(_)));
    let head_len = usize::from(has_system_head);
    if history.len() <= head_len + tail_keep {
        return None;
    }
    let tail_start = snap_tail_start(history, history.len() - tail_keep, head_len)?;
    let mut next = Vec::with_capacity(head_len + 1 + tail_keep);
    if has_system_head {
        next.push(history[0].clone());
    }
    next.push(ConversationItem::user(summary_text));
    next.extend_from_slice(&history[tail_start..]);
    Some(next)
}

/// Build the replacement history for a checkpoint. Keeps the system head
/// (if any) and the newest tail verbatim. Returns `None` when no
/// checkpoint is needed.
pub fn build_checkpoint_history(
    history: &[ConversationItem],
    summary_text: String,
) -> Option<Vec<ConversationItem>> {
    build_checkpoint_history_with(history, summary_text, TAIL_KEEP_ITEMS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> ConversationItem {
        ConversationItem::user(text.to_string())
    }

    /// Extract the text of a user item for assertions.
    fn user_text(item: &ConversationItem) -> String {
        match item {
            ConversationItem::User(u) => u
                .content
                .iter()
                .filter_map(|p| match p {
                    xai_grok_sampling_types::ContentPart::Text { text } => Some(text.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        }
    }

    #[test]
    fn threshold_fires_at_85_percent_of_128k() {
        assert!(!should_compact(COMPACT_THRESHOLD_TOKENS - 1));
        assert!(should_compact(COMPACT_THRESHOLD_TOKENS));
    }

    #[test]
    fn policy_disabled_never_fires() {
        let policy = CompactPolicy {
            enabled: false,
            ..CompactPolicy::default()
        };
        assert_eq!(policy.decide(u64::MAX), CompactDecision::Keep);
        assert!(policy.trigger(u64::MAX).is_none());
    }

    #[test]
    fn policy_threshold_math_matches_trigger_percent() {
        let policy = CompactPolicy::default();
        assert_eq!(policy.threshold_tokens(), COMPACT_THRESHOLD_TOKENS);
        let trigger = policy.trigger(COMPACT_THRESHOLD_TOKENS).unwrap();
        assert_eq!(trigger.utilization_percent, 85);
        assert_eq!(trigger.total_tokens, COMPACT_THRESHOLD_TOKENS);
        assert!(policy.trigger(COMPACT_THRESHOLD_TOKENS - 1).is_none());
    }

    /// Boundary behaviour of the vendored `AutoCompactTrigger::check` as seen
    /// through the production path (`CompactPolicy::trigger`): fires exactly at
    /// the threshold, clamps over-utilization to 100%, and never fires on a
    /// zero window (which `NonZeroU64` cannot represent).
    #[test]
    fn policy_trigger_boundaries_and_clamp() {
        let policy = CompactPolicy::default();

        assert!(policy.trigger(108_799).is_none(), "one token below 85%");
        assert!(policy.trigger(108_800).is_some(), "exactly 85%");

        let full = policy
            .trigger(500_000)
            .expect("over-utilization still fires");
        assert_eq!(full.utilization_percent, 100, "clamped, not wrapped");
        assert_eq!(full.context_window.get(), 128_000);

        let zero_window = CompactPolicy {
            context_window: 0,
            ..CompactPolicy::default()
        };
        assert!(
            zero_window.trigger(u64::MAX).is_none(),
            "zero window is unrepresentable"
        );
        assert_eq!(zero_window.decide(u64::MAX), CompactDecision::Keep);
    }

    /// A custom threshold moves both the decision and the reported percent in
    /// lockstep (config-driven compaction stays consistent).
    #[test]
    fn custom_threshold_shifts_threshold_and_percent() {
        let policy = CompactPolicy {
            threshold_percent: 50,
            context_window: 100_000,
            ..CompactPolicy::default()
        };
        assert_eq!(policy.threshold_tokens(), 50_000);
        assert!(policy.trigger(49_999).is_none());
        let t = policy
            .trigger(50_000)
            .expect("fires at the custom threshold");
        assert_eq!(t.utilization_percent, 50);
    }

    #[test]
    fn short_history_needs_no_checkpoint() {
        let history: Vec<ConversationItem> = (0..TAIL_KEEP_ITEMS)
            .map(|i| user(&format!("m{i}")))
            .collect();
        assert!(split_for_checkpoint(&history).is_none());
        assert!(build_checkpoint_history(&history, "s".to_string()).is_none());
    }

    #[test]
    fn config_context_survives_toml_round_trip_with_defaults() {
        let cfg = crate::config::MaverickConfig::default();
        assert!(cfg.context.auto_compact_enabled);
        assert_eq!(cfg.context.auto_compact_threshold_percent, 85);
        assert_eq!(cfg.context.tail_keep_items, 24);
        assert!(cfg.context.tool_output_budgets.is_empty());
        let toml = toml::to_string(&cfg).expect("serialize");
        let back: crate::config::MaverickConfig = toml::from_str(&toml).expect("deserialize");
        assert!(back.context.auto_compact_enabled);
        assert_eq!(back.context.auto_compact_threshold_percent, 85);
        // Old config files without `[context]` still load (serde default).
        let legacy: crate::config::MaverickConfig =
            toml::from_str("[ui]\ntheme = \"dark\"\n").expect("legacy deserialize");
        assert!(legacy.context.auto_compact_enabled);
    }

    #[test]
    fn checkpoint_keeps_system_head_and_tail_verbatim() {
        let mut history = vec![ConversationItem::system("sys".to_string())];
        for i in 0..(TAIL_KEEP_ITEMS + 10) {
            history.push(user(&format!("m{i}")));
        }
        let (has_head, _, _) = split_for_checkpoint(&history).unwrap();
        assert!(has_head);
        let next = build_checkpoint_history(&history, build_checkpoint_summary(&[], None)).unwrap();
        assert!(matches!(next[0], ConversationItem::System(_)));
        assert!(user_text(&next[1]).contains(COMPACTION_SUMMARY_MARKER));
        assert_eq!(next.len(), 2 + TAIL_KEEP_ITEMS);
    }

    #[test]
    fn checkpoint_never_starts_the_tail_on_a_tool_result() {
        use xai_grok_sampling_types::{AssistantItem, ToolCall};

        let mut history = vec![ConversationItem::system("sys".to_string())];
        history.push(user("run the tests"));
        history.push(ConversationItem::Assistant(AssistantItem {
            content: "running them now".into(),
            tool_calls: vec![ToolCall {
                id: "call-1".into(),
                name: "run_terminal_cmd".to_string(),
                arguments: "{}".into(),
            }],
            model_id: Some("scripted".into()),
            model_fingerprint: None,
            reasoning_effort: None,
        }));
        history.push(ConversationItem::tool_result("call-1", "all green"));

        // tail_keep = 1 lands the naive cut straight on the tool result.
        let next = build_checkpoint_history_with(&history, "summary".to_string(), 1)
            .expect("checkpoint");
        assert!(matches!(next[0], ConversationItem::System(_)));
        assert!(matches!(next[1], ConversationItem::User(_)));
        assert!(
            matches!(next[2], ConversationItem::Assistant(_)),
            "tail must be re-anchored on the assistant that owns the tool call"
        );
        assert!(matches!(next[3], ConversationItem::ToolResult(_)));
    }

    #[test]
    fn summary_preserves_auto_continue_contract() {
        let todos = vec!["finish research".to_string(), "write report".to_string()];
        let text = build_checkpoint_summary(&todos, Some("wrapped with findings"));
        assert!(text.contains(COMPACTION_SUMMARY_MARKER));
        assert!(text.contains("finish research"));
        assert!(text.contains("wrapped with findings"));
        let empty = build_checkpoint_summary(&[], None);
        assert!(empty.contains("No open todos"));
        assert!(empty.contains("No segment summary"));
    }
}
