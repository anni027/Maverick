---
tags:
  - glossary
  - reference
aliases:
  - Terms
  - Vocabulary
---

# Glossary

- **AgentLoop** — Nexus's provider-agnostic orchestration loop for conversation, provider calls, tool execution, budgets, compaction, and cancellation.
- **AppState** — shared Rust state containing sessions, providers, configuration, permissions, tool bridge, and per-session loops.
- **ChatStateHandle** — handle to the vendored chat-state actor that stores conversation items and sampling configuration.
- **Compaction** — folding older conversation history into a checkpoint summary while preserving the system head and newest tail.
- **Context window** — the token capacity used to decide when compaction should run; Nexus aligns it with the active provider.
- **Kilo model catalog** — list of models exposed by the Kilo AI Gateway (`/models` endpoint). Fetched by `list_kilo_models` with configured key/base URL; falls back to unauthenticated on 401/403/429.
- **MCP** — Model Context Protocol; in Nexus it supplies tools to the bridge rather than acting as a selectable LLM provider.
- **Provider** — the Rust trait abstracting xAI, OpenAI, Anthropic, custom-compatible, or subprocess backends.
- **Provider registry** — in-memory collection of configured provider implementations used when initializing or hot-swapping sessions.
- **Segment** — one bounded portion of a user request; a request may continue into additional segments when budgets and open todos require it.
- **ToolBridge** — vendored tool runtime that dispatches terminal, filesystem, search, skill, web, todo, and MCP tools.
- **TodoState** — typed bridge resource used to checkpoint and resume open work across segments.
- **JSONL** — JSON Lines session-history format stored under `sessions/<id>/chat_history.jsonl`.
- **Tauri invoke** — frontend-to-Rust command call used for sessions, providers, settings, tools, and messages.
- **Tauri event** — Rust-to-frontend stream used for assistant text, tool calls, warnings, cancellation, and errors.
- **Vendored crate** — a crate copied into `nexus/crates` from the production `grok-build` snapshot rather than referenced as an external dependency.

## Navigation

- [[03 Nexus Architecture]]
- [[04 Request Lifecycle]]
- [[12 Source Index]]
