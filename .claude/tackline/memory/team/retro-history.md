# Retrospective History

## Retro: 2026-03-23 (Sprint 1 — Wave 0+1 Foundation)
- Tasks completed: 7/7 (T01, T02, T03, T04, T06, T07, T14)
- Commits: 9 (7 feat, 2 chore) — 2,259 lines across 27 files
- New learnings: 12 across 3 active members (architect: 8, backend: 4, infra: 2)
- Pruned/archived: 0 entries (first sprint, all fresh)
- Key insight: Worktree isolation fails when git init happens mid-session; serial dispatch with file-scope isolation is the fallback. Agent design decisions that deviate from spec (rejection routing) must be caught and reviewed before downstream work begins.

## Retro: 2026-03-23 (Sprint 2 — Wave 2 Execution Layer)
- Tasks completed: 8/8 (T05, T08, T09, T10, T11, T15, T16, T17)
- Commits: 4 merged (16 total session) — 130 tests across 3 crates
- New learnings: 16 across 3 active members (architect: 4, backend: 7, infra: 5)
- Pruned/archived: 0 entries (all files under 25 lines)
- Key insight: Fix all known compile errors on main BEFORE dispatching agents — all 8 agents independently wasted cycles fixing the same 2 pre-existing bugs. Worktree isolation works perfectly now; 7 parallel agents ran without conflicts.

## Retro: 2026-03-23 (Wave 3)
- Tasks completed: 4 (T18, T22, T25, T46)
- Tests: 130 → 206 (+76, +58%)
- New learnings: 9 across 3 members (backend, frontend, infra)
- Pruned/archived: 0 entries
- Fix rate: 0% (all feat commits, no corrections)
- Key insight: Parallel worktree dispatch of 3 agents with clean auto-merge is the sweet spot. All three Rust agents produced correct code on first try with zero rework. Frontend activation smooth.
- Process note: T46 had a true dependency on T22 and was correctly sequenced serial.

## Retro: 2026-03-23 (Wave 4)
- Tasks completed: 4 (T23, T24, T28, integration tests)
- Tests: 206 → 280 (+74, +36%)
- New learnings: 11 across 2 members (frontend +8, tester +3)
- Pruned/archived: 0 entries
- Fix rate: 12.5% (1 fix commit out of 8 — CSS module types + TS strict comparison)
- Merge conflict: 1 (T23 App.tsx vs T28 layout — resolved by combining imports)
- Key insight: 3 frontend agents in parallel worktrees worked despite all touching ui/. Only one merge conflict on App.tsx, easily resolved. Tester agent activated cleanly on first dispatch, produced 36 integration tests.
- Process note: Build step revealed missing CSS module type declarations — agents should add css.d.ts as standard scaffold step.
- Tester cold start: Successful. Agent read existing code thoroughly and produced well-structured integration tests covering approval flow, attention flow, and actor lifecycle.

## Retro: 2026-03-23 (Waves 7-9 — Jira + OAuth + UX)
- Tasks completed: ~15 across 3 waves
- Commits: 23 this session (15 feat, 2 fix, 6 merge)
- Tests: 387 → 534+ (+147, +38%)
- New learnings: 13 across 4 members
- Fix rate: 8.7% (2 fix / 23 — missing DomainEvent match arms after core changes merged)
- Key insight: After merging core enum changes, run cargo check on main BEFORE dispatching downstream agents. Two fix commits were needed for match arms the architect's new DomainEvent variants broke in classifier.rs and router.rs.
- Process: 11 agents across 3 waves. User feedback mid-sprint (OAuth to PKCE, column config to Board, Triage to Inbox) incorporated via follow-up dispatch waves.
- UX decisions: header to status bar, column config on Board not Settings, OAuth PKCE for distributable, settings persist to backend SQLite, status bar shows CPU/agent metrics.

## Retro: 2026-03-23 (Bug Fix Sprint — UI Polish)
- Tasks completed: 2 (status bar flashing, redundant stage chip)
- Commits: 4 (2 fix, 1 revert, 1 fix) — net 2 meaningful changes
- Fix rate: 25% (1 misdiagnosed component, reverted)
- Key insight: When user reports a UI bug with specific visible text, grep for that exact string first — semantic exploration found the wrong component. Also: small <10-line fixes are faster as direct edits than worktree agent dispatches.

## Retro: 2026-03-23 (Feature Sprint — Desktop + Wiring + Integrations)
- Tasks completed: 7 (Tauri shell, theme switching, Settings redesign, sidebar resize, Focus Mode, WS wiring, GitHub integration)
- Commits: 15 (7 feat, 6 fix, 1 revert, 1 cleanup)
- Tests: 181 UI + 28 Rust server (14 new GitHub + broadcast tests)
- Parallel agents: 3 dispatched for WS/GitHub/Focus — all succeeded, zero conflicts
- Key insight: Status bar flicker required 3 fix attempts — should read the full signal chain upfront before patching symptoms. Delete generated .module.css.d.ts files; the wildcard css.d.ts suffices.
- Process: Direct edits for small fixes saved time vs agent dispatch. User screenshot feedback was the most productive iteration loop.
- Remaining: GitHub import UI, Tauri packaging/icon, real actor→WS broadcast, board DnD in Tauri.

## Retro: 2026-03-23 (Sprint 2 — Waves 10-11: Icons + Notifications + Theme)
- Tasks completed: 10 (icon pack, agents view, rename, GitHub import UI, notification panel, WS broadcast, sidebar restructure, theme vars, chrome vars, traffic light fix)
- Commits: ~10 (4 feat, 3 fix, 1 refactor, 2 merge) — 2,251 lines added, 388 removed
- Tests: 181 UI + 408 Rust = 589 total (stable, no regressions)
- Fix rate: 30% — theme/sidebar polish required multiple iterations based on live user feedback
- Key insight: Hardcoded hex colors in component CSS are tech debt that compounds — should establish CSS variable tokens (--chrome-*, --sidebar-*) BEFORE building components, not after. Retrofitting is expensive.
- Process: User-driven iteration via Tauri desktop screenshots was the most productive feedback loop. Small direct edits for UI polish >> worktree agents. Sidebar redundancy (nav item + section) caught by user, not by agents.
- Remaining open from user: inbox toggle should move to top-right header (not status bar), sidebar agent list needs optimization for 50+ agents
