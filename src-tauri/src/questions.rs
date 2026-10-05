//! Clarifying questions (`ask_user` tool): when something is unclear, the
//! model asks the user instead of guessing. The UI renders the options, the
//! answer arrives via the `answer_question` command, and it re-enters history
//! as the tool result — like Claude Code / OpenCode question flows.
//!
//! Safety rails: the tool is offered on interactive parent loops only
//! (headless runs and subagent children fail closed), a global
//! `[interaction]` switch can remove it entirely, and a per-run budget that
//! scales with the run's segment budget stops ask-loops from burning money.

use crate::agent_event::QuestionOption;

/// Client-facing tool name for clarifying questions.
pub const ASK_USER_TOOL_NAME: &str = "ask_user";
/// Max questions bundled in one call (Claude-style stepper).
pub const ASK_USER_MAX_BATCH: usize = 5;
/// Question text is truncated to this (a question should fit on a card).
pub const ASK_USER_MAX_QUESTION_CHARS: usize = 500;
/// Min/max options per question (Claude-style: 2–4 real choices).
pub const ASK_USER_MIN_OPTIONS: usize = 2;
pub const ASK_USER_MAX_OPTIONS: usize = 4;
pub const ASK_USER_MAX_LABEL_CHARS: usize = 120;
pub const ASK_USER_MAX_DESC_CHARS: usize = 300;
/// User answers longer than this are truncated before entering history.
pub const ASK_USER_MAX_ANSWER_CHARS: usize = 4000;

/// Dynamic per-run question budget: scales with the run's segment budget so
/// long tasks may clarify more often (default 3 segments → 6 questions).
/// Never below 2.
pub fn max_questions_per_run(effective_max_segments: u32) -> u32 {
    (2 * effective_max_segments.max(1)).max(2)
}

/// Parsed `ask_user` arguments.
#[derive(Debug, Clone)]
pub struct AskUserArgs {
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub allow_custom: bool,
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max.min(s.len());
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s[..cut].to_string()
}

pub fn parse_ask_args(args: &serde_json::Value) -> Result<AskUserArgs, String> {
    let question = args
        .get("question")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Missing required parameter `question` (non-empty string)".to_string())?;
    let raw_options = args
        .get("options")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            "Missing required parameter `options` (array of 2-4 {label, description} objects)"
                .to_string()
        })?;
    if raw_options.len() < ASK_USER_MIN_OPTIONS || raw_options.len() > ASK_USER_MAX_OPTIONS {
        return Err(format!(
            "`options` must hold {ASK_USER_MIN_OPTIONS}-{ASK_USER_MAX_OPTIONS} choices (got {})",
            raw_options.len()
        ));
    }
    let mut options = Vec::with_capacity(raw_options.len());
    for (i, opt) in raw_options.iter().enumerate() {
        let label = opt
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("`options[{i}]` needs a non-empty `label`"))?;
        let description = opt
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        options.push(QuestionOption {
            label: truncate_chars(label, ASK_USER_MAX_LABEL_CHARS),
            description: truncate_chars(&description, ASK_USER_MAX_DESC_CHARS),
        });
    }
    Ok(AskUserArgs {
        question: truncate_chars(question, ASK_USER_MAX_QUESTION_CHARS),
        options,
        allow_custom: args
            .get("allow_custom")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    })
}

