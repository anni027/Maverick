---
tags:
  - repository
  - architecture
  - navigation
aliases:
  - Repo map
  - Directory map
---

# Repository map

## Top-level layout

| Path | Role |
|---|---|
| `nexus/` | Standalone Tauri/React application and vendored Grok core crates. |
| `grok-build/` | Production Grok CLI/TUI source snapshot. |
| `workspace/` | Local working area; ignored by the root `.gitignore` and used as the tool working directory. |
| `DESIGN.md` | Intended visual system. |
| `PRODUCT.md` | Intended product definition. |
| `PLAN.md` | Historical implementation plan and status notes for the agent loop. |

There is no `AGENTS.md` in the scanned source trees. The root `.gitignore` excludes `target/`, `node_modules/`, `dist/`, `.vite/`, and workspace/local state.

## `nexus/`

The Nexus workspace contains:

- `src-tauri/`: Rust backend, Tauri commands, provider implementations, session/config stores, tools, MCP, compaction, usage, and subagents.
- `frontend/`: React/Vite/TypeScript UI.
- `crates/`: selected vendored xAI/Grok crates copied from the production tree.
- `Cargo.toml`: hand-authored workspace manifest that adds `src-tauri` to the vendored crate members.

The backend crate is named `maverick-backend` in the manifest, but its library name is `maverick_backend`. See [nexus/src-tauri/Cargo.toml](../nexus/src-tauri/Cargo.toml#L1-L13).

## `grok-build/`

The production tree is a much larger CLI/TUI system:

- `crates/codegen/xai-grok-pager-bin`: composition root for the `grok` binary.
- `crates/codegen/xai-grok-pager`: full-screen TUI.
- `crates/codegen/xai-grok-shell`: agent runtime and transport entry points.
- `crates/codegen/xai-grok-tools`: tool implementations.
- `crates/codegen/xai-grok-workspace`: host filesystem, VCS, execution, and checkpoints.
- `crates/common/`, `crates/build/`, `prod/mc/`, and `third_party/`: supporting crates and vendored upstream source.

The production README explicitly says the root `Cargo.toml` is generated and that contributors should edit per-crate manifests. See [grok-build/README.md](../grok-build/README.md#L95-L120).

## Relationship between the trees

`nexus/README.md` documents the app as **Maverick** and says it vendors ACP-free core crates rather than depending on `grok-build`. The backend depends only on a selected subset of those crates; it does not embed the production shell, pager, memory system, or full ACP runtime. The README's own title and branding are Maverick, not Nexus — see [[09 Provenance and Drift]].

`grok-build/SOURCE_REV` records the production snapshot as `d5a0335a47221e8c9519936cb693e9b6450227ec`. Nexus is therefore a synchronized snapshot with deliberate local adaptations, not a live dependency view. See [[09 Provenance and Drift]].

## Navigation

- Architecture: [[03 Nexus Architecture]]
- Runtime: [[04 Request Lifecycle]]
- Frontend: [[08 Frontend Experience]]
- Development: [[10 Development and Validation]]
- Source lookup: [[12 Source Index]]
