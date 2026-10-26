---
tags:
  - tools
  - permissions
  - automation
aliases:
  - Tool runtime
  - Safety model
---

# Tools and permissions

## Tool bridge

The bridge uses the vendored local terminal and filesystem backends. The registered built-in subset is:

- `run_terminal_cmd`
- `get_task_output`
- `kill_task`
- `read_file`
- `search_replace`
- `list_dir`
- `grep`
- `todo_write`
- `skill`
- `web_fetch`
- dynamic `duckduckgo_search`

See [nexus/src-tauri/src/tools.rs](../nexus/src-tauri/src/tools.rs#L316-L399).

The workspace directory comes from `MAVERICK_WORKSPACE_DIR` when that environment variable is set, otherwise `<cwd>/workspace` (created if missing). Nothing in the UI can override it, so a session opened from a different working directory lands in a different workspace. See [tools.rs](../nexus/src-tauri/src/tools.rs#L222-L236).

## Write shim

`write_to_file`, `write_file`, and `create_file` are handled directly by `AgentLoop`, outside the bridge dispatch path. The shim accepts relative or absolute paths, creates parent directories, and writes directly. See [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L633-L655) and [tools.rs](../nexus/src-tauri/src/tools.rs#L263-L310).

This is a meaningful boundary to remember when reviewing safety claims: not every write path flows through the same bridge/guard code.

## Permission guard

`PermissionGuard` evaluates explicit deny lists first, then allow lists, then a default policy. Reads pass by default; `search_replace` and `todo_write` need approval when auto-approval is off. Decisions are appended to `tool-decisions.log` with a short argument preview.

The current `AppState` constructs the guard with `auto_approve: true` — the source comment reads "auto-approve for now", so the deny/allow/default policy is effectively bypassed for every session. `run_terminal_cmd` explicitly bypasses this guard because terminal safety is expected to be handled by the bridge. See [tools.rs](../nexus/src-tauri/src/tools.rs#L31-L135) and [commands.rs](../nexus/src-tauri/src/commands.rs#L76-L78).

## Automation guidance

Fresh sessions receive a system addendum that tells the model to run commands likely to exceed roughly 60 seconds in the background, poll with bounded waits, kill stray jobs, and checkpoint progress with todos. The budget-warning prompt repeats the background-leftovers reminder. See [prompts.rs](../nexus/src-tauri/src/prompts.rs#L1-L21) and [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L50-L63).

## Output shaping

Tool results larger than 12,000 bytes are head-and-tail truncated with UTF-8-safe boundaries before entering conversation history. Per-tool budgets can only reduce that default. Failed results receive an idempotent `[TOOL_ERROR] ` marker. See [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L68-L71) for the default, [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L164-L186) for the truncation helper, [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L600-L610) for where a result is budgeted and stored, and [agent_loop.rs](../nexus/src-tauri/src/agent_loop.rs#L197-L205) for the error marker.

## Related notes

- [[03 Nexus Architecture]]
- [[04 Request Lifecycle]]
- [[07 Persistence and Configuration]]
- [[11 Current Work and Risks]]
