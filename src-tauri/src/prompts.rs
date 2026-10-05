//! Phase 4 automation guidance injected for background-capable sessions.
//!
//! The loop itself stays provider-agnostic; this module only owns the prompt
//! text so `AgentLoop` and tests share one definition. Long-running work must
//! prefer `run_terminal_cmd` with `background: true`, then poll the returned
//! `task_id` with `get_task_output` (bounded waits) instead of blocking the
//! turn budget on a foreground `sleep` or a hanging command.

/// System-head addendum for automation sessions.
///
/// The host sets it once via `ChatStateHandle::replace_system_head` (or at
/// session creation); the loop never re-injects it mid-segment.
pub const AUTOMATION_SYSTEM_ADDENDUM: &str = "AUTOMATION RULES: for any command that may run longer than ~60s, \
start it with run_terminal_cmd in background mode and note the returned task_id. Poll with get_task_output using \
short bounded waits; never block a turn on sleep or a foreground long-running command. Kill stray jobs with \
kill_task. Checkpoint progress with todo_write so a segment wrap-up can resume cleanly. BRIEF FIRST: for any \
vague request — files, pages, docs, code, research, commands, anything — ask questions BEFORE acting. No \
state-changing tool calls until the gaps are closed. Bundle every independent question into ONE ask_user call \
(up to 5); go sequential across rounds only when later questions depend on earlier answers. Ask about \
everything, including small details (style, \
length, naming, format): trivial unknowns get questions, not silent defaults. Look up facts yourself (read \
files, docs, code); ask the user about intent, taste, and decisions. Follow-up rounds are normal when answers \
open new dimensions — stopping while dimensions are still open is a failure. Quick \
read-only recon (listing files, reading a file to shape options) is allowed while briefing. Never fill a \
requested artifact with invented or placeholder content unless the user said 'surprise me' or 'just an example'.";

/// Reminder appended (as a user turn) when the budget warning fires on an
/// automation-heavy segment: nudge the model toward backgrounding leftovers.
pub const AUTOMATION_BUDGET_REMINDER: &str = "If any long-running commands are still active, leave them in the \
background (do not kill them unless stray), record their task_ids in your todos, and wrap up this segment.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addendum_demands_brief_first_and_bans_silent_invention() {
        assert!(AUTOMATION_SYSTEM_ADDENDUM.contains("BRIEF FIRST"));
        assert!(AUTOMATION_SYSTEM_ADDENDUM.contains("including small details"));
        assert!(AUTOMATION_SYSTEM_ADDENDUM.contains("surprise me"));
        // The old "decide it yourself" license for guessing is gone.
        assert!(!AUTOMATION_SYSTEM_ADDENDUM.contains("reasonably decide yourself"));
    }
}
