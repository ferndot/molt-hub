## Session Handoff: 2026-03-23 — Sprint 2 (Waves 10-11) + UI Polish

### What Got Done
- **Wave 10**: Installed solid-icons (Tabler), replaced all emoji/entity icons across 9+ files, built Agents list view (/agents) with filtering + status tabs, renamed TriageSidebar → InboxSidebar
- **Wave 11**: GitHub import UI dialog, evolved inbox into heterogeneous notification panel with type filtering, wired Axum actors to WS broadcast channels (board/triage/metrics/agent output), added periodic health metrics broadcast
- **UI polish**: Cleaned up import dialog icons + inline styles, restructured sidebar (MC top, agents middle, Settings bottom), added --chrome-* CSS vars for theme-aware status bar, made sidebar + status bar adapt to light/dark, fixed traffic light spacer
- **Tauri**: Fixed beforeDevCommand path, installed tauri-cli, launched desktop app successfully

### Key Decisions
- **Sidebar structure**: Removed redundant "Agents" nav link — sidebar IS the agent list. Only "Mission Control" as top nav, Settings pinned to bottom. (rejected: keeping all 3 nav links)
- **Theme variables**: Introduced `--chrome-*` token family for status bar/toolbar chrome rather than reusing sidebar or bg vars. (rejected: making sidebar always-dark — user wanted it to adapt)
- **Sidebar adapts to theme**: Sidebar is now light gray in light mode, dark in dark mode. Previously was always-dark by design. User explicitly requested light adaptation.
- **Icon library**: solid-icons with Tabler (TbOutline*) — consistent, modern, tree-shakeable. (rejected: custom SVGs, lucide-solid)

### Patterns & Discoveries
- Hardcoded hex colors compound as tech debt — establish CSS variable tokens BEFORE building components
- `color-mix(in srgb, ...)` works well for sidebar search input backgrounds that adapt to any theme
- Tauri `beforeDevCommand` runs from the Tauri crate directory, not workspace root — relative paths need to account for this
- Vite HMR cache doesn't recover from file renames — dev server restart required after TriageSidebar → InboxSidebar rename

### In-Progress Work
- **Inbox toggle position**: User wants it at top-right of the app (like a header bar), not in the status bar. Currently still in StatusBar.tsx. Need to move toggle + badge into a new top header bar in AppLayout, above the main content area.
- **Sidebar agent list scaling**: Works fine for 4-5 agents but needs optimization for 50+ — consider virtualized list (TanStack Virtual already in deps), status grouping/collapsing, or a compact single-line mode.

### Uncommitted Changes
- `.claude/tackline/memory/team/retro-history.md` — retro entry appended
- Session snapshot files rotated by hooks

### Open Questions
- **AppLayout.tsx — inbox toggle placement**: Move to a thin top bar spanning the main content area? Or put it in the traffic light spacer area on the right side? Options: (A) New `<TopBar>` component between body and status bar — gives room for breadcrumbs/context later, (B) Float it in the top-right corner of the main content area as a fixed-position button. Criteria: where the user naturally looks for notifications. Ask: user for preference on next session.
- **Sidebar agent list at scale**: For 50+ agents, should we (A) group by status with collapsible sections, (B) use virtualized scrolling, (C) cap visible list at ~10 with "show all" expansion, or (D) hide sidebar list entirely and rely on /agents view? Criteria: how many agents user typically runs. Ask: user for typical agent count.

### Recommended Next Steps
1. **Move inbox toggle to top-right**: Create a `TopBar` component in AppLayout that sits above the main content, holds the inbox toggle + badge + unread count. Remove inbox props from StatusBar.
2. **Optimize sidebar agent list**: Add status grouping (Running > Waiting > Blocked > Done) with collapsible sections and agent count per group. Consider TanStack Virtual if list exceeds ~20.
3. **Begin T08 (Claude Code SDK adapter)**: This is the critical path to live data. Read `crates/harness/src/adapter.rs` for the AgentAdapter trait, then implement a Claude Code adapter that spawns `claude` CLI and parses structured output.

### Risks & Warnings
- Tauri `beforeDevCommand` is currently empty string — works when Vite is already running, but `cargo tauri build` uses `beforeBuildCommand` which still has the `npm --prefix ui run build` path (may need similar fix)
- 15 old worktree branches exist from previous sprints — consider pruning with `git branch -D` to reduce clutter
- 589 tests passing but no tests cover the new notification panel or agents view (mock data, no stores tested)
