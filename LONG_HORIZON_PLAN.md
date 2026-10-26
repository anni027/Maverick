# Long-Horizon Autonomy Upgrade — Execution Plan

> **Status:** implemented 2026-09-23 — all 4 changes + 8 new tests green (63 passed), no new clippy warnings.
> **Scope:** `nexus\src-tauri\src` — make one user message capable of driving long coding runs
> (OpenCode-style: hours, many sequential tool-call iterations) instead of stopping after
> ~3 segments / ~123 turns.

## Problem (from the 2026-09-22 review)

Current behavior in `agent_loop.rs::send_user_message_auto`:

- Hard stop at `[budget].max_segments` (default 3) × `max_turns` (default 40) ≈ 123 model turns.
- `has_open_todos()` is a *stop gate*: a model that never calls `todo_write` silently ends the
  run after segment 1 even with work remaining.
- Compaction only runs **between** segments — 40 turns × 12 KB tool outputs can overflow the
  context window **mid-segment** → provider 400 → run dead.
- Wrap-up turn (no tools offered): tool calls emitted anyway are still **executed**, then the
  run hard-bails at the ceiling ("hard turn ceiling reached").
- Spend cap is per-session-lifetime: once tripped, every future message on that session aborts.
- Subagent child usage is invisible to the parent's ledger/cap (`spawn_subagent` creates a
  fresh `UsageLedger`).

## Changes

### 1. Unbounded segmentation, gated by spend cap — `agent_loop.rs::send_user_message_auto`
- When `[budget].spend_cap_usd` is **set**: segments are unbounded — stop only on natural
  finish, identical-wrap-up guard, spend cap, or cancel.
- When **no cap** is set: keep `max_segments` as the safety limit (never burn unbounded money
  by default).
- Drop `has_open_todos()` as a stop gate — todos become advisory (still re-injected at segment
  starts and embedded in compaction summaries). Keep the identical-wrap-up broken-loop guard.
- Reset the usage ledger at the start of each `send_user_message_auto` (add
  `UsageLedger::reset()` in `usage.rs`) so the spend cap is per-message, not per-session.
- Cap trip at a segment boundary ends gracefully (emit event, not mid-run `bail!`); keep the
  existing hard bail only as backstop.

### 2. Mid-segment compaction — `agent_loop.rs::run_segment` (+ reuse `maybe_compact`)
- After pushing each tool result, check `chat.get_estimated_total_tokens()` against the
  existing `CompactPolicy` threshold and run the checkpoint when ≥ threshold.
- Only between turns (never mid-turn) so it can't race an in-flight provider call.

### 3. Wrap-up-turn hardening — `agent_loop.rs::run_segment`
- On the wrap-up turn, **do not execute** tool calls emitted despite none being offered; push
  an error tool result ("no tools available this turn; answer in text") instead. Converts the
  ceiling `bail!` into a recoverable path.

### 4. Subagent usage merge — `agent_loop.rs::spawn_subagent` + `usage.rs`
- Add `UsageLedger::merge(&other)`; after the child segment, merge child totals into the
  parent ledger so subagent spend counts toward the parent's cap and `get_usage`.

## Files touched
| File | Change |
|---|---|
| `agent_loop.rs` | runner loop, wrap-up handling, mid-segment compaction call, child usage merge (~80 lines) |
| `usage.rs` | `reset()`, `merge()` (~20 lines) |
| `agent_event.rs` | optional `SpendCapReached { spent_usd, cap_usd }` variant + sink arms (~10 lines) |
| `config.rs` | no schema change; update doc comment on `max_segments` semantics |

## Tests (scripted provider, existing patterns: `ScriptedProvider`, `build_agent_with_telemetry`)
- Unbounded segments when cap set; segments capped when no cap.
- No-todos no longer stops the run.
- Mid-segment compaction fires when threshold crossed.
- Wrap-up hallucinated tool calls not executed; run recovers.
- Child usage merged into parent ledger; ledger reset per run.
- Cap stop emits `SpendCapReached`, not an error bail.

## Explicitly out of scope (separate P0 work)
- `PermissionGuard` wiring into `dispatch_tool_call` (currently dead code).
- `write_to_file` absolute-path sandboxing.
- Per-session todo state (shared `resources_state.json`).
- App-data relocation out of `%TEMP%\maverick-app`.

## Verification
1. `cargo test -p maverick-backend --lib` — existing 55 tests + new tests green.
2. `cargo clippy -p maverick-backend` clean.
3. Manual: set `[budget] spend_cap_usd = 0.05` (and small `avg_tokens_per_turn`) in
   `%TEMP%\maverick-app\config.toml`; run a multi-step task; confirm multiple
   `SegmentBoundary` events, accumulating `UsageUpdated`, and a clean cap stop.
4. Optional live check (Kilo/nemotron gateway): one long task; verify it continues past the
   old 3-segment stop and stays coherent after mid-segment compaction.

## Notes
- `[budget]` defaults stay 40/3 — unbounded running is an explicit `spend_cap_usd` opt-in.
- Review findings that motivated this plan: permission guard never consulted at runtime,
  in-segment context growth unbounded, todo-gated continuation fragile with
  reasoning-heavy gateway models.
