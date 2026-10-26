---
tags:
  - persistence
  - configuration
  - sessions
aliases:
  - State storage
  - Session persistence
---

# Persistence and configuration

## App-data location

The Tauri entry point resolves Tauri's per-app data directory inside `.setup()`:

```text
<roaming app data>/com.maverick.app
```

On Windows that is `%APPDATA%\com.maverick.app`. `AppState::new` then creates `sessions/`, `config.toml`, and `resources_state.json` under it.

See [nexus/src-tauri/src/main.rs](../nexus/src-tauri/src/main.rs#L14-L25).

Earlier builds used `<std::env::temp_dir()>/maverick-app`, which is convenient for a demo but not a durable boundary: temporary-directory cleanup and profile changes could remove sessions, configuration, and tool state. That location is no longer used.

## Configuration

`config.toml` contains:

- provider API keys
- default provider
- provider base URL/model/kind overrides
- MCP server definitions
- UI preferences
- context/compaction policy
- turn/segment budgets and optional spend cap

The configuration model and defaults are in [nexus/src-tauri/src/config.rs](../nexus/src-tauri/src/config.rs#L18-L108). Saves write `config.toml.tmp`, `fsync` it, and rename it over `config.toml`, so a crash mid-write cannot leave a torn file. A load that finds unparseable TOML moves it to `config.toml.corrupt` and returns defaults instead of failing startup. See [config.rs](../nexus/src-tauri/src/config.rs#L266-L307). Concurrent saves serialize on `ConfigManager::persist_lock`, so two settings panels cannot interleave writes. See [config.rs](../nexus/src-tauri/src/config.rs#L615-L621).

API keys are stored as plaintext strings in the TOML file. The frontend snapshot exposes key names, not values. See [config.rs](../nexus/src-tauri/src/config.rs#L18-L25) and [config.rs](../nexus/src-tauri/src/config.rs#L601-L613).

## Session history

Each session is a directory under:

```text
<app_data>/sessions/<session_id>/chat_history.jsonl
```

`JsonlChatPersistence` loads existing JSONL, appends new items, and exposes history to the chat actor. See [nexus/src-tauri/src/session_store.rs](../nexus/src-tauri/src/session_store.rs#L22-L104).

Malformed lines are still skipped while loading, but append and flush failures are now logged through `tracing` rather than dropped, and `replace_history_for_strip_and_ack` returns the real result to its caller. Compaction replaces history under the session's write lock: the fresh history handle is installed while the lock is held, so a concurrent append cannot interleave with the rewrite. See [session_store.rs](../nexus/src-tauri/src/session_store.rs#L83-L157).

Session ids are validated before they reach the filesystem (`validate_session_id` rejects empty ids, path separators, and `..`), so a crafted id cannot escape `sessions/`. It is applied on create, list-lookup, delete, and rename. See [session_store.rs](../nexus/src-tauri/src/session_store.rs#L160-L169).

Session listing orders directories by modification time. Delete removes the session directory before dropping it from the index, and rename renames the directory first, so a failed operation cannot leave the index pointing at a file that no longer exists. See [session_store.rs](../nexus/src-tauri/src/session_store.rs#L176-L266).

## Tool/resource state

The tool bridge stores resource state at `<app_data>/resources_state.json`. The current tool context does not initialize the vendored memory backend. See [tools.rs](../nexus/src-tauri/src/tools.rs#L311-L399).

## Audit log

Permission decisions are appended to `tool-decisions.log` in the resolved workspace directory. The log contains a timestamp, tool name, decision, reason, and a truncated argument preview. See [tools.rs](../nexus/src-tauri/src/tools.rs#L31-L135).

## Related notes

- [[04 Request Lifecycle]]
- [[06 Tools and Permissions]]
- [[11 Current Work and Risks]]
