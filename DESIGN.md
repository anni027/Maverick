# Design

<!-- impeccable:design-schema 1 -->

## Visual World

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
   - Inline minimalist capsule: `[>_] Ran terminal cmd: cargo test (350ms) ✓ Done ▾`
   - Collapsed once finished to avoid cluttering chat history.
   - Expands into an ultra-clean terminal viewer with copy button and duration.

3. **Floating Bottom Composer Dock**:
   - Centered 768px pill floating above bottom edge.
   - Left `+` icon for attachments and MCP actions.
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
   - Center-left: Model selector pill (`Qwen 2.5 Coder ▾` / `GPT-4o ▾`).
   - Right: Clean Settings icon and New Chat icon.
