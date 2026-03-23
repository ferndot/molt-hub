---
name: sprint1_complete
description: Wave 4 shipped (4 tasks, 280 tests), ready for Wave 5
type: project
---

Wave 4 fully shipped to main as of 2026-03-23.

**Cumulative stats:** 16 tasks completed across 4 waves, 280 tests passing (238 Rust + 42 frontend), 0 failures.

**Wave 4 tasks completed:**
- T23: Triage Queue view (priority-sorted, quick-action buttons, virtual list)
- T24: Kanban board (drag-and-drop, expandable cards, mock data)
- T28: Sidebar + main layout (nav, agent list, attention badges)
- Integration tests: 36 cross-cutting server tests (approval flow, attention flow, actor lifecycle)

**Wave 5 tasks (next — from epic priority order):**
- T12: Activity-based health monitoring
- T13: Audit logging at adapter boundary
- T19: agent_dispatch hook type
- T20: Time-based transition scheduler
- T26: Agent Detail Panel
- T29: Keyboard navigation
- T30: Colorblind-safe status indicators

**Why:** Wave 4 delivered the first usable UI. Wave 5 adds polish, monitoring, and the remaining P1 features.

**How to apply:** The app is now usable: `cd ui && npm run build && cd .. && cargo run --bin molt-hub`. Triage queue, kanban board, and sidebar layout all functional with mock data.