/// [`ToolSpec`] advertising `ask_user` to the model.
pub fn ask_user_tool_spec() -> xai_grok_sampling_types::ToolSpec {
    xai_grok_sampling_types::ToolSpec {
        name: ASK_USER_TOOL_NAME.to_string(),
        description: Some(
            "Ask the user clarifying questions when something is genuinely unclear instead of \
             guessing. Bundle every INDEPENDENT question into ONE call (up to 5) — the user steps \
             through them like a form. Go sequential across rounds only when later questions depend \
             on earlier answers. Use when requirements are ambiguous, a choice is hard to reverse \
             or costs real money/time, or several valid interpretations exist. Look up facts \
             yourself (read files, docs, code); ask about intent, taste, and decisions — including \
             small details like style, length, and format. When the request is vague, brief first: \
             ask several rounds before acting, and never invent core content silently. Each question \
             provides 2-4 options with honest one-line descriptions of the tradeoffs; the user may \
             also answer in free text or skip questions (then proceed with your best judgment)."
                .to_string(),
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": ASK_USER_MAX_BATCH,
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {
                                "type": "string",
                                "description": "The clarifying question, e.g. 'Which database should this use?'"
                            },
                            "options": {
                                "type": "array",
                                "minItems": 2,
                                "maxItems": 4,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": { "type": "string", "description": "Short choice name" },
                                        "description": { "type": "string", "description": "One-line tradeoff of this choice" }
                                    },
                                    "required": ["label"]
                                }
                            },
                            "allow_custom": {
                                "type": "boolean",
                                "description": "Allow a free-text answer (default true)"
                            }
                        },
                        "required": ["question", "options"]
                    }
                }
            },
            "required": ["questions"]
        }),
    }
}

/// Parsed `ask_user` call: a batch of 1–5 independent questions.
#[derive(Debug, Clone)]
pub struct AskUserBatch {
    pub questions: Vec<AskUserArgs>,
}

/// Parse a batched `ask_user` call. Each item follows the single-question
/// shape validated by [`parse_ask_args`].
pub fn parse_ask_batch(args: &serde_json::Value) -> Result<AskUserBatch, String> {
    let raw = args
        .get("questions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            "Missing required parameter `questions` (array of 1-5 {question, options} objects)"
                .to_string()
        })?;
    if raw.is_empty() || raw.len() > ASK_USER_MAX_BATCH {
        return Err(format!(
            "`questions` must hold 1-{ASK_USER_MAX_BATCH} questions (got {})",
            raw.len()
        ));
    }
    let mut questions = Vec::with_capacity(raw.len());
    for (i, item) in raw.iter().enumerate() {
        parse_ask_args(item)
            .map_err(|e| format!("`questions[{i}]`: {e}"))
            .map(|q| questions.push(q))?;
    }
    Ok(AskUserBatch { questions })
}

