---
tags:
  - source-index
  - reference
aliases:
  - File index
  - Source map
---

# Source index

## Product and planning

- [PRODUCT.md](../PRODUCT.md#L1-L59) — intended product, users, workflow, constraints, and principles.
- [DESIGN.md](../DESIGN.md#L1-L54) — visual system and responsive/accessibility targets.
- [PLAN.md](../PLAN.md#L1-L367) — historical long-task agent plan, implementation status, tests, risks, and open questions.

## Nexus backend

- [nexus/src-tauri/src/main.rs](../nexus/src-tauri/src/main.rs#L1-L74) — process entry point, Tauri command registration, app-data directory setup.
- [nexus/src-tauri/src/lib.rs](../nexus/src-tauri/src/lib.rs#L1-L93) — module exports and headless demo.
- [nexus/src-tauri/src/commands.rs](../nexus/src-tauri/src/commands.rs#L1-L1605) — `AppState`, session/provider commands, message flow.
- [nexus/src-tauri/src/commands.rs](../nexus/src-tauri/src/commands.rs#L819-L925) — Kilo model catalog helpers (`kilo_models_url`, `kilo_error_detail`, `parse_kilo_models`, `send_kilo_models_request`).
- [nexus/src-tauri/src/commands.rs](../nexus/src-tauri/src/commands.rs#L927-L979) — `list_kilo_models` Tauri command with provider_id, base_url, api_key params and fallback.
- [nexus/src-tauri/src/commands.rs](../nexus/src-tauri/src/commands.rs#L1168-L1357) — skills marketplace commands (`list/add/remove_marketplace_source`, `list/search/install_marketplace_skills`, `installed` marking).
- [nexus/src-tauri/src/commands.rs](../nexus/src-tauri/src/commands.rs#L1359-L1605) — composer `+` menu workspace browser: `WorkspaceEntry`/`WorkspaceFileRead`, jailed `ws_resolve`/`ws_list`/`ws_read` (depth 3, 500 entries, 24 KB cap, NUL sniff) and `list_workspace_files`/`read_workspace_file` commands plus `workspace_browser_tests`.
- [nexus/src-tauri/src/agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L46-L2726) — orchestration, budgets, tools, compaction, subagents, cancellation.
- [nexus/src-tauri/src/tools.rs](../nexus/src-tauri/src/tools.rs#L1-L476) — permissions, chat handles, workspace resolution, tool bridge.
- [nexus/src-tauri/src/session_store.rs](../nexus/src-tauri/src/session_store.rs#L1-L266) — JSONL persistence, session id validation, and session management.
- [nexus/src-tauri/src/config.rs](../nexus/src-tauri/src/config.rs#L1-L722) — configuration model, corrupt-file recovery, persistence, provider and MCP settings.
- [nexus/src-tauri/src/config.rs](../nexus/src-tauri/src/config.rs#L241-L277) — `MarketplaceSource` (GitHub skill-catalog source, built-in `anthropic_official` default).
- [nexus/src-tauri/src/skills.rs](../nexus/src-tauri/src/skills.rs#L491-L947) — marketplace catalog (tree parsing, frontmatter, disk cache, install into hub cache) plus `marketplace_tests`.
- [nexus/src-tauri/src/config.rs](../nexus/src-tauri/src/config.rs#L602-L630) — marketplace source persistence (`marketplace_sources`, add/remove).
- [nexus/src-tauri/src/compaction.rs](../nexus/src-tauri/src/compaction.rs#L1-L406) — deterministic context checkpoints.
- [nexus/src-tauri/src/usage.rs](../nexus/src-tauri/src/usage.rs#L1-L250) — token and cost ledger.
- [nexus/src-tauri/src/subagents.rs](../nexus/src-tauri/src/subagents.rs#L1-L160) — bounded child-agent contract.
- [nexus/src-tauri/src/prompts.rs](../nexus/src-tauri/src/prompts.rs#L1-L21) — automation system guidance.
- [nexus/src-tauri/src/mcp.rs](../nexus/src-tauri/src/mcp.rs#L1-L286) — MCP discovery and handshake error propagation; marketplace scaffold at `scan_marketplace` (L217).
- [nexus/src-tauri/src/providers/provider.rs](../nexus/src-tauri/src/providers/provider.rs#L1-L124) — provider trait and DTOs (api_key `skip_serializing` at L54).
- [nexus/src-tauri/src/providers/registry.rs](../nexus/src-tauri/src/providers/registry.rs#L1-L61) — insertion-ordered provider registry (`order` at L15, `default_id` at L54).
- [nexus/src-tauri/src/providers/mod.rs](../nexus/src-tauri/src/providers/mod.rs#L1-L307) — provider factories, defaults, and URL normalization.

## Nexus frontend

- [nexus/frontend/src/main.tsx](../nexus/frontend/src/main.tsx#L1-L11) — React entry point.
- [nexus/frontend/src/App.tsx](../nexus/frontend/src/App.tsx#L1-L246) — shell, initialization, sessions, providers, UI preferences, and settings.
- [nexus/frontend/src/components/Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L1-L801) — message stream, tool events, input, attachment chips, per-session run ownership, cancellation, MCP dialog.
- [nexus/frontend/src/components/ComposerPlusMenu.tsx](../nexus/frontend/src/components/ComposerPlusMenu.tsx#L1-L327) — composer `+` menu: attach text files (12 KB cap), browse/read workspace files, insert skill references, open the MCP dialog.
- [nexus/frontend/src/components/Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L1-L882) — provider, MCP, skills, UI, context, and budget settings.
- [nexus/frontend/src/components/Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L111-L152) — Kilo model picker: `fetchKiloModels` (L111), `selectKiloModel` (L124), modal (L845).
- [nexus/frontend/src/components/Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L306-L367) — skills marketplace handlers (`loadMpSources`, `loadMpSkills`, `searchMpSkills`, `installMpSkill`, `addMpSource`, `removeMpSource`).
- [nexus/frontend/src/components/Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L619-L673) — skills marketplace panel (source select, search, results, add-source form).
- [nexus/frontend/src/components/SessionSidebar.tsx](../nexus/frontend/src/components/SessionSidebar.tsx#L1-L455) — session selection and mutations.
- [nexus/frontend/package.json](../nexus/frontend/package.json#L1-L27) — frontend scripts and dependencies.
- [nexus/frontend/vite.config.ts](../nexus/frontend/vite.config.ts#L1-L21) — development server and build settings.

## Vendored crates (security-relevant)

- [nexus/crates/codegen/xai-grok-sampler/src/sampling_log.rs](../nexus/crates/codegen/xai-grok-sampler/src/sampling_log.rs#L30) — `auth_prefix` redaction in `request_span`.
- [nexus/crates/codegen/xai-grok-sampler/src/client.rs](../nexus/crates/codegen/xai-grok-sampler/src/client.rs#L777) — `auth_header_prefix`/`x_api_key_prefix` redaction in `client_post` log.
- [nexus/src-tauri/src/providers/provider.rs](../nexus/src-tauri/src/providers/provider.rs#L54) — `ProviderConfig::api_key` marked `skip_serializing`.

## Production Grok Build

- [grok-build/README.md](../grok-build/README.md#L1-L140) — production purpose, build commands, layout, and contributor guidance.
- [grok-build/SOURCE_REV](../grok-build/SOURCE_REV#L1) — source snapshot SHA.
- [grok-build/Cargo.toml](../grok-build/Cargo.toml#L1-L466) — generated workspace manifest; treat as reference-only.

## Related notes

- [[00 Home]]
- [[02 Repository Map]]
- [[13 Glossary]]
