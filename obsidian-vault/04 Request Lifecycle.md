---
tags:
  - runtime
  - agent-loop
  - workflow
aliases:
  - Message lifecycle
  - Agent request flow
---

# Request lifecycle

## Entry points

The Tauri process resolves Tauri's per-app data directory, creates `AppState` under it, installs the shell plugin, and registers commands for sessions, providers, configuration, MCP, skills, usage, and messages. See [nexus/src-tauri/src/main.rs](../nexus/src-tauri/src/main.rs#L1-L68).

The React app initializes sessions, providers, tools, UI preferences, and the selected session in [nexus/frontend/src/App.tsx](../nexus/frontend/src/App.tsx#L41-L105).

## Session initialization

`init_session` resolves the provider in this order:

1. Explicit provider ID.
2. Configured default provider.
3. First registered provider.

It loads or creates the session chat handle, aligns the sampling context window with the provider, constructs an `AgentLoop`, and stores it in `AppState`. See [commands.rs](../nexus/src-tauri/src/commands.rs#L173-L240).

"First registered" is stable: the registry keeps an explicit insertion order rather than relying on `HashMap` iteration, so the same fallback provider is chosen on every run. See [registry.rs](../nexus/src-tauri/src/providers/registry.rs#L15-L58).

## Sending a message

`send_message` looks up the session loop, creates a Tauri event sink, and awaits `send_user_message_auto`. See [commands.rs](../nexus/src-tauri/src/commands.rs#L242-L266).

The frontend adds the user message optimistically, listens for `agent-event`, and reloads persisted messages after a turn completes. See [Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L49-L139) and [Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L192-L332).

## One segment

A segment is bounded by a configurable turn budget. The loop:

1. Pushes the user prompt.
2. Builds a request with tool definitions.
3. Emits `TurnStarted`.
4. Calls the provider, with cancellation racing the provider future.
5. Streams assistant text.
6. Records assistant history.
7. Executes each tool call.
8. Truncates tool output, marks errors, emits completion events, and stores results.
9. Emits `TurnCompleted`.

The provider call and cancellation race are visible in [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L355-L455). Tool dispatch and output shaping are at [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L635-L700).

## Budget behavior

Defaults are 40 tool-call turns per segment and up to 3 auto-continued segments (`MAX_TURNS` / `MAX_SEGMENTS`, both `pub(crate)` in `agent_loop.rs` and mirrored as serde defaults in `BudgetConfig`). At the warning turn (`max_turns - 3`, saturating so tiny budgets warn on turn 1), the model receives a prompt to checkpoint and wrap up. At the wrap-up turn (`max_turns + 1`), no tools are offered and the model is forced toward a text summary. A hard ceiling (`max_turns + 2`) is a last resort if a model emits tool calls despite receiving none.

The segmented runner (`send_user_message_auto`) continues only when:

- the prior segment hit its budget,
- auto-continue is enabled (else `max_segments` collapses to 1),
- the segment cap is not exhausted — **or**, when a `spend_cap_usd` is set, segments are unbounded and stop only on natural finish, the identical-wrap-up guard, the cap, or cancel. Without a cap, `max_segments` (default 3) stays the safety limit; an absolute backstop of 500 segments applies when a cap is set but the provider never reports cost.
- consecutive wrap-ups are not identical (broken-loop guard),
- the spend cap has not been exceeded (graceful `SpendCapReached` at a segment boundary; mid-segment trips still `bail!` as a backstop),
- the user has not cancelled.

**Open todos are advisory, not a stop gate.** They are re-injected as a system reminder at segment starts and embedded in compaction summaries, but a model that never calls `todo_write` no longer ends the run after segment 1.

See [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L883-L985) and [config.rs](../nexus/src-tauri/src/config.rs#L86-L173).

## Compaction between segments

At a segment boundary, Nexus can fold older history into one `[COMPACTION SUMMARY]` item while preserving the system head and newest tail. Open todos are re-injected as a system reminder. The policy defaults to 85% of the live context window with 24 tail items. See [compaction.rs](../nexus/src-tauri/src/compaction.rs#L1-L20) and [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L860-L882).

## Subagents

Parent loops may expose a `subagent` tool. A child loop receives a fresh in-memory context, a budget clamped to 10 turns, and cannot delegate further. Only the compact summary returns to the parent; the child's thinking and tool-call events are mirrored to the parent UI. See [subagents.rs](../nexus/src-tauri/src/subagents.rs#L1-L21) and [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L701-L761).

## Events consumed by the UI

The frontend handles assistant text, tool-call start/completion, turn completion, segment boundaries, spend warnings, cancellation, and errors — and only for the session that currently owns the run. See [Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L192-L332).

## Related notes

- [[03 Nexus Architecture]]
- [[05 Providers and MCP]]
- [[06 Tools and Permissions]]
- [[11 Current Work and Risks]]
