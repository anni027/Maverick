//! Live subagents (§5.6-D).
//!
//! The parent loop offers a `subagent` tool (parent depth only). Calling it
//! spawns a child [`crate::AgentLoop`] with a fresh in-memory context and a
//! turn budget clamped to [`SUBAGENT_BUDGET_TURNS`]; the child runs a single
//! segment and its findings come back as a compact
//! [`render_subagent_summary`] blob. The parent context never ingests raw
//! subtask output — only the summary tool result.
//!
//! Nesting is capped at one level: child loops (depth ≥ 1) are not offered
//! the tool, and a hallucinated nested call fails closed.

/// Maximum tool-call turns a subtask description may claim.
pub const SUBAGENT_BUDGET_TURNS: u32 = 10;

/// Client-facing tool name for delegation.
pub const SUBAGENT_TOOL_NAME: &str = "subagent";

/// Maximum delegation depth. Parent runs at 0; children run at 1 and cannot
/// delegate further.
pub const SUBAGENT_MAX_DEPTH: u32 = 1;

/// A subtask the orchestrator wants to delegate.
#[derive(Debug, Clone)]
pub struct SubagentTask {
    /// Short goal, e.g. "research X".
    pub goal: String,
    /// Tool-call turn budget for the child (capped at [`SUBAGENT_BUDGET_TURNS`]).
    pub budget_turns: u32,
}

impl SubagentTask {
    pub fn new(goal: impl Into<String>, budget_turns: u32) -> Self {
        Self {
            goal: goal.into(),
            budget_turns: budget_turns.clamp(1, SUBAGENT_BUDGET_TURNS),
        }
    }
}

/// Render the compact summary a child would return to the parent.
/// Parent context sees only this, never raw subtask output.
pub fn render_subagent_summary(task: &SubagentTask, findings: &str) -> String {
    format!(
        "[SUBAGENT goal=\"{}\" budget={}] {}",
        task.goal,
        task.budget_turns,
        findings.trim()
    )
}

/// Tool arguments for the `subagent` tool.
#[derive(Debug, Clone)]
pub struct SubagentArgs {
    pub goal: String,
    pub budget_turns: u32,
}

/// Parse `subagent` tool arguments. Rejects empty goals so a confused model
/// cannot burn a child budget on nothing.
pub fn parse_subagent_args(args: &serde_json::Value) -> Result<SubagentArgs, String> {
    let goal = args
        .get("goal")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Missing required parameter `goal` (non-empty string)".to_string())?;
    let budget_turns = args
        .get("budget_turns")
        .and_then(|v| v.as_u64())
        .map(|n| n.min(u64::from(u32::MAX)) as u32)
        .unwrap_or(SUBAGENT_BUDGET_TURNS);
    Ok(SubagentArgs {
        goal: goal.to_string(),
        budget_turns: budget_turns.clamp(1, SUBAGENT_BUDGET_TURNS),
    })
}

/// [`ToolSpec`] advertised to the parent model for delegation.
pub fn subagent_tool_spec() -> xai_grok_sampling_types::ToolSpec {
    xai_grok_sampling_types::ToolSpec {
        name: SUBAGENT_TOOL_NAME.to_string(),
        description: Some(
            "Delegate a self-contained research subtask to a child agent with a fresh context. \
             Prefer this over dumping large outputs into this conversation: describe the goal, \
             give a small turn budget (max 10), and you will receive a compact summary of findings. \
             The subtask cannot call back into subagents."
                .to_string(),
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "Self-contained goal for the child, e.g. 'research X and list sources'"
                },
                "budget_turns": {
                    "type": "integer",
                    "description": "Child turn budget, 1-10 (default 10)",
                    "minimum": 1,
                    "maximum": 10
                }
            },
            "required": ["goal"]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_is_clamped() {
        assert_eq!(SubagentTask::new("r", 0).budget_turns, 1);
        assert_eq!(
            SubagentTask::new("r", 99).budget_turns,
            SUBAGENT_BUDGET_TURNS
        );
    }

    #[test]
    fn summary_carries_goal_and_findings() {
        let task = SubagentTask::new("research X", 5);
        let summary = render_subagent_summary(&task, "found Y");
        assert!(summary.contains("research X"));
        assert!(summary.contains("found Y"));
        assert!(summary.starts_with("[SUBAGENT"));
    }

    #[test]
    fn args_require_nonempty_goal_and_clamp_budget() {
        let args = parse_subagent_args(&serde_json::json!({"goal": "  do X  "})).unwrap();
        assert_eq!(args.goal, "do X");
        assert_eq!(args.budget_turns, SUBAGENT_BUDGET_TURNS);

        let args =
            parse_subagent_args(&serde_json::json!({"goal": "do X", "budget_turns": 99})).unwrap();
        assert_eq!(args.budget_turns, SUBAGENT_BUDGET_TURNS);

        let args =
            parse_subagent_args(&serde_json::json!({"goal": "do X", "budget_turns": 0})).unwrap();
        assert_eq!(args.budget_turns, 1);

        assert!(parse_subagent_args(&serde_json::json!({})).is_err());
        assert!(parse_subagent_args(&serde_json::json!({"goal": "   "})).is_err());
    }

    #[test]
    fn spec_shape_matches_contract() {
        let spec = subagent_tool_spec();
        assert_eq!(spec.name, SUBAGENT_TOOL_NAME);
        assert!(
            spec.description
                .as_deref()
                .unwrap_or_default()
                .contains("Delegate")
        );
    }
}
