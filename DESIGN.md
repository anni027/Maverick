# Design

<!-- impeccable:design-schema 1 -->

## Visual World

Minimal Ferrari × ChatGPT. ChatGPT's center-768 familiar, Ferrari's Rosso restraint. No marketing, no bento, no heavy carbon — just chat, whitespace, and one accent.

## Palette

- Bg #0A0A0A, Surface #111111, Panel #141414, Line #1E1E1E / #2A2A2A
- Text #EDEDED, Muted #8A8A8A, Faint #5A5A5A
- Rosso #E30613 (only for focus, primary, selection), Rosso-dim rgba(227,6,19,0.12)
- Strategy: Restrained — 95% neutrals, 5% Rosso.

## Typography

- Body: Inter 400-600, 14px/1.7 for chat, 13px for UI, -0.02em for headings.
- Mono: JetBrains Mono 400-500, 11px, for meta, tools, inputs.
- No display serif, no Barlow heavy — minimal needs quiet type.

## Materials & Elevation

- No carbon weave, no gradients. Panel is 1px Line on Bg, radius 12px (chat) / 8px (inputs) / 999px (pills).
- Button: 1px Line, 8px radius, hover #212121, active scale 0.98. Primary Rosso on text/Bg.
- Input: Bg #0F0F0F, Line, 8-16px radius, focus Line-2.

## Layout

- Sidebar 260, Bg #0F0F0F, right 1px Line, top New Chat full-width text/Bg, history list 8px radius, bottom user.
- Header 56, Bg Bg, bottom 1px Line, left toggle 36px, center Mark 22 + NEXUS, model selector pill, tools count pill.
- Main: centered 760 max, chat scroll, input dock bottom 1px top, 768 centered, 8px radius.

## Iconography

- Authored SVG only, stroke 1.3-1.6, 12-16px, consistent. No emoji, no Lucide default, no power-status badge.

## Motion

- Single enter 0.5s ease, reduced-motion snaps.

## States

- Empty: 48px rounded logo, welcome, 4 example prompts as 12px radius panels, no mock.
- Loading: 2px Rosso line shimmer, step mono, fallback 10s.
- Tool: 12px radius panel with Rosso dot.

## Responsive

- Tauri 1200x800, sidebar collapses, header single line, mobile not primary.

## Accessibility

- Focus 1px Line-2, caret text, selection Rosso/white, scrollbar thin, WCAG AA on dark.
