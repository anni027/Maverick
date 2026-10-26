---
tags:
  - provenance
  - vendor
  - drift
aliases:
  - Grok Build snapshot
  - Nexus drift notes
---

# Provenance and drift

## Production snapshot

`grok-build/SOURCE_REV` records:

```text
d5a0335a47221e8c9519936cb693e9b6450227ec
```

The production README describes a periodically synced Grok CLI/TUI source tree. Nexus vendors selected ACP-free crates rather than depending on that tree.

Sources: [grok-build/SOURCE_REV](../grok-build/SOURCE_REV#L1), [grok-build/README.md](../grok-build/README.md#L31-L35), [nexus/README.md](../nexus/README.md#L8-L9)

## Nexus-specific adaptations

A directory-level comparison found five targeted differences between the production snapshot and the Nexus vendored copy:

1. **Windows-compatible protoc output** — Nexus uses real temporary files instead of `/dev/stdout` and `/dev/null` in `xai-proto-build`.
2. **Provider-selected model remains authoritative** — the chat-state request builder uses `model: None` instead of forcing `"mock"`, avoiding provider/model mismatches.
3. **Percentage-based deterministic compaction** — `AutoCompactTrigger::check` adds threshold, zero-window, overflow, and 100% clamping behavior.
4. **Reasoning-content compatibility** — `ChatChunkDelta.reasoning_content` accepts the serde alias `"reasoning"`.
5. **Typed tool-state access** — `ToolBridge::get_resource_cloned<T>()` lets Nexus read typed resources such as `TodoState` without parsing persistence files.

These are deliberate Nexus adaptations. A change in `grok-build` will not automatically flow into `nexus`; shared-crate work needs an explicit porting step.

## Recent Nexus-specific security fixes (post-snapshot)

The following fixes were applied to the vendored crates after the snapshot and are not present in the production `grok-build` tree:

- **Credential redaction in sampling logs** — `sampling_log.rs` redacts `auth_prefix` to `"redacted"`; `client.rs` redacts `auth_header_prefix` and `x_api_key_prefix` in `client_post` events. This prevents credential fragments from appearing in `sampling.jsonl` logs. See [sampling_log.rs](../nexus/crates/codegen/xai-grok-sampler/src/sampling_log.rs#L30) and [client.rs](../nexus/crates/codegen/xai-grok-sampler/src/client.rs#L777).
- **API key serialization guard** — `ProviderConfig` in the Nexus provider module now marks `api_key` with `skip_serializing`, preventing API keys from being sent to the frontend via `ProviderInfoDto`. See [provider.rs](../nexus/src-tauri/src/providers/provider.rs#L54).

These are Nexus-only fixes; they do not exist in the production `grok-build` tree and must be re-applied if the vendored crates are re-synced.

## Naming drift

The intended product is Nexus in the root `PRODUCT.md` and `DESIGN.md`, but the copied application still uses Maverick in:

- `nexus/README.md` — the README's own title and body are Maverick, and it even describes the app as "maverick/" in its repo-layout tree.
- `nexus/src-tauri` package/module documentation and crate name (`maverick-backend` / `maverick_backend`), including `main.rs` ("Maverick main entry point") and the headless demo target.
- frontend entry points, labels, placeholders, and status text
- runtime app-data directory name (`com.maverick.app` under the OS app-data dir)

The root product document also names `nexus-app` and "Nexus", while the implementation uses `com.maverick.app` and `Maverick`. See [PRODUCT.md](../PRODUCT.md#L27-L31) and [main.rs](../nexus/src-tauri/src/main.rs#L14-L25).

## Documentation drift

`PLAN.md` contains historical phase status and roadmap claims. It is useful for understanding implemented machinery, but live-provider acceptance tests and later roadmap claims should be treated as unverified unless re-run.

## Obsidian integration

No Nexus-specific Obsidian vault, note-creation command, encrypted-note facility, or knowledge-vault adapter was found in the scanned runtime and frontend. This vault is a documentation artifact created alongside the codebase, not an application feature.

## Related notes

- [[02 Repository Map]]
- [[03 Nexus Architecture]]
- [[11 Current Work and Risks]]
