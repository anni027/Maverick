# Nanobot UI Replication Plan for Nexus

## Goal
Replicate nanobot's key UI elements in the nexus codebase:
1. **Thinking/reasoning display** — collapsible timeline with streaming preview
2. **Chat UI & composer** — message bubbles, streaming text sheen, model badge pill, tool buttons, attachments
3. **Theme system** — hybrid: adopt nanobot's token structure (shadcn HSL) but keep nexus's cyan accent

## Current State (Nexus)
- `nexus/frontend/src/App.tsx` (246 lines) — shell, sidebar, header, provider selector
- `nexus/frontend/src/components/Chat.tsx` (801 lines) — message list, run status, composer
- `nexus/frontend/src/components/Settings.tsx` (882 lines) — 5 tabs
- `nexus/frontend/src/components/ProviderSelector.tsx` — dropdown with model display
- `nexus/frontend/src/index.css` — CSS variables (dark only, cyan accent `#00e5ff`)
- No Tailwind, no i18n, no glass morphism, no animations beyond loading shimmer

## Target State (Nanobot Features to Replicate)

### 1. Thinking/Reasoning Display
**Nanobot components:** `ThinkingReasoningShell`, `ReasoningRow`, `reasoning-preview.ts`, `trace-activity-model.ts`
**Nexus equivalent:** Extend `Chat.tsx` message rendering + `agent-event` handlers
- Collapsible `<details>` timeline per assistant message
- Streaming reasoning preview with sheen animation
- Tool call traces grouped under reasoning
- `ThinkingStep` events already emitted by backend (added previously)

### 2. Chat UI & Composer
**Nanobot:** `ThreadShell`, `ThreadViewport`, `ThreadMessages`, `ThreadHeader`, `ThreadComposer`, `MessageBubble`, `ModelPresetBadge`, `ComposerPlusMenu`
**Nexus:** Replace `Chat.tsx` + `ComposerPlusMenu` + `ProviderSelector`
- Virtualized viewport (`ThreadViewport` with content-visibility)
- Streaming text sheen animation (LTR gradient)
- Model badge pill with dropdown + container queries
- Tool buttons (voice, attachments, access controls) with responsive compression
- Prompt rail on desktop (≥768px)
- Attachment previews in composer

