# Learnings: frontend

## Codebase Patterns
- SolidJS with fine-grained reactivity for high-frequency real-time updates, 7KB bundle
- Triage Queue is the primary active surface, not the Kanban board
- Three-tier human attention model: Decision Queue → Notification Digest → Passive Dashboard
- Four-level interrupt enum (P0-P3) in schema, rendered as 2 levels in v0 UI
- WS singleton in ui/src/lib/ws.ts exports useWebSocket() hook and named connect/disconnect functions (added: 2026-03-23, dispatch: T22)
- Route paths: /triage, /board, /agents (added: 2026-03-23, dispatch: T22)
- Use `Router root=AppLayout` for persistent layout shells in @solidjs/router v0.15 (added: 2026-03-23, dispatch: T28)
- attentionStore.ts exports p0Count, p1Count, setP0Count, setP1Count signals — T25 should wire these (added: 2026-03-23, dispatch: T28)
- Keep store actions as pure functions for vitest node testing without SolidJS context (added: 2026-03-23, dispatch: T23)
- createVirtualizer requires scroll container ref via getScrollElement returning HTMLElement|null (added: 2026-03-23, dispatch: T23)
- boardStore is a singleton createStore — tests mutate shared state, need manual restore (added: 2026-03-23, dispatch: T24)

## Gotchas
- vitest default environment should be 'node' for non-DOM tests; 'jsdom' requires separate install (added: 2026-03-23, dispatch: T22)
- @solidjs/router v0.15 nests Route components differently than v0.13 (added: 2026-03-23, dispatch: T22)
- Vitest runs in ESM mode — use static import, not require() (added: 2026-03-23, dispatch: T28)
- Two separate VirtualSection instances each need their own scroll container; unified list needs a single virtualizer (added: 2026-03-23, dispatch: T23)
- HTML5 dragEnd fires on source element, not drop target — reset opacity on the card wrapper (added: 2026-03-23, dispatch: T24)

## Preferences
- CSS modules for component styling (added: 2026-03-23, dispatch: T28)

## Cross-Agent Notes
- T23/T24 should import createVirtualizer from ui/src/lib/virtual.ts, not directly from @tanstack/solid-virtual (added: 2026-03-23, dispatch: T22)
- T25 can call setP0Count/setP1Count from attentionStore.ts to drive sidebar badge (added: 2026-03-23, dispatch: T28)
- BoardView needs wiring into App.tsx routes — T24 did NOT modify App.tsx per constraints (added: 2026-03-23, dispatch: T24)
