---
tags:
  - architecture
  - rust
  - nexus
aliases:
  - Backend architecture
  - Agent runtime architecture
---

# Nexus architecture

## Composition

Nexus is a single-process Tauri application with a React WebView frontend and a Rust backend. The backend is intentionally smaller than the production Grok shell: it composes a local `AgentLoop`, a provider abstraction, and a tool bridge.

```text
Tauri commands / AppState
        │
        ├─ ProviderRegistry
        ├─ SessionManager
        ├─ ConfigManager
        └─ shared ToolBridge
                │
                └─ per-session AgentLoop
                        ├─ ChatStateHandle
                        ├─ ToolBridge
                        └─ Provider
```

The module surface is exported from [nexus/src-tauri/src/lib.rs](../nexus/src-tauri/src/lib.rs#L9-L30). The crate is `maverick-backend` (library `maverick_backend`) per [nexus/src-tauri/Cargo.toml](../nexus/src-tauri/Cargo.toml#L1-L13).

## AppState

`AppState` owns:

- `SessionManager`
- provider registry
- permission guard
- per-session `AgentLoop` instances
- the shared `ToolBridge`
- configuration manager
- app-data path

It is constructed asynchronously in [nexus/src-tauri/src/commands.rs](../nexus/src-tauri/src/commands.rs#L18-L138). Each session receives its own chat-state handle but shares the bridge and configuration objects.

## AgentLoop

`AgentLoop` is the orchestration boundary. It:

1. Pushes a user message into `ChatStateHandle`.
2. Builds a request with the current tool specifications.
3. Calls the active `Provider`.
4. Streams assistant text and tool-call events.
5. Dispatches tool calls through the bridge or the in-loop subagent/write shims.
6. Shapes and stores tool results.
7. Applies cancellation, usage accounting, compaction, and segmented continuation.

The core structure and constants are in [nexus/src-tauri/src/agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L48-L226). The segmented runner is at [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L883-L985).

## Chat state

The vendored `xai-chat-state` actor owns conversation items, sampling configuration, event flow, and persistence. Fresh sessions receive the automation system addendum; resumed sessions retain their stored history. See [nexus/src-tauri/src/tools.rs](../nexus/src-tauri/src/tools.rs#L137-L200).

## Tool bridge

`ToolBridge` is built with local terminal and filesystem backends. The registered subset includes terminal execution, task output/kill, file reads and edits, directory listing, grep, todos, skills, web fetch, and a dynamic DuckDuckGo search tool. See [nexus/src-tauri/src/tools.rs](../nexus/src-tauri/src/tools.rs#L316-L399).

## Provider boundary

All native and external backends implement the same `Provider` trait and exchange `ConversationRequest` / `ConversationResponse` values from the vendored sampling types. See [nexus/src-tauri/src/providers/provider.rs](../nexus/src-tauri/src/providers/provider.rs#L46-L112).

`ProviderKind` defines five variants — `Xai`, `OpenAi`, `Anthropic`, `Subprocess`, `Mcp` — but the `Mcp` variant is never instantiated by any factory function. `create_provider` only matches `xai`/`openai`/`anthropic` and a custom branch (OpenAI- or Anthropic-compatible). `SubprocessProvider` exists but is only built via `SubprocessProvider::from_config`, not through `create_provider`. MCP servers therefore remain a tool source on the bridge, never a selectable provider. See [providers/mod.rs](../nexus/src-tauri/src/providers/mod.rs#L107-L128).

Native context windows, used for compaction thresholds: xAI `131_072`, OpenAI `128_000`, Anthropic `200_000`. See the tests in [providers/mod.rs](../nexus/src-tauri/src/providers/mod.rs#L165-L175).

## What is deliberately absent

Nexus does not currently initialize the vendored memory backend, LSP, scheduler, image/video generation, or the production Grok shell. The tool context explicitly sets `memory_backend: None` and leaves the heavier optional backends unset. See [tools.rs](../nexus/src-tauri/src/tools.rs#L317-L360).

## Related notes

- [[04 Request Lifecycle]]
- [[05 Providers and MCP]]
- [[06 Tools and Permissions]]
- [[07 Persistence and Configuration]]
