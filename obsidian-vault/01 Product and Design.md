---
tags:
  - product
  - design
  - nexus
aliases:
  - Nexus product brief
  - Product and visual design
---

# Product and design

## Product intent

[[00 Home|Nexus]] is described as a chat-first desktop automation agent for developers and technical operators. The user should be able to start a session, send a natural-language request, watch streaming assistant and tool events, manage providers and MCP servers, and persist history without leaving the chat.

The product document identifies the stack as Tauri 2.11, Rust, WebView, React 18, Vite 5, and TypeScript. The operating context is a single 1200×800 desktop window with long sessions and frequent tool output.

Sources: [PRODUCT.md](../PRODUCT.md#L15-L37), [nexus/README.md](../nexus/README.md#L1-L25)

## Core workflow

1. Create or select a session.
2. Select a provider.
3. Send a message.
4. Stream assistant text and tool calls.
5. Inspect tool results and continue the conversation.
6. Persist the session and resume it later.

The workflow is implemented across [[04 Request Lifecycle]] and [[08 Frontend Experience]].

## Product principles

- Chat is the product; settings and history are secondary chrome.
- Tool calls are first-class content, not opaque logs.
- Empty states must remain empty; do not fabricate sessions, tools, or provider data.
- The visual language is restrained, technical, and precise.
- The application is for operating on a local machine, not persuading the user.

Source: [PRODUCT.md](../PRODUCT.md#L49-L55)

## Visual system

The root design document calls for a dark, minimal interface with one Rosso accent:

- Background `#0A0A0A`, surfaces around `#111111`–`#141414`, and muted text around `#8A8A8A`.
- Rosso `#E30613` reserved for focus, primary actions, selection, and small status cues.
- Inter for UI/body text and JetBrains Mono for metadata and tools.
- A 260px sidebar, 56px header, centered 760px chat column, and bottom input dock.
- Authored SVG icons, subtle motion, and reduced-motion support.

Source: [DESIGN.md](../DESIGN.md#L5-L54)

## Branding drift

The root documents use **Nexus**, while the copied Nexus code and runtime still contain **Maverick** in package names, module documentation, UI labels, and the app-data directory. Treat the root product/design documents as the intended naming direction and the copied files as current implementation evidence until a deliberate rename is completed.

See [[09 Provenance and Drift]] for the concrete mismatches.
