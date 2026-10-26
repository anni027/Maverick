# Ask-User / Clarification Feature

## Goal
When the agent's instructions are ambiguous, pause the run and ask the user
questions — inspired by Claude Code, Claude Web, OpenCode, and Kilo Code.

## How the reference tools do it

| Tool | Mechanism |
|------|-----------|
| **Claude Code** | `AskUserQuestion` tool — structured questions with multiple choice options. Agent pauses, waits for answer, resumes. |
| **Claude Web** | Same `AskUserQuestion` tool, rendered as a card in the chat. |
| **OpenCode** | `question` permission + `part.ask` ACP events. The agent emits a `part.ask` with the question; the client must respond with `part.answer` before the turn continues. |
| **Kilo Code** | `ask_user` tool — the agent calls a tool named `ask_user` with a `question` string and optional `options` array. The tool result is the user's answer. |

## Chosen approach: Kilo Code style (tool-based)

Kilo Code's approach is the simplest to integrate: the agent calls a tool
named `ask_user`, the tool pauses execution until the user responds, and the
tool result (the user's answer) is fed back into the conversation. This keeps
the existing event loop intact — no new top-level events needed, just a tool
that blocks.

### Backend changes

1. **New tool: `ask_user`** (`tools.rs` or a new module)
   - Signature: `ask_user(question: string, options?: string[])`
   - Emits `AgentEvent::ToolCallStarted { name: "ask_user", args: { question, options } }`
   - Waits for a user response via a `tokio::sync::oneshot` channel
   - Returns the user's answer as the tool result
   - Timeout: 5 minutes, then returns an error so the run doesn't hang forever

2. **New Tauri command: `respond_to_question`**
   - `respond_to_question(sessionId, answer)`
   - Resolves the pending oneshot for that session
   - No return value

3. **New `AgentEvent::QuestionAsked`** (optional, for UI signaling)
   - `question: string`, `options: Option<string[]>`, `askedAt: number`
   - Emitted when the tool is called, so the UI can show a question card

### Frontend changes

1. **Question card** (new component `QuestionCard.tsx`)
   - Shows the question text
   - Shows options as buttons if provided
   - Free-text input if no options
   - "Send" button to submit the answer
   - Disables while waiting for the run to be cancelled

2. **Chat.tsx wiring**
   - Listen for `QuestionAsked` event → render the card in the message list
   - `respond_to_question` invoke on submit

3. **Cancel handling**
   - If the run is cancelled while a question is pending, the oneshot resolves with an error and the tool returns "cancelled"

### Agent loop changes

- The `ask_user` tool is added to the tool spec list
- When the model calls it, the loop pauses (the tool's execution blocks on the oneshot)
- The tool result (user answer) is fed back as a tool result message
- The loop continues normally

## Files to modify

| File | Change |
|------|--------|
| `nexus/src-tauri/src/tools.rs` | Add `ask_user` tool implementation |
| `nexus/src-tauri/src/agent_event.rs` | Add `QuestionAsked` event |
| `nexus/src-tauri/src/commands.rs` | Add `respond_to_question` command |
| `nexus/src-tauri/src/agent_loop.rs` | Wire the tool into the spec list |
| `nexus/frontend/src/components/QuestionCard.tsx` | New component |
| `nexus/frontend/src/components/Chat.tsx` | Wire the event + send button |
| `nexus/frontend/src/types/index.ts` | Add `QuestionAsked` to `AgentEventType` |

## Open questions — RESOLVED

1. **Should the agent auto-detect ambiguity, or should the user opt in?**
   → **Agent decides** — the agent detects ambiguity and calls `ask_user` automatically. More useful, matches Claude Code / Kilo Code.

2. **Should there be a max questions per run?**
   → **5 per run, configurable** in Settings → Context → Ask-User budget. Hard limit prevents infinite loops; configurable lets power users raise it.

3. **Should the question be shown in the sidebar or inline?**
   → **Above the prompt bar** — inline in the chat, positioned just above the composer dock. Simpler than a sidebar, more visible than mid-stream.

4. **Timeout behavior?**
   → **5 minutes, then auto-resume** with "user did not respond" as the answer. The run continues so the agent can work around the missing answer.

5. **Should the user be able to type a free-form answer even with options?**
   → **Yes, always** — a text input is always available as a fallback, even when options are provided. The user can override any option with free text.

## Validation

- [ ] Backend compiles
- [ ] Frontend builds
- [ ] Ask a question → card appears → answer → run continues
- [ ] Options work → selecting one fills the answer
- [ ] Free-text works → typing and sending works
- [ ] Cancel while waiting → run stops gracefully
- [ ] Timeout → run continues with "no response" answer
- [ ] Max questions per run → 6th question is rejected