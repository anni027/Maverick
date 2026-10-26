---
tags:
  - development
  - testing
  - contributor-guide
aliases:
  - Contributor guide
  - Build and test commands
---

# Development and validation

## Prerequisites

Nexus documentation lists:

- Rust 1.94, pinned by `rust-toolchain.toml`
- `protoc` v29.3 on `PATH` or via `PROTOC`; the bundled Windows binary is available under `nexus/bin/`
- Node 24 for the frontend

See [nexus/README.md](../nexus/README.md#L38-L44).

The production tree additionally documents DotSlash for hermetic tools and notes that Windows builds are best-effort. See [grok-build/README.md](../grok-build/README.md#L54-L79).

## Nexus commands

From the Nexus workspace root, the documented headless path is:

```text
cargo run -p maverick-backend
```

The backend crate is `maverick-backend`; the library is `maverick_backend`. See [nexus/src-tauri/Cargo.toml](../nexus/src-tauri/Cargo.toml#L1-L13) and [nexus/README.md](../nexus/README.md#L46-L56).

Frontend scripts are:

```text
npm run dev
npm run build
npm run preview
npm run tauri
```

See [nexus/frontend/package.json](../nexus/frontend/package.json#L6-L10).

The Vite server binds explicitly to `127.0.0.1:1420` with a strict port so Tauri development does not wait on an IPv6-only `localhost`. See [nexus/frontend/vite.config.ts](../nexus/frontend/vite.config.ts#L5-L20).

### Verification loop

The shortest useful check for a Nexus change:

```text
cargo test -p maverick-backend --message-format short   # from nexus/
npx tsc --noEmit                                        # from nexus/frontend/
```

Run workspace-wide `cargo check --workspace` only on Unix. On Windows it fails in
the vendored `xai-grok-pager-pty-harness` crate, which imports
`process_has_exited_without_reap` — a `#[cfg(unix)]`-only helper. That crate is
not in the app dependency graph, so scope checks and tests to
`-p maverick-backend`.

Piping those commands through `ripgrep` makes `$LASTEXITCODE` report ripgrep's
status, not cargo's; check the pass/fail count instead.

## Production commands

The production README recommends crate-local validation rather than full-workspace builds:

```text
cargo check -p <crate>
cargo test -p xai-grok-config
cargo clippy -p <crate>
cargo fmt --all
```

The composition-root package is `xai-grok-pager-bin`. See [grok-build/README.md](../grok-build/README.md#L75-L79) and [grok-build/README.md](../grok-build/README.md#L113-L120).

The production root `Cargo.toml` is generated; edit per-crate manifests instead. See [grok-build/README.md](../grok-build/README.md#L108-L111).

## Recorded plan status

`PLAN.md` records the agent-loop work as shipped through budgeted segments, deterministic compaction, automation guidance, permission audit scaffolding, usage telemetry, cancellation, and parent-only subagents. It also records that real-provider validation remained the largest acceptance gap at the time of writing.

Treat those statements as historical plan evidence, not as a substitute for running the current tests. See [PLAN.md](../PLAN.md#L1-L5) and [PLAN.md](../PLAN.md#L209-L289).

## Suggested contributor path

1. Read [[02 Repository Map]] and [[03 Nexus Architecture]].
2. Make production changes in `grok-build` when the intended fix belongs to the shared crate.
3. Port the deliberate Nexus delta explicitly; do not assume a dependency update synchronizes the trees.
4. Run crate-local checks/tests for the touched crate.
5. Exercise the relevant Tauri/frontend flow end to end when the change crosses a boundary.

## Related notes

- [[00 Home]]
- [[11 Current Work and Risks]]
- [[12 Source Index]]
