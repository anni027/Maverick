//! Per-task token & cost telemetry (§5.6-B).
//!
//! Every provider turn reports an optional [`TokenUsage`] + `cost_usd_ticks`
//! on [`ConversationResponse`](xai_grok_sampling_types::ConversationResponse).
//! [`UsageLedger`] accumulates those signals across the turns/segments of one
//! user message so the UI (and the spend guardrail) can observe them:
//!   * `AgentEvent::UsageUpdated` after every turn that reports usage,
//!   * `AgentEvent::SpendWarning` once when accumulated tokens cross 80% of
//!     the configured token budget,
//!   * an optional per-task USD spend cap that aborts further segments.
//!
//! Ticks-to-USD: [`ConversationResponse::cost_usd_ticks`] counts 1 USD = 1e10
//! ticks (see [`reported_cost_ticks`](xai_grok_sampling_types::reported_cost_ticks)).

use serde::{Deserialize, Serialize};

/// Ticks per USD (matches `ConversationResponse::cost_usd_ticks`).
pub const USD_TICKS_PER_USD: f64 = 1e10;

/// A serializable point-in-time view of a [`UsageLedger`], served to the UI
/// via the `get_usage` command.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub turns: u32,
    pub segments: u32,
}

/// Accumulates per-turn usage signals for one user message. Not `Clone` —
/// share via `Arc<Mutex<UsageLedger>>`.
#[derive(Debug, Default)]
pub struct UsageLedger {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    cost_ticks: i64,
    turns: u32,
    segments: u32,
    spend_warned: bool,
}

