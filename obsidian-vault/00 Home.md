---
tags:
  - codebase
  - nexus
  - map-of-content
aliases:
  - Nexus vault home
  - Codebase map
---

# Nexus codebase vault

This vault is a navigation layer over the code in `D:\grok`. It records the current architecture, runtime flow, contributor workflows, provenance, and known risks without copying the source tree.

> [!info] Snapshot convention
> Source links are relative to this vault and include line anchors where the scan found useful evidence. Product/roadmap documents are treated as historical unless the implementation confirms the claim.

## Start here

- [[02 Repository Map|Repository map]] — where the production Grok Build tree and the standalone Nexus app live.
- [[03 Nexus Architecture|Nexus architecture]] — the backend modules and their boundaries.
- [[04 Request Lifecycle|Request lifecycle]] — what happens from a frontend message to a persisted session.
- [[10 Development and Validation|Development and validation]] — commands and contributor entry points.

## Explore by concern

| Concern | Note |
|---|---|
| Product intent and visual system | [[01 Product and Design]] |
| Runtime orchestration | [[03 Nexus Architecture]] · [[04 Request Lifecycle]] |
| Providers and MCP | [[05 Providers and MCP]] |
| Tools, permissions, and automation | [[06 Tools and Permissions]] |
| Persistence and configuration | [[07 Persistence and Configuration]] |
| React/Tauri frontend | [[08 Frontend Experience]] |
| Vendored-code provenance | [[09 Provenance and Drift]] |
| Open work and risks | [[11 Current Work and Risks]] |
| File/topic lookup | [[12 Source Index]] · [[13 Glossary]] |

## High-level shape

```text
React + Tauri WebView
        │ invoke / agent-event
        ▼
Rust Tauri commands + AppState
        ├─ Provider registry
        ├─ AgentLoop
        ├─ ToolBridge
        └─ Session/config stores
                │
                ├─ vendored xai-chat-state
                ├─ vendored xai-grok-tools
                └─ JSONL / TOML / resource state
```

## Orientation facts

- `nexus/` is a standalone Tauri application that vendors selected ACP-free Grok core crates; it does not depend on the production `grok-build/` tree.
- The production tree is the full Grok CLI/TUI and agent runtime. See [[02 Repository Map]].
- The intended product name is **Nexus**, but several backend, frontend, and documentation strings still say **Maverick**. See [[09 Provenance and Drift]].
- Runtime app data lives under Tauri's per-app data directory (`%APPDATA%\com.maverick.app` on Windows). An earlier build used the OS temp directory, which let temp cleanup destroy sessions and configuration; that location is gone. See [[07 Persistence and Configuration]].

## Maintenance note

When a source file changes materially, update the relevant note and the source index. Keep this vault descriptive: do not paste credentials, logs, generated artifacts, or large source excerpts.
