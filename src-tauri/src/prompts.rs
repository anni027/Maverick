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
kill_task. Checkpoint progress with todo_write so a segment wrap-up can resume cleanly.";

/// Reminder appended (as a user turn) when the budget warning fires on an
/// automation-heavy segment: nudge the model toward backgrounding leftovers.
pub const AUTOMATION_BUDGET_REMINDER: &str = "If any long-running commands are still active, leave them in the \
background (do not kill them unless stray), record their task_ids in your todos, and wrap up this segment.";
