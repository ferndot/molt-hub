# Handoff: 2026-03-23 Session 2

## Last Commit
b92a76a feat(ui): lift inbox sidebar to AppLayout, add status bar toggle

## What Was Done
- **UI Bug Fixes**: Status bar flashing (4 fixes: aria-live, memoize, debounce, reconnect), redundant stage chip, sidebar resize jank
- **Tauri v2 Desktop Shell**: crates/tauri with embedded Axum server, overlay titlebar, traffic light integration, dev HMR (points to Vite), dragDropEnabled:false for HTML5 DnD
- **Theme Switching**: data-theme attribute + color-scheme CSS, light/dark/system all work
- **Settings Redesign**: Left nav + right detail panel, Integrations nested with Jira/GitHub cards
- **WebSocket Wiring**: board/triage/metrics stores connected to backend WS events, broadcast helpers in ws_broadcast.rs
- **GitHub Integration**: REST client + API handlers (crates/server/src/integrations/github_*)
- **Focus Mode**: Hidden count per column, active button state, dimmed empty columns
- **Inbox Sidebar**: Lifted to AppLayout (global), toggle in status bar with count badge

## Architecture Decisions
- Sidebar toggle: Claude-style icon in traffic light area (left), SVG panel icon in status bar (right/inbox)
- Inbox is global (AppLayout), not per-view
- Tauri dev mode: webview → localhost:5173 (Vite HMR), release → embedded Axum on 3001
- Server router extracted to `crates/server/src/serve.rs` for reuse by Tauri

## Open Items — Priority
- [ ] **Evolve inbox into app-wide notification panel** — currently only shows triage/attention items. Should support heterogeneous notification types (agent completed, build failed, PR merged, approval needed). Promote the toggle from the status bar to a prominent top-right position with always-visible badge count + pulse animation on new items. This is the highest-impact UX change remaining.
- [ ] GitHub import UI dialog (frontend, matching Jira pattern)
- [ ] Better icons for nav items (currently emoji placeholders ⚡◉⚙)
- [ ] Rename TriageSidebar.tsx → InboxSidebar.tsx (or NotificationSidebar.tsx) to match evolved naming

## Open Items — Polish
- [ ] Tauri app icon + packaging for distribution
- [ ] Wire Axum actors to WS broadcast on real state changes (currently mocked)
- [ ] Board DnD verification in Tauri desktop (dragDropEnabled:false should fix it)
- [ ] Status bar still shows mocked CPU/memory — wire to real health metrics

## Test Status
- Frontend: 181 tests passing (14 files)
- Rust server: 28 tests passing (unit + integration)
- Build: `npm run build` + `cargo build -p molt-hub-desktop` both succeed
