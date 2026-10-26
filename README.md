# Maverick

A **chat-first automation agent** built by slicing [`xai-grok`](https://github.com/xai-org/grok)
(the `grok` CLI) into a clean, single-process Rust core. Think Hermes / OpenClaw, but
with a pluggable **provider** system: native LLM providers (xAI / OpenAI / Anthropic) and
external agent runtimes (Kilo Code, OpenCode) all register as interchangeable backends.

This repository is a **new standalone app** — it vendors the ACP-free core crates from
`grok-build` (under `crates/`) rather than depending on it.

## Architecture

```
Tauri WebView (React)  ──invoke/events──▶  Rust backend (single process)
                                            ├─ AgentLoop   (fresh, ACP-free orchestrator)
                                            ├─ Provider    (xai / openai / kilocode / …)
                                            └─ ToolBridge  (vendored xai-grok-tools)
```

- **`AgentLoop`** (`src-tauri/src/agent_loop.rs`) — composes three vendored, ACP-free
  building blocks: `xai-chat-state` (conversation), `xai-grok-tools` (tool runtime),
  and a pluggable `Provider`. It is deliberately free of any ACP / transport code.
- **`Provider`** (`src-tauri/src/providers/mod.rs`) — the unified backend trait with real streaming providers (xAI / OpenAI / Anthropic / custom).
- **`ToolBridge`** (`src-tauri/src/tools.rs`) — the vendored tool runtime wired with the
  in-crate local terminal + local filesystem. No `xai-grok-shell` involved.

## Repo layout

```
maverick/
  Cargo.toml            # workspace root (hand-authored; lists crates/ + src-tauri)
  crates/               # vendored xai-grok core crates (copied from grok-build)
  src-tauri/            # the Rust backend (AgentLoop, Provider, tools, Tauri layer)
  frontend/             # React + TypeScript chat UI
  bin/protoc.exe        # protoc v29.3 (needed only to build the proto crate)
```

## Prerequisites

- **Rust 1.94** (pinned via `rust-toolchain.toml`).
- **`protoc` v29.3** on `PATH` (or set `PROTOC` to the binary). The vendored
  `xai-grok-tools-api` crate uses `protoc` at build time. On Windows, the bundled
  `bin/protoc.exe` works; on Linux/macOS the DotSlash wrapper in `grok-build/bin` is used.
- **Node 24** for the frontend (Phase 6).

## Build & run (headless demo)

```bash
# from repo root
export PROTOC=/abs/path/to/maverick/bin/protoc.exe   # Windows
cargo run -p maverick-backend
```

The demo builds the agent loop with a real provider + the real tool bridge, sends one
user message, and runs the loop — exercising text + a real `bash` tool call end-to-end
(printed as events). Requires an API key in `maverick-app/config.toml`.

## Browser (the Computer)

Maverick's agent can drive a real browser. Enable it from **Settings →
Computer → Connect**: it registers the Playwright MCP server (stdio) and runs
the handshake immediately, so the browser tools reach the model on that same
turn. It shows up on the MCP tab as the `browser` server, so the normal
allow/deny lists and the `tool-decisions.log` audit trail apply unchanged.

- **Headed by default.** A real Chrome window opens and you watch the agent
  work. Flip to headless in the Computer tab (it reconnects automatically).
- **Installed Chrome, not bundled Chromium.** Same fingerprint class as an
  ordinary browser — no `HeadlessChrome` UA tell, real plugin list.
- **Persistent profile** under the app-data dir, so logins survive stop/start.
  Turn it off for a throwaway profile.
- First connect downloads the MCP package via `npx`, so give it a moment on a
  cold cache. Needs Chrome or Edge on a known path.

The commands are `computer_status`, `computer_enable`, `computer_disable`
(`src-tauri/src/commands.rs`); options are read back out of the stored MCP
args, so no second config section is needed.

### Bot-detection proof harness

`frontend/scripts/browser-harness.mjs` is the single repeatable harness for
the two questions that matter — it drives the same browser the agent uses and
reports **both at once**:

1. **Reliability** — did the agent actually browse? Per target: navigation
   succeeded, content rendered, screenshot returned.
2. **Detection** — what does the site see? The exact signals a bot-detection
   script reads off `window`: `navigator.webdriver`, plugin count, the
   `HeadlessChrome` UA tell, languages, hardware concurrency.

```bash
cd frontend
node scripts/browser-harness.mjs                    # default 3 sites, headed
node scripts/browser-harness.mjs --json             # machine-readable
node scripts/browser-harness.mjs --runs 5           # success rate per site
node scripts/browser-harness.mjs --headless --no-profile
node scripts/browser-harness.mjs https://yoursite.example/check
```

Pass your own URLs to run it against your own endpoints — that's the point of
one harness: the same command verifies your own bot-detection *and* ordinary
public-site reliability. Exit code is `0` only when every run passes, so it
drops straight into CI or a pre-release check.

It deliberately does **not** defeat, bypass, or spoof third-party anti-bot or
CAPTCHA systems. It reports what real, installed Chrome exposes and whether
browsing works — nothing else.

## Live preview (artifacts)

The artifact drawer (right-hand panel) shows what the agent produced: a
`Preview`/`Code` toggle, prev/next across a session's artifacts, copy and
download.

**Previews are served, not inlined.** A `srcDoc` iframe has no origin, so a
relative `<link href="styles.css">`, an ES module, or a `fetch()` resolves
against `about:srcdoc` and misses — a multi-file page renders blank or
unstyled. `preview_server_start` (`src-tauri/src/preview_server.rs`) instead
serves the file's directory from an ephemeral `127.0.0.1` port, and the iframe
loads from that URL, so relative assets resolve exactly as they do in a
browser. The server:

- binds **loopback only** — never LAN-reachable, and no firewall prompt;
- serves from whatever root actually holds the artifact: the workspace that
  contains it, so its siblings resolve; a file the agent wrote outside every
  known workspace (an absolute path, or a one-off `cwd`) is served from its own
  directory rather than 404ing against a root that never held it;
- refuses relative paths that climb out with `..` or an escaping symlink, and
  answers `GET`/`HEAD` only, with no directory listing;
- sends `Cache-Control: no-store`, so the panel always shows the agent's
  *latest* write rather than a cached render.

A relative path is looked up in the session workspace first, then in the
directory the tools actually write into — those two can differ, so a bare
`index.html` is found in whichever holds it. One listener exists per root (up
to four, oldest evicted), so opening an artifact somewhere else never kills the
URL an already-open one is using.

**Reads are no longer capped at 24 KB.** Previewing goes through its own
`PREVIEW_MAX_BYTES` (4 MB) rather than the 24 KB attachment limit. A generated
page with inline CSS/JS is routinely past 24 KB, and one truncated *mid-tag*
renders as nothing — which is what made large previews look broken.

**Auto-open** (Settings → Behavior, **on by default**): when the agent writes a
previewable file (`.html`/`.htm`/`.svg`/`.md`) the drawer opens on its own.
Detection covers the whole write family — `write_to_file` as well as the
`search_replace`/`hashline_edit`/`apply_patch` tools the agent usually reaches
for — and success is "the result carries no `[TOOL_ERROR]` marker", not one
tool's exact wording. Each run auto-opens at most once, and never while the
drawer is already open — an open drawer is refreshed in place instead, so an
active artifact is never yanked away. Turn it off to keep previews click-to-open
via the eye icon on a code block or a write-tool chip.

The composer column **narrows beside the panel** instead of continuing
underneath it; the panel edge stays draggable and the width is remembered.

Bare HTML/SVG snippets that exist only in a chat message (no file behind them)
still render inline through `srcDoc`, since there is nothing to serve.

## Roadmap

- [x] Phase 0 — Scaffold + vendor ACP-free core crates
- [x] Phase 1 — Fresh `AgentLoop` + `AgentEvent` sink + real tool bridge
- [x] Phase 2 — Unified `Provider` system (xAI / OpenAI / Anthropic / Kilo / OpenCode)
- [x] Phase 3 — Tool system hardening (v1 tool subset, permission UX)
- [x] Phase 4 — MCP placeholder + real handshake pending
- [x] Phase 5 — Sessions + JSONL persistence
- [x] Phase 6 — Tauri integration + React chat UI
- [x] Phase 7 — Config store + API-key / MCP UX
- [x] Rebrand — fully Maverick, mock removed
