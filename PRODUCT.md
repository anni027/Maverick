# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

delegated: Tauri 2.11 (Rust 1.94 + WebView) + React 18 + Vite 5 + TypeScript. Chosen because repo already ships this; UI is web tech rendered in Tauri. No separate native mobile.

## Users

Primary: developers and technical operators using Nexus as a chat-first automation agent on desktop. Job: delegate shell/file/code tasks via natural language, inspect tool calls, manage sessions/providers/MCP servers without leaving chat. Secondary: power users evaluating provider backends (xAI/OpenAI/Anthropic/subprocess).

Situation: local desktop app, single window 1200x800, dark environment, long sessions, frequent tool output.

## Product Purpose

Nexus is a chat-first automation agent sliced from xAI Grok CLI into a single-process Rust core (AgentLoop + Provider trait + ToolBridge) with a pluggable provider system and a Tauri React chat UI. Success = user can start a session, send a message, see streaming assistant + tool calls execute via real local terminal/filesystem, switch providers, and persist history — without mock data or generic chrome.

## Positioning

Unlike generic chat wrappers, Nexus vendors ACP-free core crates and exposes a unified Provider abstraction (native LLM or external agent runtime as subprocess/MCP) so the AgentLoop never knows the backend. Mechanism competitors cannot copy: real tool bridge (run_terminal_cmd, read_file, search_replace, etc.) on local FS/terminal, JSONL session persistence, single-process Tauri.

## Operating Context

Workflows: new session → send message → streaming assistant → tool calls → tool results → follow-up. Also: manage API keys per provider, add/remove MCP servers (stdio/http), view tools, switch sessions.
Environment: Tauri window, Windows shell pwsh, temp dir app data (`nexus-app`), `config.toml` + `sessions/<id>/chat_history.jsonl`, protoc required for tools-api crate.
Rituals: headless demo (`MockProvider` + real ToolBridge) for architecture proof.

## Capabilities and Constraints

Capabilities: sessions list/create, provider registry (mock + xAI/OpenAI/Anthropic/subprocess), ToolBridge v1 subset (run_terminal_cmd, get_task_output, kill_task, read_file, search_replace, list_dir, grep, todo_write), MCP placeholder, config snapshot, UI prefs.
Constraints: Tauri Rust backend single process; frontend must use snake_case invokes (`session_id`, `provider_id`); no `h-screen`, respect `min-h-[100dvh]`; WebView2 required on Windows; icons must not be generic slop — no emoji, no Lucide default, no generic power-status badge.
Undecided: pricing, auth, cloud sync.

## Brand Commitments

Name Nexus. Voice: precise, technical, inspired by Ferrari — not playful, not retro. No mock data in UI (remove Mock as default display, no fake sessions/tools). No generic icons (no 🤖, no Phosphor generic set, no power-status ● LIVE badge). Ferrari inspiration is for *ChatGPT-like familiarity powered by Ferrari* — ChatGPT's recognizable sidebar + center chat + bottom input, but with Ferrari's material language (Rosso Corsa, Nero, Giallo is banned for this build — Ferrari Rosso only, sharp angular cuts, condensed type, carbon subtle). Do not clone Lamborghini/Ferrari trademarks.

## Evidence on Hand

Real: `src-tauri` Rust backend (AgentLoop, Provider, ToolBridge, SessionStore, ConfigManager), `frontend` React (App, Chat, SessionSidebar, ProviderSelector, Settings, ToolList), Tauri build with icons, headless demo passes (3 turns).
Absences: no real user sessions yet, no API keys, no MCP servers — UI must handle empty states without fabricating mock sessions/tools.
Paths: `frontend/src/App.tsx`, `frontend/src/components/*`, `src-tauri/src/*`, `PRODUCT.md` (this file).

## Product Principles

1. Chat is the product — everything else is chrome that must get out of the way after first viewport.
2. Tool calls are first-class content, not logs — show intent, args, and result with scanability.
3. No fabrication — empty means empty, with a clear next action; never invent sessions, tools, or provider data.
4. Ferrari precision — sharp geometry, tight tracking, single Rosso accent, material honesty over decoration.
5. Operate, not persuade — density for work, not marketing.

## Accessibility & Inclusion

Desktop chat: keyboard-first (Enter to send, Shift+Enter newline), focus-visible, WCAG AA contrast on dark, reduced-motion honored. No specific regulatory requirement beyond that.
