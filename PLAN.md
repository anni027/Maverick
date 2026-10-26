# Maverick — Long-Task Agent Plan

> Turn budgets, resumable segments, and heavy automation for the `maverick-backend` agent loop.
> Status: **Phases 0–4 shipped and green (`cargo test -p maverick-backend --lib`: 55 passed / 0 failed); Phase 5 live (parent-only `subagent` tool + bounded child loops).**
> Everything still open is itemized in **§5.6 — What's left** (real-provider validation + budget tuning from live measurements; token telemetry, run interrupt, live subagents, and live context-window wiring are implemented).

---

## 1. Summary

The agent loop (`nexus\src-tauri\src\agent_loop.rs`) used to hard-fail when a single
`send_user_message` run exceeded 16 LLM turns ("agent exceeded the maximum number of turns").
For long research and **automation** tasks this is both too tight and, more importantly,
the wrong failure mode: it throws away all work in flight.

This plan converts the loop from *one bounded run* into **budgeted, resumable segments** with
externalized state (todos + JSONL sessions + files), graceful wrap-up at the cap, and
automation-friendly execution (background/detached jobs, truncated outputs, error propagation).
Most of the heavy machinery already exists in the vendored crates — this plan is mostly wiring.

---

## 2. Current state (verified in code)

| Area | File | Fact |
|---|---|---|
| Turn cap | `nexus\src-tauri\src\agent_loop.rs` | ✅ FIXED (P1): three-state budget — warn at 37, no-tools wrap-up at 41, hard ceiling 42. `MAX_TURNS = 40` hardcoded |
| Loop exit | `agent_loop.rs` | breaks when the model returns zero tool calls; on the wrap-up turn no tools are offered so the break is the natural segment end |
| Error signal | `agent_loop.rs` | ✅ FIXED (P1): failed tool results get an idempotent `[TOOL_ERROR] ` sentinel prefix (sentinel chosen over schema change — Open Question #2 answered) |
| Bad args | `agent_loop.rs` | ✅ FIXED (P1): unparseable args return a marked error tool result instead of executing with `null` |
| Tool output | `agent_loop.rs` | ✅ FIXED (P1): head+tail truncation to 12 KB before `push_tool_result`, char-boundary safe |
| Registered tools | `nexus\src-tauri\src\tools.rs:180-193` | `run_terminal_cmd`, `get_task_output`, `kill_task`, `read_file`, `search_replace`, `list_dir`, `grep`, `todo_write`, Skill tool (+ new `duckduckgo_search`) |
| Web config | `tools.rs:217-218` | `web_search_config` / `web_fetch_config` = `Default::default()` (DDG tool added separately) |
| Permissions | `tools.rs` | ✅ FIXED (P4): allow/deny lists (deny wins, fail-closed) + audit log; default reads-pass / writes-need-approval |
| Todo persistence | `tools.rs:191,205` | `todo_write` registered; state persisted to `%TEMP%\maverick-app\resources_state.json` |
| Sessions | `tools.rs:64`, `src\session_store.rs` | JSONL persistence via `JsonlChatPersistence` |
| Context window | `tools.rs:83` | `context_window: 128_000` in SamplingConfig |
| Bash tool | `crates\codegen\xai-grok-tools\src\implementations\grok_build\bash\mod.rs` | foreground: 120s default, 5-min hard clamp; **background: unbounded (`Duration::MAX`)**, `task_id` + `get_task_output`/`kill_task` companions already registered |
| Compaction | `src-tauri\src\compaction.rs` + `agent_loop.rs::maybe_compact` | ✅ WIRED (P3): config-driven (`[context]` in `config.toml`) checkpoint at segment boundaries — `auto_compact_threshold_percent` (default 85) of the context window, newest `tail_keep_items` (24) kept verbatim, older prefix folded into one `[COMPACTION SUMMARY]` item carrying open todos + last wrap-up. Open todos re-injected as a `<system-reminder>` at every segment start. Per-tool output budgets (`tool_output_budgets`) shrink the 12 KB loop default |
| No-tools request | `crates\codegen\xai-chat-state\src\handle.rs:361` | `build_request(tool_definitions: Vec<ToolSpec>, …)` — empty vec = model gets no tools (forced text answer) |
| Subagents | `src-tauri\src\subagents.rs` | ✅ STUB (P5): `SubagentTask` (budget clamp 10 turns) + `render_subagent_summary`; full child-`AgentLoop` wiring still out of scope |
| Dev config | `nexus\src-tauri\tauri.conf.json` | ✅ FIXED (P0): `devUrl` → `http://localhost:1420`, plain `npm run … --prefix ../frontend` hooks; dev window gets Vite HMR |
| Settings UI | `nexus\frontend\src\components\Settings.tsx` | ✅ NEW (P3): "Context" tab — auto-compact toggle, threshold slider (50–100 %), `tail_keep_items`, per-tool output budgets for 5 tools; persists via `get/set_context_config` |

---

## 3. Problems this plan solves

1. **Dead-end at the cap** — 16 consecutive tool rounds → error string, work lost, UI stuck on `AgentEvent::Error`.
2. **No segment continuation** — no mechanism to keep working across a budget boundary.
3. **Context flooding** — search results / fetched pages / command logs pushed untruncated → re-reading churn, wasted turns, compaction pressure.
4. **Silent tool failures** — errors look like normal output → retry spirals burn the budget.
5. **Automation mismatch** — long jobs block foreground (5-min clamp) unless the model knows to go background/detached; no prompt guidance exists.
6. **Unattended risk** — `PermissionGuard` auto-approves every command; fine supervised, risky for hours-long automation.
7. **Dev workflow** — missing `devUrl` breaks frontend HMR (prereq for iterating on all of this).


---

## 4. Design principles

- **The cap bounds a segment, not the task.** A long task = N segments × (≤ `MAX_TURNS` each),
  checkpointed by todos + files between segments.
- **Never lose state.** Todos, session history, and job artifacts live on disk; any segment
  (or a crash) can resume.
- **Budget exhaustion ≠ crash.** At the cap: force a no-tools wrap-up answer; at a hard ceiling
  a few turns later: stop cleanly.
- **LLM turns are the scarce resource; wall-clock is not.** Long jobs must run background/
  detached; turns are spent on decisions, not waiting.
- **Cost guardrails are explicit.** Max spend ≈ `K segments × MAX_TURNS × avg tokens/turn` —
  all three configurable.

---

## 5. Implementation phases

### Phase 0 — Dev environment fix (prereq, ~15 min)
**Files:** `nexus\src-tauri\tauri.conf.json`
- Add `"devUrl": "http://127.0.0.1:1420"` to `build` (restores HMR; Vite is `strictPort` on 1420).
  (Explicit IPv4: bare `localhost` can bind IPv6-only while the Tauri CLI polls 127.0.0.1;
  `vite.config.ts` pins `server.host: '127.0.0.1'` to match.)
- Revert uncommitted PowerShell fallback hooks to plain `"npm run dev"` / `"npm run build"`
  (hooks run with CWD = the dir the CLI is invoked from → always `frontend/` via `npm run tauri dev`).
- Verify: `cd D:\grok\nexus\frontend && npm run tauri dev` → edit React file → hot reload.

### Phase 1 — Loop hardening (core, ~1 day) — ✅ IMPLEMENTED
**Files touched:** `agent_loop.rs`, `agent_event.rs`. `xai-grok-sampling-types` untouched.
Shipped as planned, with these actuals/deviations:
1. `MAX_TURNS = 40`, `WARNING_AT = 37`, `HARD_CEILING = 42` — hardcoded.
2. Wrap-up at `turn == MAX_TURNS + 1`: empty `specs` + injected `WRAP_UP_PROMPT`; loop breaks
   naturally on the tool-less answer. Hard ceiling bails only if a model emits tool calls with
   none offered.
3. Warning injected once at turn 37 + `AgentEvent::BudgetWarning { remaining }` emitted.
4. Error propagation via idempotent `[TOOL_ERROR] ` sentinel prefix — truncate **then** prefix,
   so the 12 KB budget math is measured on pre-marker text.
5. Truncation head+tail to 12 KB before `push_tool_result`; char-boundary safe (multi-byte UTF-8).
6. Invalid args → error tool result (never executes with `null`).
7. Tests: 5 new (wrap-up timing, short-chat regression, arg-safety round-trip, truncation
   boundary, prefix idempotence) over a scripted `Provider` + real `ToolBridge`/`ChatStateActor`.
   The suite is fully green — `cargo test -p maverick-backend --lib`: **31 passed, 0 failed**
   (the earlier `test_provider_model_reporting` actor panic is resolved; no known failures).

### Phase 2 — Segments & auto-continue — ✅ IMPLEMENTED
**Files touched:** `agent_loop.rs` (`MAX_SEGMENTS`, `CONTINUE_PROMPT`, `RunOutcome`,
`send_user_message_auto`, `has_open_todos` / `open_todo_labels`), `agent_event.rs`
(`SegmentBoundary` variant + PrintSink arm), `crates/codegen/xai-grok-tools/src/bridge.rs`
(getter passthrough), `commands.rs` + `lib.rs` (call sites).

**Verified enablers from the scan:**
- `turn` is local to `send_user_message` → every call already starts a fresh 40-turn budget.
  A segment boundary is simply a return from `send_user_message`; the runner just calls it again.
- `FinalizedToolset::get_resource_cloned::<T>()` (`registry/types.rs:1417`) can read
  `State<TodoState>`; at scan time `ToolBridge` only exposed the `update_resource` *setter*
  passthrough — the matching **getter passthrough has since been added** (`bridge.rs:580`) and is
  what the loop uses. Todo types are all `pub`:
  `xai_grok_tools::implementations::grok_build::todo::{TodoState, TodoStatus, TodoItem}` and
  `xai_grok_tools::types::resources::State` (`pub struct State<T>(pub T)`).
- `todo_write` stores state via `res.get_or_default::<State<TodoState>>()`; if the model never
  called `todo_write`, the resource is absent → `get_resource_cloned` returns `None` → treat as
  "no open todos" (stop). This replaces the fragile `resources_state.json` parsing originally
  planned (async writer + serde shape races).

**What shipped (actuals vs. the sketch below):** the sketch is the shape that landed, with these
differences — `MAX_SEGMENTS = 3`; `RunOutcome { hit_budget, wrap_up_text }` (wrap-up assistant text
captured inside `send_user_message`, no event re-parsing); `has_open_todos()` lives on `AgentLoop`
and reads the **live bridge resource** (`open_todo_labels()` → `State<TodoState>`, absent resource =
"nothing open"), labels rendered with `TodoStatus::tag()` for the compaction summary; the
identical-wrap-up guard compares `Option<String>` and only trips after a first wrap-up
(`last_wrap_up.is_some() && last_wrap_up == outcome.wrap_up_text`); both call sites
(`commands.rs:219`, `lib.rs:82`) now call `send_user_message_auto`.

**Stop conditions (all four):** no open todos · `!hit_budget` (model finished naturally) ·
`MAX_SEGMENTS` reached · identical wrap-up summary two segments running (broken-loop guard).

**Known limitation, documented:** user interrupt mid-run is not yet possible — `send_message`
awaits the whole segmented run and `AgentLoop` has no cancel token (the chat actor's internal
one is not plumbed through). Tracked as **§5.6-C**; until then the runner
is bounded (≤ `MAX_SEGMENTS × 41` model turns) so worst case is bounded, not unbounded.

**Tests:**
- Unit: `has_open_todos` true/false/absent-resource paths (seed the resource via
  `bridge.update_resource(State(TodoState::default()))`).
- Scripted-provider: provider always tool-calls → 3 segments run, 2 `SegmentBoundary` events,
  run ends via `MAX_SEGMENTS` with graceful wrap-up text, no bail.
- Scripted-provider: provider finishes naturally with pending todos left → exactly 1 segment,
  no continuation (the `!hit_budget` gate).
- Scripted-provider: provider repeats the same wrap-up text → stops at segment 2.
- Regression: existing 5 Phase 1 tests still pass; short chat = 1 segment, no boundary event.
- Accept (from original plan, unchanged): ~70-turn task completes across 2 segments; K=1
  behaves like today but with graceful wrap-up.

### Phase 3 — Context management ✅ IMPLEMENTED (config-gated)
**Files:** new `src-tauri\src\compaction.rs`, `config.rs` (`[context]`), `agent_loop.rs`
(`maybe_compact`, `reinject_todos`, per-tool budgets), `commands.rs`
(`get_context_config` / `set_context_config`), `xai-chat-state\src\types.rs`
(`AutoCompactTrigger::check`).
1. ✅ **Auto-compact wired** as a deterministic *checkpoint* at segment boundaries (never mid-turn, so
   it cannot race an in-flight turn). `CompactPolicy` derives from `[context]` in `config.toml`:
   `auto_compact_enabled` (default `true`), `auto_compact_threshold_percent` (default `85`),
   `tail_keep_items` (default `24`). Fires when
   `estimated_tokens >= context_window × percent / 100`; semantics mirror the vendored
   `xai-grok-agent\src\compaction.rs::CompactionPolicy` default (85) and reuse
   `AutoCompactTrigger` for the utilization-percent read. The LLM-summarization pass
   (`xai-grok-compaction`) stays vendored — no provider round-trip at the boundary yet.
   Replacement shape: `[system head?, [COMPACTION SUMMARY] item, newest tail…]`.
2. ✅ **Post-compaction todo re-injection**: `reinject_todos()` pushes a `<system-reminder>`
   (TodoGate pattern, `xai-grok-agent\src\system_reminder.rs:59`) listing pending/in-progress
   todos at every segment start; the checkpoint summary embeds the same list plus the previous
   wrap-up text, so todos survive compaction. No-op when nothing is open.
3. ✅ **Per-tool truncation budgets**: `[context].tool_output_budgets = { read_file = 8000, … }`
   (bytes). Budgets only ever *shrink* below the 12 KB loop default; unlisted tools keep it.
- **Tests:** `compaction.rs` — threshold math (=108 800 at 85%/128k), policy-disabled never fires,
  trigger utilization percent, system-head + tail preservation, todo/wrap-up contract, `[context]`
  TOML round-trip including legacy configs without the section; `agent_loop.rs` — no checkpoint
  below threshold, disabled policy no-op, per-tool budget truncates, reminder only when todos are
  open; vendored `xai-chat-state` — `AutoCompactTrigger::check` boundaries + 100% clamp.
- Accept: ✅ logic compile-verified and unit-covered; a live many-segment token-flatness
  measurement still requires a real provider run.

### Phase 4 — Automation ergonomics & safety (~1 day) — ✅ implemented (incl. §5.6-B/C)
**Files:** `prompts.rs`, `tools.rs`, `config.rs`
1. ✅ **System prompt rules** (`src-tauri\src\prompts.rs`): `AUTOMATION_SYSTEM_ADDENDUM` seeded as
   the system head of fresh sessions (`tools.rs::build_chat_handle`) — long work →
   `run_terminal_cmd` `background: true`, poll `get_task_output` in bounded waits, never block on
   sleep/foreground long jobs, `kill_task` strays, checkpoint via `todo_write`. The budget-warning
   turn carries the background-leftovers reminder text.
2. ✅ **Permission pass**: `PermissionGuard` has explicit `allow`/`deny` lists — **deny wins over
   auto-approve (fail-closed)** — plus the default policy (reads pass; `search_replace` /
   `todo_write` need `auto_approve`) and an append-only audit log (`tool-decisions.log`:
   timestamp + tool + decision + reason + 500-byte arg preview). `with_lists(..)` /
   `with_log_path(..)` builders allow a read-only unattended session.
3. ⚠️ **Cost guardrail: partial.** Per-run turn budgets are observable (`RunOutcome.hit_budget`,
   `BudgetWarning { remaining }`, `SegmentBoundary { segment, max_segments }`) and `[context]`
   bounds per-segment context spend, but per-task token accounting / an 80%-of-K warn event is not
   yet wired to a usage ledger.
- Accept: ⚠️ partially met — background-first guidance and guarded permissions are in place; the
  "1-hour unattended job" and per-segment token telemetry still need a real run.

### Phase 5 — Subagents (optional, later, ~2–3 days) — ✅ live (parent-depth tool, §5.6-D)
**Files:** new `src-tauri\src\subagents.rs` (stub); full wiring needs `tools.rs`
(`SessionContext.subagent`) + the task backend resource.
- ✅ **Stub landed:** `SubagentTask` (goal + turn budget clamped to `SUBAGENT_BUDGET_TURNS = 10`) and
  `render_subagent_summary` (`[SUBAGENT goal="…" budget=N] <findings>`), with tests. This is the
  parent/child contract — the parent context sees only compact summaries, never raw dumps.
- ❌ **Not wired:** spawning live child `AgentLoop`s (fresh context + budget returning the summary)
  through `SessionContext.subagent` stays out of scope, as the plan allows ("use when
  single-segment tasks still overflow; not required for most automation").

### 5.6 What's left — the remaining work, prioritized

**A. Real-provider validation (biggest gap; blocks everything else's "accept" criteria)**
Every phase is logic-verified against a scripted provider, not against a live LLM. Needed:
- [ ] Long research task (>16 rounds, DDG + fetches) completing across **2–3 segments** with
      graceful wrap-ups (Phase 2 accept).
- [ ] Automation task ("start a long background build, poll `get_task_output`, report") staying
      background-first per `AUTOMATION_SYSTEM_ADDENDUM` (Phase 4 accept).
- [ ] Compaction **token-flatness**: a many-segment run whose estimated context stays under the
      threshold after each checkpoint, and whose answers stay coherent *after* compaction
      (Phase 3 accept — the deterministic checkpoint is cheap, but quality is unmeasured).
- [ ] Crash/kill-restart: kill the app mid-background-job, restart, resume from JSONL + todos.
- [ ] Delegation task exercising the live `subagent` tool (parent summary quality, budget
      clamping at 10 turns, no nested delegation).
- [ ] Cancel mid-run via the new ■ Stop button (`cancel_message` → `Cancelled` event).
- [ ] Token telemetry sanity: `UsageUpdated`/`SpendWarning` events + `get_usage` totals against
      a real provider's reported usage; spend-cap abort on a capped task.

**B. Token & cost telemetry (Phase 4's remaining ❌) — ✅ IMPLEMENTED**
- [x] Per-segment/per-task token ledger (`src-tauri/src/usage.rs::UsageLedger`: prompt/
      completion/total tokens + USD cost from `cost_usd_ticks`), surfaced as
      `AgentEvent::UsageUpdated` per reporting turn + a `get_usage` command
      (`AgentLoop::usage_snapshot`, registered in `main.rs`).
- [x] 80%-of-budget (`max_segments × max_turns × avg_tokens_per_turn`, default 2000)
      one-shot warn event (`AgentEvent::SpendWarning`).
- [x] Optional per-task spend cap (`[budget].spend_cap_usd`) that aborts further segments
      with an error event.
- Tests: `usage.rs` unit (accumulation, zero-total fallback, tick filtering, one-shot
  warning, cap comparison) + `agent_loop.rs` integration (event accumulation into snapshot,
  silent-provider path, warning timing, cap abort).

**C. Run interrupt (Phase 2 documented limitation) — ✅ IMPLEMENTED**
- [x] `CancellationToken` per run: `send_user_message_auto` mints a fresh token per user
      message (`reset_cancel_token`); the turn loop checks it each iteration and `select!`s
      the provider call against it, so cancel lands promptly instead of after ≤ `3 × 41`
      turns. Emits `AgentEvent::Cancelled { segment }`.
- [x] `AgentLoop::cancel_current_run()` (sync, idempotent) + `cancel_message` Tauri command
      + ■ Stop button in `Chat.tsx` + `Cancelled` rendering.
- Tests: pre-cancelled run emits `Cancelled` with zero model turns; stale cancel never leaks
  into the next auto-run.

**D. Live subagents (Phase 5) — ✅ IMPLEMENTED (parent-depth only, no vendored `task` backend)**
- [x] `subagent` tool advertised to depth-0 loops (`tool_specs`); arg parsing
      (`parse_subagent_args`: non-empty goal, budget clamped 1–10); child `AgentLoop` with a
      fresh in-memory context, cloned bridge/provider, `depth = 1`, single segment capped at
      the task budget; findings return as a `render_subagent_summary` blob via a throwaway
      sink — the parent sees summaries only. Contract + tests already exist (`subagents.rs`).
- [x] Nesting fails closed: children are not offered the tool, and a hallucinated nested
      call returns an error tool result.
- Deviation note: implemented as a loop-intercepted tool rather than vendored
  `SessionContext.subagent` + `task`-tool backend wiring — same parent/child contract,
  far smaller blast radius. The vendored path stays available if deeper shell integration
  (background subagents, scheduler sharing) is ever needed.
- Tests: tool offered to parent only, summary shape, nested-call rejection, end-to-end
  delegation with summary-only leakage.

**E. Live context-window wiring (Phase 3 polish) — ✅ IMPLEMENTED**
- [x] `Provider::context_window()` (xAI 131072 / OpenAI 128000 / Anthropic 200000, default
      128000) matching each sampler config; `build_chat_handle_with_window` +
      `sync_chat_context_window` align the session sampling config at `init_session` and on
      every provider hot-swap (`sync_context_window`); `maybe_compact` resolves the live
      chat window first, then the provider's, then 128k.
- Tests: `test_provider_context_windows`, `effective_window_prefers_live_chat_config`.

**F. Tuning / follow-ups**
- [x] `MAX_TURNS` (40) and `MAX_SEGMENTS` (3) moved to `[budget]` (`BudgetConfig`: `max_turns`,
      `max_segments`, `auto_continue`, `spend_cap_usd`, `avg_tokens_per_turn`) with identical
      defaults derived from the same `agent_loop` constants; legacy `config.toml` files load
      unchanged. `get/set_budget_config` commands + Context-tab UI wired. Warning/wrap-up/
      ceiling derive from the live values with saturating math for tiny budgets.
- [x] Auto-continue default: on, with explicit opt-out per config (`auto_continue: false` →
      K=1 with graceful wrap-up). Per-session opt-in/out stays a follow-up if needed.
- [x] Vendored keyless `web_fetch` already registered alongside DDG (Open Question #4 —
      confirmed in `tools.rs`, no work needed).
- [ ] Revisit 40/3 defaults after A measures real runs (polling-heavy automation may want
      80–120 turns, Open Question #1).
- [ ] Consider wiring the vendored LLM-summarization compaction pass (`xai-grok-compaction`)
      for higher-fidelity checkpoints once A confirms the deterministic summary suffices.

**Not planned (unchanged, §9):** MCP marketplace hardening, image/video gen, LSP, scheduler tools,
true PTY UX, distributed orchestration.

---

## 6. Key code shape (Phase 1, illustrative only)

```rust
const MAX_TURNS: u32 = 40;
const HARD_CEILING: u32 = MAX_TURNS + 2;

let mut turn: u32 = 0;
loop {
    turn += 1;
    let wrap_up = turn == MAX_TURNS + 1;          // forced no-tools answer
    if turn > HARD_CEILING { bail!("hard turn ceiling reached ({turn})"); }

    let specs: Vec<ToolSpec> = if wrap_up { vec![] }
        else { self.tools.tool_definitions().await.into_iter().map(ToolSpec::from).collect() };

    if wrap_up {
        self.chat.push_user_message(ConversationItem::user(WRAP_UP_INSTRUCTIONS.into()));
    } else if turn + 3 == MAX_TURNS {
        // budget warning note into the conversation
    }

    let request = self.chat.build_request(specs, /* …unchanged… */).await;
    // … unchanged: provider.complete, push assistant, execute calls …
    // calls.is_empty() -> break  (guaranteed on the wrap-up turn: no tools offered)
}
```

---

## 7. Test plan

- **Unit:** wrap-up triggers exactly at `MAX_TURNS+1`; no-tools request built; truncation
  head+tail correctness; error flag propagation; unparseable-args error result.
  Phase 2 (`agent_loop.rs` tests): 3-segment cap, natural-finish gate, identical
  wrap-up guard, no-todos stop, natural-stop `RunOutcome`. Phase 3 (`compaction.rs`): threshold
  math (=108 800 @85%/128k), trigger boundaries + 100 % clamp, custom threshold, disabled policy,
  system-head + tail preservation, auto-continue contract, `[context]` TOML round-trip (incl.
  legacy configs); `agent_loop.rs`: no checkpoint below threshold, disabled policy no-op, per-tool
  budget truncates (below-default only), reminder only when todos open, todo state reflection.
  Phase 4 (`tools.rs`): deny-list wins over auto-approve + audit log. Phase 5 (`subagents.rs`):
  budget clamping, summary shape. Phase 1 + 2 truncation: boundary + below-default budget cases.
- **Fake-provider loop test:** a scripted provider that always emits one tool call → run ends
  in a wrap-up summary (not an error) at the right turn.
- **Integration (real):** see §5.6-A — the scripted-provider suite covers logic; these still need
  a live provider run (research task >16 rounds, unattended automation job, kill-restart resume).
- **Regression:** normal short chats never hit warnings; `cargo check` / `clippy` clean for
  workspace member `src-tauri`.

## 8. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Wrap-up produces a low-quality answer | Wrap-up prompt demands findings so far + explicit "what I could not finish"; auto-continue covers the rest |
| Auto-continue burns money on a broken loop | Hard K cap (3–5 segments); identical-error detection stops repeat segments |
| Compaction regressions mid-task | Phase 3 behind a config flag; compact only at segment boundaries initially |
| Unattended damage | Phase 4 permission deny/confirm list + full command logging before enabling long unattended runs |
| Context window still explodes | Truncation (P1) + compaction (P3) + per-tool output budgets |

## 9. Out of scope (for now)

- MCP marketplace hardening, image/video gen, LSP, scheduler tools (stay unregistered).
- True PTY / interactive terminal UX.
- Distributed / multi-machine job orchestration.

## 10. Open questions

1. `MAX_TURNS` default for automation sessions — 40 vs 80–120 (polling turns are cheap but
   not free)? *(deferred to §5.6-F)*
2. Error encoding: ~~extend `ToolResultItem` schema (clean) vs sentinel text prefix (quick)?~~
   **Answered (P1): sentinel prefix** (`[TOOL_ERROR] `, idempotent). Revisit only
   if it proves fragile in practice; JSONL schema stays untouched.
3. Auto-continue default: on, or opt-in per session? *(deferred to §5.6-F; currently always on)*
4. Register vendored `web_fetch` (keyless, SSRF-guarded) alongside the DDG tool while we're
   in there? *(deferred to §5.6-F)*

