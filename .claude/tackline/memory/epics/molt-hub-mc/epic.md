# Epic: Molt Hub — Agent Mission Control

**Epic ID**: molt-hub-mc
**Created**: 2026-03-22
**Source**: /blossom
**Goal**: Create a mission control application for managing many simultaneous AI coding agents. Performant, customizable per-stage pipeline with instructions/skills/integrations/hooks. Human-in-the-loop without context-switch overload. Agent-harness agnostic. Mobile-accessible for approvals and status.

## Architecture Decisions

- **Backend**: Rust (Axum + Tokio) — process management at scale, shared Tauri toolchain, no GC pauses
- **Frontend**: SolidJS — fine-grained reactivity for high-frequency real-time updates, 7KB bundle
- **Database**: SQLite WAL — 30-40K inserts/sec, zero config, local-first. Event store from day 1 (not JSON lines).
- **Transport**: WebSocket (multiplexed, 3 message types: subscribe/unsubscribe/event) for streaming, Tauri IPC for commands (Phase 2)
- **State**: Event sourcing + CQRS. Events table is write side, projection tables (task_current_state, task_timeline) are read side. No actors — `tokio::spawn` + `Mutex<HashMap<TaskId, TaskHandle>>`.
- **State machines**: Rust enums with `transition(self, event) -> Result<TaskState, TransitionError>`. Compiler enforces exhaustive handling — primary mechanism preventing agent-introduced regressions.
- **Distribution**: Web UI via `molt-hub serve` (Phase 1), Tauri 2.0 shell (Phase 2)
- **Agent integration**: AgentAdapter trait (spawn, send, on_message, status, terminate, abort). acpx adapter primary, Claude SDK adapter, generic CLI adapter.
- **Agent isolation**: acpx permission deny as default posture. Pipeline stages declare required permissions. No OS-level user separation.
- **Container isolation**: Containers are a hook, not a layer. `StartDevEnvironment` hook kind on stage `on_enter`. Agent runs on host (acpx), container runs dev server, filesystem is integration point. Two auto-detected paths: no compose file → bollard single container; has compose file → shell out to `docker compose --project-name molt-{slug} up`.
- **Config**: YAML stage config with JSON Schema validation, Handlebars instruction templating with restricted variables
- **Previews**: Managed Caddy reverse proxy (bundled, started as child process). Routes registered via Caddy admin API on container start. `{service}.{worktree-slug}.molt.localhost` → container port. Embedded iframe in Agent Detail Panel.
- **Credentials**: System keychain via `keyring` crate. Pipeline-scoped aliases. fd-based injection (anonymous pipe, close-on-exec).
- **Audit**: Isolated writer — separate tokio task, bounded channel, dedicated SQLite DB. Agent processes never hold audit DB handle.
- **IDE access**: URI scheme buttons in UI (vscode://, cursor://, zed://), no CLI UX
- **Primary interface**: Web UI only. CLI is limited to `molt-hub serve` to start the server.

## Key Design Insights

1. **Triage Queue is the primary active surface**, not the Kanban board. Board provides structure; triage drives action.
2. **Three-tier human attention model**: Decision Queue (requires action) → Notification Digest (batched FYI) → Passive Dashboard (pull on demand).
3. **Four-level interrupt enum in schema** (P0-P3), rendered as 2 levels in v0 UI (needs attention / doesn't). Schema supports 4 from day 1 to avoid migration.
4. **AI-summarized status** as default display — raw agent output available on demand.
5. **Agent harness as protocol, not plugin** — AgentAdapter trait is the extension point. 4 methods to implement for a new backend.
6. **Container-as-hook** — dev environments are a stage lifecycle hook, not infrastructure. `StartDevEnvironment` on_enter, `TeardownDevEnvironment` on terminal stage. Agent (acpx on host) and container (dev server) share the worktree via volume mount. Filesystem is the integration point.
7. **Subdomain routing via managed Caddy** — routes registered/deregistered via Caddy admin API during hook execution. `{service}.{worktree-slug}.molt.localhost` → container port. No user-facing Caddy config.
8. **Preview hot-switching** — Agent Detail Panel embeds iframe pointing at subdomain; clicking between agent cards switches preview.
9. **Structural foundations over scope** — state machines, adapter trait, CQRS, SQLite, scoped credentials, and fd injection are restored because they make agent-written code reliable. Compiler-enforced invariants turn open-ended implementation into fill-in-the-blank for AI agents.
10. **Event schema**: 10 types (TaskCreated, TaskStageChanged, AgentAssigned, AgentOutput, TaskBlocked, TaskUnblocked, HumanDecision, AgentCompleted, TaskCompleted, TaskPriorityChanged). Per-task with session_id embedded. ULID ids, correlation (task_id) and causation (caused_by) chains from day 1. Stages are user-defined strings in project config, not Rust enum variants.

## Meeting Review (2026-03-23)

**Panelists**: Systems Architect, Security Engineer, Product Engineer
**Rounds**: 6 exchanges across architecture, security, product, simplification

### Key Resolutions
- **Restore structural foundations**: State machines, adapter trait, CQRS, SQLite WAL, AgentJail trait, scoped credentials, fd injection, isolated audit writer, AI summaries, context assembly, 4-level interrupt enum
- **Keep scope cuts**: Plugin system, customizable layouts, seccomp, network egress filtering, separate macOS user
- **Container model**: Containers are a hook, not a layer. Agent on host, container runs dev server, filesystem integration point.
- **Security posture**: acpx permission deny default, keyring-backed credential store with pipeline scoping, fd injection, isolated audit writer
- **Frontend**: SolidJS stays (user rejected htmx proposal). SSE considered but WebSocket retained for bidirectional needs.
- **Kanban**: Full drag-and-drop (user requirement). Stage-specific hook execution via hook executor engine.

## Spike Findings

### Items

1. **T01: Scaffold Rust workspace monorepo** — crates: core, server, harness, ui-api
   - source: Tech Stack spike
   - confidence: CONFIRMED
   - priority: P0
   - depends-on: none
   - agent: scaffolding agent — creates Cargo.toml workspace, crate structure, CI config

2. **T02: Define core data model** — Pipeline, Stage, Task, Agent, Team, Session, Event as Rust types
   - source: Core Architecture spike (item 3)
   - confidence: CONFIRMED
   - priority: P0
   - depends-on: T01
   - agent: domain modeler — touches core/src/model/. Requires DDD pattern knowledge.

3. **T03: Implement event store on SQLite WAL** — append-only log, batched writes, correlation/causation IDs
   - source: Core Architecture spike (items 1, 9, 10), Tech Stack spike (items 8, 10)
   - confidence: CONFIRMED
   - priority: P0
   - depends-on: T02
   - agent: persistence agent — touches core/src/events/, server/src/store/. Requires event sourcing patterns, SQLite WAL tuning.

4. **T04: Build state machine engine** — task progression through stages, Rust enums + guards/actions
   - source: Core Architecture spike (item 2), Stage Pipeline spike (item 19)
   - confidence: CONFIRMED
   - priority: P0
   - depends-on: T02
   - agent: state machine agent — touches core/src/machine/. Requires statechart theory, Rust enum patterns.

5. **T05: Implement actor-per-task concurrency** — each active task owns a tokio task + state machine instance
   - source: Core Architecture spike (item 4)
   - confidence: LIKELY
   - priority: P0
   - depends-on: T03, T04
   - agent: concurrency agent — touches server/src/actors/. Requires Tokio async patterns, actor model.

6. **T06: Build WebSocket multiplexing server** — single connection, sub/unsub per agent stream
   - source: UI spike (item 19), Tech Stack spike (item 11)
   - confidence: CONFIRMED
   - priority: P0
   - depends-on: T01
   - agent: transport agent — touches server/src/ws/. Requires Axum WebSocket, multiplexing protocol design.

7. **T07: Define AgentAdapter trait** — spawn, send, on_message, status, terminate, abort
   - source: Agent Harness spike (items 2, 3), Core Architecture spike (item 7)
   - confidence: CONFIRMED
   - priority: P0
   - depends-on: T02
   - agent: interface designer — touches harness/src/adapter.rs. Requires trait design, async stream patterns.

8. **T08: Implement Claude Code SDK adapter** — structured streaming, session persistence, cost tracking
   - source: Agent Harness spike (item 1)
   - confidence: CONFIRMED
   - priority: P0
   - depends-on: T07
   - agent: SDK integration agent — touches harness/src/claude/. Requires Claude Agent SDK knowledge.

9. **T09: Implement generic CLI adapter** — stdin/stdout/stderr, output parser plugins, heuristic progress
   - source: Agent Harness spike (items 2, 3, 7)
   - confidence: LIKELY
   - priority: P0
   - depends-on: T07
   - agent: CLI adapter agent — touches harness/src/cli/. Requires process management, output parsing.

10. **T10: Build process supervisor** — tokio::process + pty-process, graceful shutdown, orphan detection
    - source: Tech Stack spike (items 13, 14), Agent Harness spike (item 9)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T07
    - agent: supervisor agent — touches harness/src/supervisor.rs. Requires pty-process crate, signal handling.

11. **T11: Implement git worktree lifecycle manager** — create on assign, work in worktree, PR/merge on complete
    - source: Agent Harness spike (item 5)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T07
    - agent: git agent — touches harness/src/worktree.rs. Requires git2 crate, worktree management.

12. **T12: Implement activity-based health monitoring** — output timestamps, file change detection, stuck detection
    - source: Agent Harness spike (item 9)
    - confidence: LIKELY
    - priority: P1
    - depends-on: T10
    - agent: monitoring agent — touches harness/src/health.rs

13. **T13: Build audit logging at adapter boundary** — every spawn/send/terminate logged
    - source: Agent Harness spike (item 11)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T07
    - agent: logging agent — touches harness/src/audit.rs

14. **T14: Define stage configuration schema** — YAML + JSON Schema validation, hot-reload
    - source: Stage Pipeline spike (items 1, 2, 3)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T02
    - agent: config agent — touches core/src/config/. Requires serde_yaml, jsonschema crates.

15. **T15: Build hook executor engine** — ordered handlers, seq/parallel, failure policies, rich context
    - source: Stage Pipeline spike (items 5, 6, 7)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T14, T05
    - agent: hook engine agent — touches server/src/hooks/. Requires async execution patterns.

16. **T16: Implement instruction templating** — Handlebars-style, restricted variables, base inheritance
    - source: Stage Pipeline spike (items 10, 11, 12)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T14
    - agent: template agent — touches core/src/templates/. Requires handlebars-rust crate.

17. **T17: Build transition rules engine** — condition-action pairs, guard evaluation
    - source: Stage Pipeline spike (item 19), Core Architecture spike (item 2)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T04, T14
    - agent: rules engine agent — touches core/src/transitions/

18. **T18: Implement human-gated transitions** — approval tracking, multi-approver, timeout + escalation
    - source: Stage Pipeline spike (item 20), Core Architecture spike (item 5)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T17
    - agent: approval agent — touches server/src/approvals/

19. **T19: Implement agent_dispatch hook type** — spawn sub-agent as hook, async completion
    - source: Stage Pipeline spike (item 8)
    - confidence: LIKELY
    - priority: P1
    - depends-on: T15, T07
    - agent: dispatch hook agent — touches server/src/hooks/agent_dispatch.rs

20. **T20: Build time-based transition scheduler** — delayed jobs for timeouts and escalation
    - source: Stage Pipeline spike (item 22)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T17
    - agent: scheduler agent — touches server/src/scheduler.rs

21. **T21: Implement pipeline versioning** — snapshot-on-entry, tasks pinned to version
    - source: Stage Pipeline spike (item 4), Core Architecture spike (item 8)
    - confidence: LIKELY
    - priority: P2
    - depends-on: T14
    - agent: versioning agent — touches core/src/config/versioning.rs

22. **T22: Scaffold SolidJS frontend** — project setup, WebSocket client, TanStack Virtual
    - source: Tech Stack spike (item 5)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T06
    - agent: frontend scaffolding agent — creates ui/ directory, SolidJS + Vite + TanStack Virtual

23. **T23: Build Triage Queue view** — priority-sorted decision queue with quick-action buttons
    - source: UI spike (item 5)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T22, T18
    - agent: triage UI agent — touches ui/src/views/Triage/. Requires SolidJS, real-time list patterns.

24. **T24: Build Kanban board** — columns as stages, expandable cards, drag-and-drop transitions
    - source: UI spike (items 1, 2, 3, 4)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T22, T04
    - agent: kanban UI agent — touches ui/src/views/Board/. Requires DnD library, card state management.

25. **T25: Implement four-level interrupt classification** — P0-P3 event classification, notification routing
    - source: UI spike (item 10), Core Architecture spike (item 5)
    - confidence: CONFIRMED
    - priority: P0
    - depends-on: T05
    - agent: interrupt agent — touches server/src/attention/. Requires classification rules, notification dispatch.

26. **T26: Build Agent Detail Panel** — split-pane, output stream + Monaco diff viewer, session history
    - source: UI spike (item 8)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T22
    - agent: detail panel agent — touches ui/src/views/AgentDetail/. Requires Monaco integration.

27. **T27: Implement AI-summarized status** — fast model summarization with caching
    - source: UI spike (item 14)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T22, T12
    - agent: summarization agent — touches server/src/summaries/. Requires LLM API integration, cache strategy.

28. **T28: Build sidebar + main layout** — nav, agent list, persistent attention badges
    - source: UI spike (item 15)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T22
    - agent: layout agent — touches ui/src/layout/

29. **T29: Implement keyboard navigation** — j/k, Enter/Escape, g+key, Cmd+K palette
    - source: UI spike (item 16)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T22
    - agent: keyboard agent — touches ui/src/keyboard/

30. **T30: Build colorblind-safe status indicators** — color + shape, Okabe-Ito palette
    - source: UI spike (item 22)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T22
    - agent: design system agent — touches ui/src/design/

31. **T31: Define MoltIntegration plugin interface** — authenticate, read, write, subscribe, healthCheck
    - source: Stage Pipeline spike (item 13)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T02
    - agent: plugin interface agent — touches core/src/integrations/

32. **T32: Build encrypted credential store** — alias-based, pipeline-scoped, encrypted at rest
    - source: Stage Pipeline spike (item 14)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T01
    - agent: security agent — touches server/src/credentials/

33. **T33: Implement GitHub integration** — PRs, issues, checks, branches
    - source: Stage Pipeline spike (item 15)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T31
    - agent: GitHub agent — touches integrations/github/

34. **T34: Implement generic webhook integration** — configurable HTTP callbacks
    - source: Stage Pipeline spike (item 15)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T31
    - agent: webhook agent — touches integrations/webhook/

35. **T35: Implement Focus Mode** — suppress non-P0, queue-and-summarize on exit
    - source: UI spike (item 13)
    - confidence: CONFIRMED
    - priority: P1
    - depends-on: T25
    - agent: focus mode agent — touches ui/src/features/FocusMode/

36. **T36: Build decision batching** — detect batchable items, grouped review UI
    - source: UI spike (item 11)
    - confidence: LIKELY
    - priority: P2
    - depends-on: T23
    - agent: batching agent — touches ui/src/views/Triage/batching

37. **T37: Build Command Center dashboard** — agent grid, status lights, progress, resource usage
    - source: UI spike (item 6)
    - confidence: LIKELY
    - priority: P2
    - depends-on: T22, T12
    - agent: dashboard agent — touches ui/src/views/Dashboard/

38. **T38: Build Activity Feed** — chronological events, filterable
    - source: UI spike (item 7)
    - confidence: CONFIRMED
    - priority: P2
    - depends-on: T22, T03
    - agent: feed agent — touches ui/src/views/Activity/

39. **T39: Implement attention budget indicator** — pending counts, load projection
    - source: UI spike (item 12)
    - confidence: POSSIBLE
    - priority: P2
    - depends-on: T25
    - agent: budget agent — touches ui/src/features/AttentionBudget/

40. **T40: Implement customizable panel layouts** — drag panels, save presets
    - source: UI spike (item 18)
    - confidence: LIKELY
    - priority: P2
    - depends-on: T28
    - agent: layout agent — touches ui/src/layout/panels

41. **T41: Add ARIA live regions for accessibility** — screen reader support for status updates
    - source: UI spike (item 23)
    - confidence: CONFIRMED
    - priority: P2
    - depends-on: T22, T25
    - agent: a11y agent — touches ui/src/design/a11y

42. **T42: Implement pipeline templates** — variable substitution in YAML
    - source: Stage Pipeline spike (item 16)
    - confidence: CONFIRMED
    - priority: P2
    - depends-on: T14
    - agent: template agent — touches core/src/config/templates

43. **T43: Implement pipeline inheritance** — extends + deep merge
    - source: Stage Pipeline spike (item 17)
    - confidence: LIKELY
    - priority: P3
    - depends-on: T42
    - agent: inheritance agent — touches core/src/config/inheritance

44. **T44: Add on_stall lifecycle event** — stuck task detection + escalation
    - source: Stage Pipeline spike (item 9)
    - confidence: LIKELY
    - priority: P2
    - depends-on: T15, T12
    - agent: stall detector agent — touches server/src/hooks/stall

45. **T45: Add DuckDB analytics layer** — event log analytics, Parquet export
    - source: Tech Stack spike (item 9)
    - confidence: LIKELY
    - priority: P3
    - depends-on: T03
    - agent: analytics agent — touches server/src/analytics/

46. **T46: Build CLI + browser delivery** — `molt-hub serve`, opens localhost
    - source: Tech Stack spike (item 15)
    - confidence: LIKELY
    - priority: P1
    - depends-on: T06, T22
    - agent: CLI agent — touches server/src/main.rs, build scripts

47. **T47: Wrap in Tauri 2.0 shell** — native window, system tray, IPC + WebSocket
    - source: Tech Stack spike (items 12, 16)
    - confidence: CONFIRMED
    - priority: P2
    - depends-on: T46
    - agent: Tauri agent — creates tauri/ directory, Tauri config

48. **T48: Add Docker sandboxing option** — container isolation for agent processes
    - source: Agent Harness spike (item 10)
    - confidence: LIKELY
    - priority: P2
    - depends-on: T10
    - agent: sandbox agent — touches harness/src/sandbox/

## Priority Order

1. T01 (scaffold) → T02 (data model) → T03 (event store) + T04 (state machine) + T06 (WebSocket) + T07 (adapter trait) + T14 (stage config)
2. T05 (actors) + T08 (Claude adapter) + T09 (CLI adapter) + T10 (supervisor) + T11 (worktrees) + T15 (hooks) + T16 (templates) + T17 (transitions)
3. T18 (human gates) + T22 (SolidJS scaffold) + T25 (interrupts) + T46 (CLI delivery)
4. T23 (triage) + T24 (kanban) + T26 (agent detail) + T27 (summaries) + T28 (layout) + T29 (keyboard) + T30 (status indicators)
5. T12, T13, T19, T20, T31-T35 (integrations + polish)
6. T36-T48 (advanced features + distribution)

## Task IDs

| Task ID | Title | Priority | Status | Assigned Agent |
|---------|-------|----------|--------|----------------|
| T01 | Scaffold Rust workspace monorepo | P0 | open | scaffolding |
| T02 | Define core data model | P0 | open | domain modeler |
| T03 | Implement event store (SQLite WAL) | P0 | open | persistence |
| T04 | Build state machine engine | P0 | open | state machine |
| T05 | Implement actor-per-task concurrency | P0 | open | concurrency |
| T06 | Build WebSocket multiplexing server | P0 | open | transport |
| T07 | Define AgentAdapter trait | P0 | open | interface designer |
| T08 | Claude Code SDK adapter | P0 | open | SDK integration |
| T09 | Generic CLI adapter | P0 | open | CLI adapter |
| T10 | Build process supervisor | P0 | open | supervisor |
| T11 | Git worktree lifecycle manager | P0 | open | git |
| T12 | Activity-based health monitoring | P1 | open | monitoring |
| T13 | Audit logging at adapter boundary | P1 | open | logging |
| T14 | Stage configuration schema (YAML) | P0 | open | config |
| T15 | Hook executor engine | P0 | open | hook engine |
| T16 | Instruction templating (Handlebars) | P0 | open | templates |
| T17 | Transition rules engine | P0 | open | rules engine |
| T18 | Human-gated transitions | P0 | open | approvals |
| T19 | agent_dispatch hook type | P1 | open | dispatch hook |
| T20 | Time-based transition scheduler | P1 | open | scheduler |
| T21 | Pipeline versioning | P2 | open | versioning |
| T22 | Scaffold SolidJS frontend | P0 | open | frontend scaffolding |
| T23 | Triage Queue view | P0 | open | triage UI |
| T24 | Kanban board | P0 | open | kanban UI |
| T25 | Four-level interrupt classification | P0 | open | interrupt |
| T26 | Agent Detail Panel | P1 | open | detail panel |
| T27 | AI-summarized status | P1 | open | summarization |
| T28 | Sidebar + main layout | P1 | open | layout |
| T29 | Keyboard navigation | P1 | open | keyboard |
| T30 | Colorblind-safe status indicators | P1 | open | design system |
| T31 | MoltIntegration plugin interface | P1 | open | plugin interface |
| T32 | Encrypted credential store | P1 | open | security |
| T33 | GitHub integration | P1 | open | GitHub |
| T34 | Generic webhook integration | P1 | open | webhook |
| T35 | Focus Mode | P1 | open | focus mode |
| T36 | Decision batching | P2 | open | batching |
| T37 | Command Center dashboard | P2 | open | dashboard |
| T38 | Activity Feed | P2 | open | feed |
| T39 | Attention budget indicator | P2 | open | budget |
| T40 | Customizable panel layouts | P2 | open | layout |
| T41 | ARIA accessibility | P2 | open | a11y |
| T42 | Pipeline templates | P2 | open | templates |
| T43 | Pipeline inheritance | P3 | open | inheritance |
| T44 | on_stall lifecycle event | P2 | open | stall detector |
| T45 | DuckDB analytics layer | P3 | open | analytics |
| T46 | CLI + browser delivery | P1 | open | CLI |
| T47 | Tauri 2.0 shell | P2 | open | Tauri |
| T48 | Docker sandboxing | P2 | open | sandbox |

## Critical Path

T01 → T02 → T04 → T17 → T18 → T23 (Triage Queue = first usable human interaction surface)

Parallel: T01 → T06 → T22 → T24 (Kanban board)
Parallel: T02 → T07 → T08 + T10 → T11 (agent execution)

**Minimum time to first usable demo:** T01 + T02 + T04 + T06 + T07 + T08 + T10 + T14 + T17 + T18 + T22 + T23 + T24 + T25 + T46

## Parallel Opportunities

**Wave 1** (no deps on each other): T03, T04, T06, T07, T14 — all depend only on T01/T02
**Wave 2** (no deps on each other): T05, T08, T09, T10, T11, T15, T16, T17 — all depend on Wave 1 items
**Wave 3** (no deps on each other): T18, T22, T25, T32, T46 — all depend on Wave 2 items
**Wave 4** (no deps on each other): T23, T24, T26, T27, T28, T29, T30, T31, T33, T34, T35 — all depend on Wave 3 items
