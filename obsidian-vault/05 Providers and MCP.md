---
tags:
  - providers
  - mcp
  - backend
aliases:
  - Provider registry
  - MCP integration
---

# Providers and MCP

## Provider abstraction

`Provider` is the backend boundary used by `AgentLoop`. It exposes identity, kind, capabilities, context-window size, and an asynchronous `complete` method. The wire vocabulary is `ConversationRequest` → `ConversationResponse`, which keeps the loop independent of HTTP, SSE, or subprocess transport details.

See [nexus/src-tauri/src/providers/provider.rs](../nexus/src-tauri/src/providers/provider.rs#L72-L112).

## Native providers

The provider module implements:

- xAI
- OpenAI
- Anthropic
- custom OpenAI-compatible providers
- custom Anthropic-compatible providers

Defaults and URL normalization are in [providers/mod.rs](../nexus/src-tauri/src/providers/mod.rs#L33-L120). Provider settings support base URL and model overrides; custom provider selection uses an explicit `kind` of `openai` or `anthropic`.

The UI exposes known providers plus a Kilo AI Gateway preset and a custom-provider form. See [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L12-L21) and [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L342-L460).

## Kilo AI Gateway model selection

The Kilo provider supports browsing and selecting from the gateway's model catalog. The frontend opens a model picker modal that fetches the catalog from the Kilo gateway via a dedicated Tauri command. Selection persists immediately via `set_provider_settings`, hot-swaps the active session, and refreshes the provider list. See [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L102-L113), [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L115-L144), and the picker modal at [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L722-L756).

The catalog request uses the configured base URL and API key when available, falls back to unauthenticated requests on 401/403/429, and parses common response shapes (`data`, `models`, or top-level array). See [commands.rs](../nexus/src-tauri/src/commands.rs#L819-L925) and [commands.rs](../nexus/src-tauri/src/commands.rs#L927-L979).

## Provider registry behavior

At startup, providers are registered only when configuration provides an API key or enough custom settings to construct a backend. The registry is held behind an async read/write lock. Settings changes hot-swap matching active session loops and synchronize their context windows. See [commands.rs](../nexus/src-tauri/src/commands.rs#L32-L74) and [commands.rs](../nexus/src-tauri/src/commands.rs#L152-L171).

Native providers are wired through the real `xai-grok-sampler` actor, not a mock: `XaiProvider` uses the Responses backend with a 131_072-token window, `OpenAiProvider` uses ChatCompletions with 128_000, and `AnthropicProvider` uses Messages with 200_000. Each sets `stream_tool_calls: true`. See [providers/xai.rs](../nexus/src-tauri/src/providers/xai.rs#L14-L26), [providers/openai.rs](../nexus/src-tauri/src/providers/openai.rs#L14-L26), and [providers/anthropic.rs](../nexus/src-tauri/src/providers/anthropic.rs#L14-L26).

The registry keeps an explicit insertion order (`order: Vec<String>`) alongside its id map, and `register`, `list`, `ids`, `default_id`, and `remove` all walk that order. `default_id()` and the "first registered" fallback in `init_session` are therefore stable across runs. See [providers/registry.rs](../nexus/src-tauri/src/providers/registry.rs#L15-L58).

`ProviderKind::Mcp` is defined but never instantiated — no factory creates an MCP provider. MCP servers are registered as tools on the bridge, never as providers. See [providers/mod.rs](../nexus/src-tauri/src/providers/mod.rs#L107-L128).

## MCP is a tool source, not a provider

Although `ProviderKind::Mcp` exists as an enum variant, the current provider configuration has only HTTP and subprocess variants, and provider registration does not create an MCP provider. MCP servers are registered as tools on the shared `ToolBridge`.

See [provider.rs](../nexus/src-tauri/src/providers/provider.rs#L22-L62) and [config.rs](../nexus/src-tauri/src/config.rs#L190-L208).

## MCP discovery

For stdio servers, Nexus performs a real ACP/MCP handshake and `tools/list`, with a 30-second handshake timeout and 15-second listing timeout. Real registrations are added to the bridge. A handshake or discovery failure now propagates as an error and installs nothing: `add_mcp_server_real` returns the failure instead of leaving a `test_tool` placeholder, and callers surface it. See [mcp.rs](../nexus/src-tauri/src/mcp.rs#L33-L61).

HTTP MCP performs a real handshake and fails when no tools are returned. See [mcp.rs](../nexus/src-tauri/src/mcp.rs#L112-L168).

Startup restoration launches background handshakes and does not wait on them — `AppState::new` must not block on a 30-second handshake (see [commands.rs](../nexus/src-tauri/src/commands.rs#L81-L145)). The result is no longer discarded: each restore logs success, a handshake failure, or a panicked task, and a server that fails its restore stays absent from the bridge (so `get_mcp_status` reports `Error`) until it is re-added (see [commands.rs](../nexus/src-tauri/src/commands.rs#L172-L187)). `add_mcp_server_full` (the command the Settings dialog calls) writes the server into `config.toml` first, then runs the handshake and returns any handshake error to the UI — so a failed add leaves config updated but registers nothing (see [commands.rs](../nexus/src-tauri/src/commands.rs#L741-L796)). These behaviors are important when interpreting "Ready", "Placeholder", and "Error" status in the UI.

## Marketplace scaffold

`scan_marketplace` returns five hardcoded entries with fabricated installation counts. It is a scaffold, not evidence of a live marketplace or real usage statistics. See [mcp.rs](../nexus/src-tauri/src/mcp.rs#L217-L286).

## Related notes

- [[03 Nexus Architecture]]
- [[04 Request Lifecycle]]
- [[11 Current Work and Risks]]