/// Render the batched tool result back to the model: one Q/A pair per line.
/// Empty answers are per-question skips (the user stepped past them).
pub fn format_batch_result(items: &[(&str, &str)]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(i, (q, a))| {
            let n = i + 1;
            let a = a.trim();
            if a.is_empty() {
                format!("Q{n}: {q}\nA{n}: (skipped — use best judgment for this one)")
            } else {
                format!("Q{n}: {q}\nA{n}: {a}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_event::PendingQuestion;

    fn valid_args() -> serde_json::Value {
        serde_json::json!({
            "question": "Which DB?",
            "options": [
                {"label": "SQLite", "description": "zero-config, local"},
                {"label": "Postgres", "description": "needs a server"}
            ]
        })
    }

    #[test]
    fn parses_valid_args_with_defaults() {
        let args = parse_ask_args(&valid_args()).unwrap();
        assert_eq!(args.question, "Which DB?");
        assert_eq!(args.options.len(), 2);
        assert_eq!(args.options[0].label, "SQLite");
        assert!(args.allow_custom);
    }

    #[test]
    fn rejects_bad_shapes() {
        assert!(parse_ask_args(&serde_json::json!({})).is_err());
        assert!(parse_ask_args(&serde_json::json!({"question": "  "})).is_err());
        // One option is not a real choice.
        assert!(parse_ask_args(&serde_json::json!({
            "question": "Q?",
            "options": [{"label": "only"}]
        }))
        .is_err());
        // Five options is a survey, not a question.
        let many: Vec<_> = (0..5)
            .map(|i| serde_json::json!({"label": format!("o{i}")}))
            .collect();
        assert!(parse_ask_args(&serde_json::json!({"question": "Q?", "options": many})).is_err());
        // Empty label rejected.
        assert!(parse_ask_args(&serde_json::json!({
            "question": "Q?",
            "options": [{"label": "a"}, {"label": "  "}]
        }))
        .is_err());
    }

    #[test]
    fn truncates_long_fields() {
        let long = "x".repeat(2000);
        let args = parse_ask_args(&serde_json::json!({
            "question": long,
            "options": [{"label": long, "description": long}, {"label": "b"}]
        }))
        .unwrap();
        assert_eq!(args.question.len(), ASK_USER_MAX_QUESTION_CHARS);
        assert_eq!(args.options[0].label.len(), ASK_USER_MAX_LABEL_CHARS);
        assert_eq!(args.options[0].description.len(), ASK_USER_MAX_DESC_CHARS);
    }

    #[test]
    fn dynamic_cap_scales_with_segments() {
        assert_eq!(max_questions_per_run(1), 2);
        assert_eq!(max_questions_per_run(3), 6);
        assert_eq!(max_questions_per_run(0), 2);
        assert_eq!(max_questions_per_run(10), 20);
    }

    #[test]
    fn spec_shape_matches_contract() {
        let spec = ask_user_tool_spec();
        assert_eq!(spec.name, ASK_USER_TOOL_NAME);
        let desc = spec.description.as_deref().unwrap_or_default();
        assert!(desc.contains("clarifying question"));
        assert!(desc.contains("brief first"));
    }

    fn batch_item(q: &str) -> serde_json::Value {
        serde_json::json!({
            "question": q,
            "options": [{"label": "a"}, {"label": "b"}]
        })
    }

    #[test]
    fn parses_valid_batch() {
        let batch = parse_ask_batch(&serde_json::json!({
            "questions": [batch_item("Q1?"), batch_item("Q2?"), batch_item("Q3?")]
        }))
        .unwrap();
        assert_eq!(batch.questions.len(), 3);
        assert_eq!(batch.questions[0].question, "Q1?");
    }

    #[test]
    fn rejects_bad_batches() {
        assert!(parse_ask_batch(&serde_json::json!({})).is_err());
        assert!(parse_ask_batch(&serde_json::json!({"questions": []})).is_err());
        let many: Vec<_> = (0..6).map(|i| batch_item(&format!("Q{i}"))).collect();
        assert!(parse_ask_batch(&serde_json::json!({"questions": many})).is_err());
        // Per-item failures name the index.
        let err = parse_ask_batch(&serde_json::json!({
            "questions": [batch_item("ok?"), {"question": "bad?"}]
        }))
        .unwrap_err();
        assert!(err.contains("questions[1]"), "got: {err}");
    }

    #[test]
    fn formats_mixed_answered_and_skipped() {
        let out = format_batch_result(&[("Q1?", "SQLite"), ("Q2?", "  ")]);
        assert!(out.contains("Q1: Q1?"));
        assert!(out.contains("A1: SQLite"));
        assert!(out.contains("A2: (skipped — use best judgment for this one)"));
    }

    #[test]
    fn pending_batch_round_trips_to_event_json() {
        let q = PendingQuestion {
            id: "q-1-1".to_string(),
            question: "Q?".to_string(),
            options: vec![QuestionOption {
                label: "a".to_string(),
                description: "b".to_string(),
            }],
            allow_custom: true,
        };
        let batch = crate::agent_event::PendingBatch {
            id: "q-1".to_string(),
            questions: vec![q],
        };
        let event = crate::agent_event::AgentEvent::QuestionAsked {
            id: batch.id.clone(),
            questions: batch.questions.clone(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "QuestionAsked");
        assert_eq!(json["id"], "q-1");
        assert_eq!(json["questions"][0]["options"][0]["label"], "a");
    }
}
