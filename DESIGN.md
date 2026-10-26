# Design

<!-- impeccable:design-schema 1 -->

## Visual World

<<<<<<< HEAD
Pure Minimalist ChatGPT. Distraction-free, monochrome clarity, generous whitespace, center-768 content column, collapsible thinking and code execution drawers. No loud neon, no arbitrary badges, no intrusive headers.

## Palette

### Dark Mode (Primary)
- Main Background: `#212121`
- Sidebar: `#171717`
- Surfaces & Composer: `#2f2f2f`
- Hover States: `#383838`
- Subtle Dividers & Borders: `rgba(255, 255, 255, 0.08)` / `#303030`
- Text Primary: `#ececec`
- Text Muted: `#b4b4b4`
- Text Faint: `#8e8e8e`
- Accent / Submit: `#ffffff` (monochrome crisp contrast)

### Light Mode
- Main Background: `#ffffff`
- Sidebar: `#f9f9f9`
- Surfaces & Composer: `#f4f4f4`
- Hover States: `#ececec`
- Subtle Dividers & Borders: `#e5e5e5`
- Text Primary: `#0d0d0d`
- Text Muted: `#707070`
- Text Faint: `#9e9e9e`
- Accent / Submit: `#000000`

## Typography

- Body & Headings: `Inter`, system-ui, `-apple-system`, `BlinkMacSystemFont`, `sans-serif`. 15px / 1.7 line height for optimal conversational reading.
- Code & Terminal: `JetBrains Mono`, `ui-monospace`, `SFMono-Regular`, `monospace`. 13px with clean syntax contrast.

## Materials & Elevation

- Pure flat planes with 1px hairline borders (`rgba(255, 255, 255, 0.08)` or `#e5e5e5`).
- Radii:
  - Chat Bubble (User): `24px` (`rounded-3xl`)
  - Floating Composer Dock: `26px` (`rounded-[26px]`)
  - Tool Execution & Thinking Pills: `999px` (`rounded-full`) or `12px` (`rounded-xl`)
  - Buttons / Dropdowns: `8px` / `999px`
- Shadows: None on flat elements; subtle soft ambient diffusion on floating popups/dropdowns (`0 10px 25px -5px rgba(0, 0, 0, 0.3)`).

## Elements & Interaction (ChatGPT Specification)

1. **Reasoning Steps (o1/o3 style)**:
   - Collapsed by default after completion: `✦ Thought for 12 seconds ▾`
   - Subtle pulse during generation: `✦ Thinking...`
   - Expands to a clean indented transcript with a muted left border (`border-l-2 border-white/10`).

2. **Tool Calls & Command Execution (Code Interpreter style)**:
   - Inline minimalist capsule: `[>_] Ran terminal cmd: <command> <duration> ✓ Done ▾`
   - Collapsed once finished to avoid cluttering chat history.
   - Expands into an ultra-clean terminal viewer with copy button and duration.

3. **Floating Bottom Composer Dock**:
   - Centered 768px pill floating above bottom edge.
   - Multi-line textarea auto-expanding up to 200px.
   - Circular submit button (crisp white circle with black arrow up `↑` in dark mode).

4. **Sidebar Navigation**:
   - Width 260px, `#171717`.
   - Top: "New chat" button with subtle icon + text.
   - History: Clean grouping by chronology ("Today", "Previous 7 days", "Previous 30 days").
   - Hover reveals clean 3-dot menu for Rename and Delete.
   - Bottom: Minimalist profile / settings row.

5. **Header Bar**:
   - 52px high, distraction-free.
   - Left: Sidebar toggle icon.
   - Center-left: Model selector pill (`Provider ▾`).
   - Right: Clean Settings icon and New Chat icon.
=======
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
- Header 56, Bg Bg, bottom 1px Line, left toggle 36px, center Mark 22 + MAVERICK, model selector pill, tools count pill.
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
>>>>>>> 117c40dd1a8dd522105165e843dc400ec3f34dc3
