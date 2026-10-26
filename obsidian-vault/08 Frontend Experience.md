---
tags:
  - frontend
  - react
  - tauri
aliases:
  - UI architecture
  - Chat frontend
---

# Frontend experience

## Application shell

The React entry point renders `App` inside a Tauri WebView. The shell contains:

- session sidebar
- header with provider selector and settings
- centered chat column
- settings modal
- streaming message area
- floating input dock with attachment chips and a composer `+` menu

See [nexus/frontend/src/main.tsx](../nexus/frontend/src/main.tsx#L1-L11) and [nexus/frontend/src/App.tsx](../nexus/frontend/src/App.tsx#L195-L246).

## Initialization

`App` loads sessions, providers, the default provider, tools, and UI preferences; picks the first real session (or a fresh `session-<timestamp>` id on a first run); initializes the selected session; and retries with a fresh id if that open fails, keeping the error on screen instead of hiding it behind a placeholder session. It also listens for `agent-event` and surfaces initialization errors. See [App.tsx](../nexus/frontend/src/App.tsx#L41-L105).

## Chat component

`Chat` owns the visible message list and input state. It:

- loads persisted messages on session changes,
- subscribes to backend events,
- streams assistant text,
- renders tool-call start/completion,
- shows segment, spend, cancellation, and error states,
- sends messages through `invoke('send_message')`,
- cancels with `invoke('cancel_message')`,
- supports Enter to send and Shift+Enter for a newline,
- stages up to 6 file attachments (chips with size and remove) and serializes each send as `[Attached file: name]` fenced blocks ahead of the typed text,
- keeps run ownership (`runSessionId`) so switching sessions mid-run does not move the spinner or append events to the wrong transcript.

See [nexus/frontend/src/components/Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L47-L430).

## Composer `+` menu

The circular `+` button in the dock opens `ComposerPlusMenu`, a popover with four entries:

- **Attach file** — hidden multi-file picker; reads only the first 12 KB of each file, rejects NUL-containing (binary) content with an alert, caps at 5 MB on disk and 6 chips per message.
- **Workspace files** — browses the app workspace through `list_workspace_files` (depth 3, 500 entries, `node_modules`/`target`/`dist`/`__pycache__` and hidden dirs skipped, directories first); picking a file loads it with `read_workspace_file` (24 KB cap, NUL sniff, UTF-8-safe truncation) and stages it with its workspace-relative path as the chip name.
- **Skills** — lists hub skills and inserts `use skill "<name>"` at the textarea caret (the backend has no slash-command expansion; the model invokes the `skill` tool).
- **Add MCP server** — closes the menu and opens the existing MCP dialog (the old `＋ MCP` pill is gone).

The menu closes on Escape or outside click; sub-views share a back button and breadcrumb. See [ComposerPlusMenu.tsx](../nexus/frontend/src/components/ComposerPlusMenu.tsx#L57-L327) and [Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L417-L428).

## Session sidebar

`SessionSidebar` supports selection, new sessions, inline rename, and two-step delete confirmation. See [nexus/frontend/src/components/SessionSidebar.tsx](../nexus/frontend/src/components/SessionSidebar.tsx#L12-L80).

## Settings

Settings exposes:

- provider keys and custom endpoints,
- default provider,
- **Kilo AI Gateway model browser and selection** — modal catalog picker with search/filter, immediate model persistence via `set_provider_settings`, and hot-swap of active sessions. See [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L102-L153) and [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L722-L756).
- MCP server management and status,
- skills, hub fetching, and the GitHub-backed skills marketplace (source select, search, one-click install into the hub cache),
- UI preferences (tool bubbles, auto-scroll, compact spacing — applied to the running app immediately; theme selection persists but has no light palette yet — component colors are all tokens in `index.css` `:root` now, so light mode needs a palette block rather than a component sweep),
- context/compaction controls,
- turn/segment budgets and spend guardrails.

See [nexus/frontend/src/components/Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L12-L21) and [Settings.tsx](../nexus/frontend/src/components/Settings.tsx#L329-L612).

## Visual implementation

The root design calls for a restrained dark interface with one Rosso accent and authored SVG icons. The current UI uses inline SVGs and dark surfaces, but copied labels still say Maverick. The empty-state suggestions are plain text. See [DESIGN.md](../DESIGN.md#L34-L46) and [Chat.tsx](../nexus/frontend/src/components/Chat.tsx#L457-L483).

## Related notes

- [[01 Product and Design]]
- [[04 Request Lifecycle]]
- [[09 Provenance and Drift]]
