---
name: sprint1_complete
description: Wave 5 shipped (21 tasks, 341 tests), ready for Wave 6
type: project
---

Wave 5 fully shipped to main as of 2026-03-23.

**Cumulative stats:** 21 tasks completed across 5 waves, 341 tests passing (262 Rust + 79 frontend), 0 failures.

**Waves completed:**
- Wave 1 (7 tasks): Foundation — scaffold, data model, event store, state machine, WebSocket, adapter trait, config
- Wave 2 (8 tasks): Engine — actors, adapters, supervisor, worktree, hooks, templates, transitions
- Wave 3 (4 tasks): Human layer — approvals (T18), SolidJS scaffold (T22), interrupts (T25), CLI delivery (T46)
- Wave 4 (4 tasks): UI views — triage queue (T23), kanban board (T24), sidebar layout (T28), integration tests
- Wave 5 (5 tasks): Polish — transition scheduler (T20), agent detail (T26), keyboard nav (T29), health monitoring (T12), platform-native theme + UI refinement

**Remaining P1 tasks (Wave 6 candidates):**
- T13: Audit logging at adapter boundary
- T19: agent_dispatch hook type
- T30: Colorblind-safe status indicators
- T27: AI-summarized status
- T31-T35: Integrations (plugin interface, credentials, GitHub, webhooks, focus mode)

**How to run:** `cd ui && npm run build && cd .. && cargo run --bin molt-hub`

**Key patterns established:**
- 3-4 parallel worktree agents per sprint is the sweet spot
- Merge ordering matters for overlapping UI files (layout first, views second)
- css.d.ts needed for CSS module TypeScript support
- Platform-native theme: system fonts, prefers-color-scheme, accent-color, color-scheme: light dark
- Keyboard nav uses CustomEvents for decoupling from views
