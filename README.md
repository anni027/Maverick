# Nexus

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
- **`Provider`** (`src-tauri/src/providers/mod.rs`) — the unified backend trait. Phase 1
  ships a `MockProvider`; Phase 2 adds real streaming providers.
- **`ToolBridge`** (`src-tauri/src/tools.rs`) — the vendored tool runtime wired with the
  in-crate local terminal + local filesystem. No `xai-grok-shell` involved.

## Repo layout

```
nexus/
  Cargo.toml            # workspace root (hand-authored; lists crates/ + src-tauri)
  crates/               # vendored xai-grok core crates (copied from grok-build)
  src-tauri/            # the Rust backend (AgentLoop, Provider, tools, Tauri layer)
  frontend/             # React + TypeScript chat UI (Phase 6)
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
export PROTOC=/abs/path/to/nexus/bin/protoc.exe   # Windows
cargo run -p nexus-backend
```

The demo builds the agent loop with a `MockProvider` + the real tool bridge, sends one
user message, and runs the loop — exercising text + a real `bash` tool call end-to-end
(printed as events).

## Roadmap

- [x] Phase 0 — Scaffold + vendor ACP-free core crates
- [x] Phase 1 — Fresh `AgentLoop` + `AgentEvent` sink + `MockProvider` + real tool bridge
- [ ] Phase 2 — Unified `Provider` system (xAI / OpenAI / Anthropic / Kilo / OpenCode)
- [ ] Phase 3 — Tool system hardening (v1 tool subset, permission UX)
- [ ] Phase 4 — MCP support
- [ ] Phase 5 — Sessions + JSONL persistence
- [ ] Phase 6 — Tauri integration + React chat UI
- [ ] Phase 7 — Config store + API-key / MCP UX