impl UsageLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one provider turn. `None` usage/cost (scripted providers,
    /// subprocess backends) still counts the turn so progress stays visible.
    pub fn record(
        &mut self,
        usage: Option<&xai_grok_sampling_types::TokenUsage>,
        cost_usd_ticks: Option<i64>,
    ) {
        self.turns += 1;
        if let Some(u) = usage {
            self.prompt_tokens += u.prompt_tokens as u64;
            self.completion_tokens += u.completion_tokens as u64;
            self.total_tokens += if u.total_tokens > 0 {
                u.total_tokens as u64
            } else {
                // Backends that only fill prompt/completion still contribute.
                u.prompt_tokens as u64 + u.completion_tokens as u64
            };
        }
        if let Some(t) = cost_usd_ticks.filter(|&t| t > 0) {
            self.cost_ticks = self.cost_ticks.saturating_add(t);
        }
    }

    /// Mark a new auto-continued segment (drives the `segments` counter and
    /// the per-segment usage event fields).
    pub fn begin_segment(&mut self, segment: u32) {
        self.segments = self.segments.max(segment);
    }

    /// Clear all accumulated totals. Called at the start of every
    /// `send_user_message_auto` so the spend cap is per-message, not
    /// per-session-lifetime.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Fold a child (subagent) ledger into the parent so subagent spend
    /// counts toward the parent's cap and `get_usage` snapshot. Token and
    /// cost counters sum; `segments` takes the max so a child's segment 1
    /// never clobbers the parent's count; the one-shot spend warning latches.
    pub fn merge(&mut self, other: &UsageLedger) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(other.completion_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
        self.cost_ticks = self.cost_ticks.saturating_add(other.cost_ticks);
        self.turns = self.turns.saturating_add(other.turns);
        self.segments = self.segments.max(other.segments);
        self.spend_warned = self.spend_warned || other.spend_warned;
    }

    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    pub fn cost_usd(&self) -> f64 {
        self.cost_ticks as f64 / USD_TICKS_PER_USD
    }

    /// True when `spend_cap_usd` is set and the accumulated cost exceeds it.
    pub fn exceeds_cap(&self, spend_cap_usd: Option<f64>) -> bool {
        match spend_cap_usd {
            Some(cap) if cap >= 0.0 => self.cost_usd() > cap,
            _ => false,
        }
    }

    /// One-shot 80%-of-budget check: returns the utilization percent the
    /// first time `total_tokens` crosses 80% of `budget_tokens`, `None`
    /// otherwise (including when already warned).
    pub fn check_spend_warning(&mut self, budget_tokens: u64) -> Option<u32> {
        if self.spend_warned || budget_tokens == 0 || budget_tokens == u64::MAX {
            return None;
        }
        let threshold = budget_tokens / 10 * 8;
        if self.total_tokens >= threshold {
            self.spend_warned = true;
            let percent = (self.total_tokens.min(budget_tokens) * 100 / budget_tokens) as u32;
            Some(percent.max(80))
        } else {
            None
        }
    }

    pub fn snapshot(&self) -> UsageSnapshot {
        UsageSnapshot {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            total_tokens: self.total_tokens,
            cost_usd: self.cost_usd(),
            turns: self.turns,
            segments: self.segments,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(prompt: u32, completion: u32) -> xai_grok_sampling_types::TokenUsage {
        xai_grok_sampling_types::TokenUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
            cache_creation_prompt_tokens: 0,
        }
    }

    #[test]
    fn accumulates_tokens_and_cost() {
        let mut ledger = UsageLedger::new();
        ledger.begin_segment(1);
        ledger.record(Some(&usage(100, 50)), Some(2_000_000_000));
        ledger.record(None, None);
        let snap = ledger.snapshot();
        assert_eq!(snap.prompt_tokens, 100);
        assert_eq!(snap.completion_tokens, 50);
        assert_eq!(snap.total_tokens, 150);
        assert!((snap.cost_usd - 0.2).abs() < 1e-9, "got {}", snap.cost_usd);
        assert_eq!(snap.turns, 2);
        assert_eq!(snap.segments, 1);
    }

    #[test]
    fn zero_total_falls_back_to_prompt_plus_completion() {
        let mut ledger = UsageLedger::new();
        let mut u = usage(30, 20);
        u.total_tokens = 0;
        ledger.record(Some(&u), None);
        assert_eq!(ledger.snapshot().total_tokens, 50);
    }

    #[test]
    fn nonpositive_cost_ticks_are_ignored() {
        let mut ledger = UsageLedger::new();
        ledger.record(Some(&usage(10, 10)), Some(0));
        ledger.record(Some(&usage(10, 10)), Some(-5));
        assert_eq!(ledger.snapshot().cost_usd, 0.0);
    }

    #[test]
    fn spend_warning_fires_once_at_80_percent() {
        let mut ledger = UsageLedger::new();
        // Budget 1000 → threshold 800.
        assert_eq!(ledger.check_spend_warning(1000), None);
        ledger.record(Some(&usage(799, 0)), None);
        assert_eq!(ledger.check_spend_warning(1000), None);
        ledger.record(Some(&usage(1, 0)), None);
        assert_eq!(ledger.check_spend_warning(1000), Some(80));
        ledger.record(Some(&usage(200, 0)), None);
        assert_eq!(ledger.check_spend_warning(1000), None, "one-shot");
    }

    #[test]
    fn spend_cap_comparison() {
        let mut ledger = UsageLedger::new();
        assert!(!ledger.exceeds_cap(None));
        assert!(!ledger.exceeds_cap(Some(1.0)));
        ledger.record(None, Some(11_000_000_000)); // $1.10
        assert!(ledger.exceeds_cap(Some(1.0)));
        assert!(!ledger.exceeds_cap(Some(2.0)));
    }

    #[test]
    fn reset_clears_totals_and_warning_latch() {
        let mut ledger = UsageLedger::new();
        ledger.begin_segment(2);
        ledger.record(Some(&usage(900, 100)), Some(1_000_000_000));
        assert_eq!(ledger.check_spend_warning(1000), Some(100));
        ledger.reset();
        let snap = ledger.snapshot();
        assert_eq!(snap, UsageSnapshot::default());
        // The one-shot warning latch is cleared too: a fresh ledger warns.
        ledger.record(Some(&usage(800, 0)), None);
        assert_eq!(ledger.check_spend_warning(1000), Some(80));
        assert!(!ledger.exceeds_cap(Some(0.5)));
    }

    #[test]
    fn merge_sums_child_into_parent() {
        let mut parent = UsageLedger::new();
        parent.begin_segment(3);
        parent.record(Some(&usage(100, 50)), Some(2_000_000_000));
        let mut child = UsageLedger::new();
        child.begin_segment(1);
        child.record(Some(&usage(200, 100)), Some(3_000_000_000));
        parent.merge(&child);
        let snap = parent.snapshot();
        assert_eq!(snap.prompt_tokens, 300);
        assert_eq!(snap.completion_tokens, 150);
        assert_eq!(snap.total_tokens, 450);
        assert!((snap.cost_usd - 0.5).abs() < 1e-9, "got {}", snap.cost_usd);
        assert_eq!(snap.turns, 2);
        // The child's segment 1 must not clobber the parent's segment count.
        assert_eq!(snap.segments, 3);
    }
}