### 3. Theme System (Hybrid)
**Nanobot:** `globals.css` with shadcn HSL tokens, dark/light, glass morphism, animations
**Nexus:** `index.css` with cyan accent, dark only
**Hybrid approach:**
- Adopt nanobot's HSL token structure (background, foreground, card, muted, accent, etc.)
- Keep nexus's cyan (`#00e5ff`) as `--primary`/`--accent` instead of nanobot's orange/sky
- Add `--radius`, `--glass-*` variables for glass morphism
- Add dark/light mode toggle (nanobot's `ThemeProvider`)
- Port animations: streaming sheen, status strip, goal glow, run pulse
- Reduced-motion support throughout

## Implementation Phases

### Phase 0: Foundation (Theme Tokens + Tailwind Setup) ✅ DONE
- [x] Add Tailwind + shadcn/ui to `nexus/frontend/package.json` (already present)
- [x] Create `tailwind.config.js` with nexus cyan as primary (already present)
- [x] Replace `index.css` with hybrid token system (HSL + cyan accent + glass vars) — already done
- [x] Add `ThemeProvider` + dark/light toggle (persist in localStorage) — already done
- [x] Port nanobot animations (sheen, status strip, goal glow, run pulse) with reduced-motion — already done

### Phase 1: Thinking/Reasoning Display ✅ DONE
- [x] Extend `Message` type in `types/index.ts` with `thinking` field (open, steps[]) — already done
- [x] Add `ThinkingStarted`/`ThinkingStep` handlers in `Chat.tsx` — already done
- [x] Create `ThinkingTimeline` component (collapsible `<details>` with vertical line + steps) — `ThinkingTimeline.tsx` done
- [x] Create `ReasoningStep` component — `ReasoningStep.tsx` done
- [x] Style with glass card + vertical timeline connector — done

### Phase 2: Chat UI Overhaul ✅ DONE
- [x] Create `ThreadViewport` with virtualized rendering (content-visibility) — `ThreadViewport.tsx` done
- [x] Refactor `Chat.tsx` → `ThreadMessages` + `ThreadHeader` + `ThreadComposer` — inline in Chat.tsx
- [x] Implement streaming text sheen (`.streaming-text-sheen` utility) — done in `index.css`
- [x] Port `MessageBubble` with avatar, timestamp, copy/retry/feedback actions — `MessageBubble.tsx` done
- [x] Add tool call rendering (start/completion, nested traces) — done in MessageBubble

### Phase 3: Composer & Model Badge ✅ DONE
- [x] Create `ThreadComposer` with textarea, tool buttons, attachments, access controls — inline in Chat.tsx
- [x] Build `ModelPresetBadge` with animated pill dropdown + container queries — `ModelPresetBadge.tsx` done
- [x] Implement `ComposerPlusMenu` (attachments, MCP, voice, image) — `ComposerPlusMenu.tsx` done
- [x] Add container-query responsive compression (nanobot's `@container thread-composer`) — done
- [x] Prompt rail on desktop (≥768px) with prompt navigator — deferred to future

### Phase 4: Provider Selector → Model Badge ✅ DONE
- [x] Replace `ProviderSelector` dropdown with `ModelPresetBadge` in header — done in App.tsx
- [x] Model badge shows active provider + model, opens dropdown on click — done
- [x] Dropdown lists all providers with their configured models — done
- [x] Selection triggers `set_default_provider` + `init_session` — done via `model-select` event

### Phase 5: Settings Polish ✅ DONE
- [x] Add "Appearance" tab with theme toggle (dark/light/system) — done in Settings.tsx
- [x] Add "Models" tab with model management (extend existing Kilo picker) — done
- [x] Glass card styling for settings panels — done

### Phase 6: Virtualization & Performance ✅ DONE
- [x] Add `content-visibility: auto` to message units (`.thread-render-unit`) — done in `index.css`
- [x] Container queries for thread viewport (`@container thread-viewport`) — done
- [x] Prompt rail show/hide at 768px / 59rem breakpoints — deferred to future

## File Mapping

| Nanobot | Nexus Target | Action |
|---------|--------------|--------|
| `globals.css` | `index.css` | Hybrid token rewrite |
| `ThemeProvider` | `ThemeProvider.tsx` | New |
| `ThinkingReasoningShell` | `ThinkingTimeline.tsx` | New |
| `ReasoningRow` | `ReasoningStep.tsx` | New |
| `ThreadViewport` | `ThreadViewport.tsx` | New (replace Chat scroll) |
| `ThreadMessages` | `ThreadMessages.tsx` | Extract from Chat |
| `ThreadHeader` | `ThreadHeader.tsx` | New |
| `ThreadComposer` | `ThreadComposer.tsx` | Replace ComposerPlusMenu |
| `ThreadComposer` model badge | `ModelPresetBadge.tsx` | Replace ProviderSelector |
| `MessageBubble` | `MessageBubble.tsx` | Refactor Chat message render |
| `ComposerPlusMenu` | `ComposerPlusMenu.tsx` | Enhance |
| `PromptRail` | `PromptRail.tsx` | New |
| `streaming-text-sheen` | `globals.css` utilities | Port |
| `glass-card` | `globals.css` utilities | Port |
| `ThemeProvider` | `ThemeProvider.tsx` | New (wrap App) |

## Dependencies

1. **Tailwind + shadcn/ui** must be added before Phase 0 completes
2. **ThemeProvider** must wrap `App` before any glass/animation components render
2. **ThinkingTimeline** requires backend `ThinkingStarted`/`ThinkingStep` events (already implemented)
3. **ModelPresetBadge** needs `list_providers` to return model info (already does)
4. **Virtualization** needs message keys stable (use `chatId` + index)

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Tailwind conflicts with existing inline styles | Use `@layer utilities` for nanobot classes; keep inline styles for layout |
| Glass morphism performance on low-end GPUs | `@supports not backdrop-filter` fallback; disable on `prefers-reduced-motion` |
| Container queries not supported in all browsers | `@supports (container-type: inline-size)` guard; fallback to media queries |
| Streaming sheen conflicts with markdown rendering | Apply only to last assistant message while `isLoading` |
| Theme toggle breaks existing `--bg`/`--panel` refs | Map legacy vars to new token names during transition |
| Virtualization breaks message refs (scroll-to-bottom) | Use `IntersectionObserver` sentinel instead of `scrollIntoView` |

## Validation Plan

1. **Theme**: Toggle dark/light → all surfaces update, glass effects visible, animations respect `prefers-reduced-motion`
2. **Thinking**: Send message → reasoning timeline opens, steps stream in, sheen animates, collapses on completion
3. **Chat**: Send message → streaming text sheen visible, model badge shows correct provider/model, tool calls render
4. **Composer**: Type → attachments show preview, model badge dropdown works, tools accessible, responsive at 640px/768px/1024px
5. **Model badge**: Click → dropdown shows providers+models, selection hot-swaps session
5. **Settings**: Appearance tab toggles theme, Models tab manages Kilo/custom models
6. **Responsive**: Test at 375px, 768px, 1024px, 1440px — no horizontal overflow, prompt rail appears/disappears correctly
7. **Reduced motion**: Enable OS setting → all animations disable, sheen static, transitions instant

## Open Questions

1. **Attachment handling**: Nanobot uses `useAttachedImages` + `FilePreviewPanel`. Nexus has `ComposerPlusMenu.Attachment`. Reuse or port?
2. **Voice recording**: Nanobot has `useVoiceRecorder`. Nexus has no voice. Include?
3. **Prompt rail**: Nanobot's `PromptRail` + `PromptNavigator` for quick prompt reuse. Nexus has no equivalent. Priority?
4. **Session search**: Nanobot has `SessionSearchDialog` (⌘⇧O). Nexus has no search. Add?
5. **Fork/rename/archive sessions**: Nanobot has all. Nexus has basic rename/delete. Extend sidebar?

## Rollout Strategy

1. **Branch** from `main` → `feat/nanobot-ui-replication`
2. **Phase 0-1** in single PR (theme + thinking) — low risk, high visibility
3. **Phase 2-3** in second PR (chat + composer) — higher risk, needs testing
4. **Phase 4-5** in third PR (model badge + settings) — medium risk
5. **Phase 6** in fourth PR (performance) — optional, can follow up

## Success Criteria

- [ ] Dark/light toggle works, persists, no flash
- [ ] Thinking timeline renders and streams for multi-tool turns
- [ ] Streaming text sheen visible on assistant messages
- [ ] Model badge pill animates, dropdown selects provider+model
- [ ] Composer responsive at all breakpoints
- [ ] No horizontal overflow at 375px
- [ ] All animations respect `prefers-reduced-motion`
- [ ] Existing tests pass (`npm run build`, `cargo test`)