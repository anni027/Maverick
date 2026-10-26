---
tags:
  - risks
  - roadmap
  - open-questions
aliases:
  - Known issues
  - Open work
---

# Current work and risks

## Highest-priority validation gap

The implementation plan repeatedly distinguishes scripted-provider tests from live-provider acceptance. The most important remaining validation is a real-provider run that exercises long research, background automation, compaction coherence, crash/restart resume, cancellation, delegation, and telemetry. See [PLAN.md](../PLAN.md#L209-L238).

## Recently resolved

### Failure propagation and placeholders

- **Credential leakage in sampler logs** — The sampler emitted the first 20 characters of the `Authorization` header (`auth_prefix`) and the first 12 characters of `x-api-key` in `sampling_log` events. Fixed: `sampling_log.rs` now redacts `auth_prefix` to `"redacted"`; `client.rs` redacts `auth_header_prefix` and `x_api_key_prefix`. See [sampling_log.rs](../nexus/crates/codegen/xai-grok-sampler/src/sampling_log.rs#L30) and [client.rs](../nexus/crates/codegen/xai-grok-sampler/src/client.rs#L777).
- **API key serialization to frontend** — `ProviderConfig` serialized the `api_key` field to the frontend via `ProviderInfoDto`. Fixed: `skip_serializing` on the field. See [provider.rs](../nexus/src-tauri/src/providers/provider.rs#L54).
- **MCP handshake failures were swallowed** — `add_mcp_server_real` installed a `test_tool` placeholder and `add_mcp_server_full` reported success while discovery was discarded. Now handshake errors propagate to the caller and nothing is registered on failure; the Settings dialog re-throws into the dialog error line. See [mcp.rs](../nexus/src-tauri/src/mcp.rs#L33-L61) and [commands.rs](../nexus/src-tauri/src/commands.rs#L680-L733).
- **Startup MCP restore discarded its result** — the background restore ran `let _ =` over both the handshake and the join handle, so a server that failed at boot showed `Error` status with nothing in the logs saying why. Each restore now records success, handshake failure, or a panicked task through `log_restore_result`. See [commands.rs](../nexus/src-tauri/src/commands.rs#L172-L187).

### Data safety

- **App data lived in the OS temp directory** — Sessions, `config.toml`, and resource state sat under `temp_dir()/maverick-app`, so temp cleanup could erase everything. Now resolved from `app.path().app_data_dir()` in `.setup()` (`%APPDATA%\com.maverick.app` on Windows). See [main.rs](../nexus/src-tauri/src/main.rs#L14-L25).
- **`config.toml` could tear or brick startup** — A corrupt file used to fail startup permanently. Loading now moves unparseable TOML to `config.toml.corrupt` and returns defaults; saves `fsync` the temp file before rename; concurrent writers serialize on `persist_lock`. See [config.rs](../nexus/src-tauri/src/config.rs#L266-L307).
- **Session id paths were unvalidated** — A crafted id could escape `sessions/` with `../`. `validate_session_id` now rejects empty ids, path separators, and `..` on create, read, delete, and rename; delete and rename touch the filesystem before the index; rename-while-open rebuilds the session's chat handle and agent loop under the new id. See [session_store.rs](../nexus/src-tauri/src/session_store.rs#L160-L169) and [session_store.rs](../nexus/src-tauri/src/session_store.rs#L229-L266).
- **Persistence errors were silent** — Append and flush failures were dropped, `replace_history` returned a discarded result, and compaction could interleave with a concurrent append. Append/flush now log through `tracing`, the strip-and-ack path returns its real `Result`, and history replacement installs the fresh handle while the writer lock is held. See [session_store.rs](../nexus/src-tauri/src/session_store.rs#L83-L157).
- **Hardcoded workspace path** — `resolve_workspace_dir` returned `d:\grok\workspace`, which does not exist for anyone else. It now reads `MAVERICK_WORKSPACE_DIR` and otherwise falls back to `<cwd>/workspace`. See [tools.rs](../nexus/src-tauri/src/tools.rs#L222).

### Correctness

- **Runs leaked across session switches** — Chat held one run's state regardless of which session was selected, so switching sessions mid-run left the spinner on the wrong transcript, appended events to it, and could cancel the wrong session. Run ownership is now tracked (`runSessionId`/`runHere`/`busy`), stale `get_session_messages` responses are dropped by sequence number, and the listener no longer leaks on unmount. See [Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L71-L84) and [Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L374-L405).
- **Phantom `demo-session` directory** — Startup fell back to a literal `demo-session` id whenever `init_session` failed, creating a real directory and masking the error. Startup now picks the first real session or a fresh `session-<timestamp>` id, and a failed open retries with a fresh id while keeping the error visible. See [App.tsx](../nexus/frontend/src/App.tsx#L47-L100).
- **Provider fallback order was unstable** — The registry used `HashMap` iteration, so "first registered" changed between runs. It now keeps an explicit insertion order. See [registry.rs](../nexus/src-tauri/src/providers/registry.rs#L15-L58).
- **`remove_api_key` left live loops on the revoked key** — Unregistering removed the registry entry but not the `Provider` object the sessions already held, so the next turn kept sending the removed credential. The fallback rule now lives in one place (`ProviderRegistry::resolve`) shared with `init_session`, and key removal re-resolves every affected session — falling back to the first registered provider, or dropping the loop when nothing is left so the key cannot be used again. With no providers, `send_message` now reports "No provider configured" instead of "session not initialized". See [registry.rs](../nexus/src-tauri/src/providers/registry.rs#L62-L82) and [commands.rs](../nexus/src-tauri/src/commands.rs#L189-L253).
- **Subagent output never reached the UI** — Child-agent `ThinkingStep`/`ToolCall*` events were collected into the summary sink and discarded. A `MirroredSink` now forwards them to the parent while the summary still returns compact. See [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L141-L160).
- **Spend cap reported a hard error** — Hitting the cap emitted `SpendCapReached` and then a duplicate `Error`. It is now a graceful stop with a single event. See [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L946).
- **Tools could still run after cancel** — The tool dispatch arm awaited `tools.call` unconditionally; it now races cancellation and refuses remaining calls in the batch. Tool results in history were labeled with a raw `tool_call_id`; they now show the tool name. See [commands.rs](../nexus/src-tauri/src/commands.rs#L390-L455).

### Frontend wiring

- **UI preferences did nothing** — `show_tool_calls`, `auto_scroll`, and `compact_mode` were saved to `config.toml` but never read back or applied. App now loads `get_ui_config` and passes it to Chat, which filters tool bubbles, gates auto-scroll, and tightens spacing; Settings calls `onRefresh` after each toggle. See [App.tsx](../nexus/frontend/src/App.tsx#L29) and [Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L82).
- **MCP argument quoting** — `args.split(' ')` truncated any argument containing a space. Arguments are now split with a quote-aware tokenizer. See [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L28-L47).
- **Hardcoded colors in components** — every inline hex across the React components is now a semantic token declared in [index.css](../nexus/frontend/src/index.css#L24-L48) (`--ok`, `--warn`, `--danger`, `--code-*`, `--row-*`, `--chip`, `--bubble-user`…), with duplicates mapped onto the existing palette vars (`--panel-2`, `--panel-3`, `--bg`). Only the Aperture logo's three SVG gradient stops stay literal — SVG presentation attributes do not reliably accept `var()`. Coverage: 45 `var()` references, 0 undefined, 0 unused among the new tokens. The token swap itself was value-preserving; surfaces were then flattened to pure black on request (`--bg`/`--surface`/`--panel*`/`--chip`/`--surface-quiet`/`--bubble-user`/`--avatar-bg`/`--code-surface`/`--code-body` → `#000`, plus the dock's `rgba(13,14,17,*)` overlays → `rgba(0,0,0,*)`), with stepped values only where states must stay distinct: `--code-head #0a0a0a`, `--row-selected #111111`, `--row-hover #050505`, `--control-off #101010`. Separation now comes entirely from borders and text. Follow-up pass removed the last non-black fills: Markdown blockquote/table header/zebra/math/inline-code tints → `transparent` (structure kept via borders), and the duplicate "New chat" button under the prompt dock was deleted (`onNewSession` prop dropped from `Chat`; the sidebar's "Start new chat" remains the only affordance).
- **Skills marketplace** — the Skills tab now browses GitHub skill repos and installs with one click (default source `anthropics/skills`; sources stored in `config.toml`). Catalogs are cached on disk (`hub/catalogs/`), refresh is manual (GitHub 60 req/hr unauthenticated), installs land in `hub/skills/marketplace/` as Server scope with automatic bridge refresh. Backend: [skills.rs](../nexus/src-tauri/src/skills.rs#L491-L947), [commands.rs](../nexus/src-tauri/src/commands.rs#L1168-L1357), [config.rs](../nexus/src-tauri/src/config.rs#L241-L277); UI: [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L619-L670). 86/86 backend tests (12 marketplace, 4 workspace browser). Installs fetch SKILL.md only — bundled `scripts/`/`references/` not yet downloaded. Full write-up: [SKILLS-MARKETPLACE.md](../SKILLS-MARKETPLACE.md).
- **Kilo model selection** — The Kilo model picker fetches the catalog using the configured key/base URL, falls back to unauthenticated requests on 401/403/429, parses common response shapes, and persists the selected model immediately via `set_provider_settings` (hot-swapping active sessions). See [commands.rs](../nexus/src-tauri/src/commands.rs#L927-L979) and [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L102-L153).
- **Composer `+` menu** — the dock's `＋ MCP` pill was replaced by a four-entry `+` menu: attach text files (12 KB read cap, 5 MB file guard, NUL reject, 6 chips), browse the workspace through jailed `list_workspace_files`/`read_workspace_file` (depth 3, 500 entries, 24 KB, `..`/symlink escape denied), insert `use skill "<name>"` at the caret, or open the MCP dialog. Sends now embed attachments as `[Attached file: …]` fenced blocks and the send button accepts attachment-only messages. Backend: [commands.rs](../nexus/src-tauri/src/commands.rs#L1359-L1605); UI: [ComposerPlusMenu.tsx](../nexus/frontend/src/components/ComposerPlusMenu.tsx#L1-L327), [Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L333-L428).

## Runtime and safety observations (still open)

- API keys are plaintext in `config.toml`; no OS credential store is wired.
- `PermissionGuard` is created with auto-approval enabled.
- `run_terminal_cmd` bypasses the guard and relies on bridge checks.
- Direct write shims bypass the bridge/guard path and accept absolute paths.
- Malformed JSONL lines are still skipped silently while loading history.
- Tauri CSP is disabled and the default capability grants shell access; exact generated shell restrictions were not inspected in this scan.
- Marketplace counts are hardcoded scaffold data.

These are observations for review, not a claim that every path is exploitable. Any security change should be validated against the generated Tauri capability schema and real Windows behavior.

## Provider and integration caveats

- `ProviderKind::Mcp` exists, but MCP is currently a tool source rather than a selectable provider.
- The subprocess provider is a simple command/stdout adapter, not a full external-agent protocol.
- Provider settings hot-swap active loops, which is useful but should be tested during an in-flight request.

## Product and UX debt

- Maverick branding remains in copied source and UI.
- The product document says `nexus-app` and "Nexus"; the runtime says `com.maverick.app` and `Maverick`.
- The theme selector persists `dark`/`light`/`system`, but there is no light palette. Component colors are no longer the blocker: every inline hex in the React components is a semantic token now (see below), so a light theme only needs palette overrides plus `data-theme` wiring. DESIGN.md still describes the world as dark-only, so adding light is a product decision, not a refactor.
- Empty-state suggestions are plain text now, but there is no per-suggestion loading or error affordance.

## Open questions recorded in the plan

- Should automation defaults allow more than 40 turns per segment?
- Is deterministic compaction sufficient, or should the vendored LLM-summarization pass be wired?
- Should auto-continue be per-session rather than global?
- How should live context-window and cost telemetry be tuned from real runs?

See [PLAN.md](../PLAN.md#L357-L367).

## Related notes

- [[05 Providers and MCP]]
- [[06 Tools and Permissions]]
- [[07 Persistence and Configuration]]
- [[09 Provenance and Drift]]
