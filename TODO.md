# Epic: Molt Hub — Agent Mission Control

**Goal**: Mission control for managing many simultaneous AI coding agents. Performant, customizable per-stage pipeline with instructions/skills/integrations/hooks. Human-in-the-loop without context-switch overload. Agent-harness agnostic. Mobile-accessible for approvals and status.

**Stack**: Rust (Axum + Tokio) backend, SolidJS frontend, SQLite (WAL) database, WebSocket transport, Tauri 2.0 shell (Phase 2)

---

## Phase 1: Foundation

- [ ] T01: Scaffold Rust workspace monorepo (crates: core, server, harness, ui-api) <!-- P0, no deps -->
- [ ] T02: Define core data model — Project, Pipeline, Stage, Task, Agent, Team, Session, Event entities as Rust types. Project is top-level entity above Pipeline for multi-repo support. <!-- P0, depends: T01 -->
- [ ] T03: Implement event store on SQLite WAL — append-only event log with batched writes, correlation/causation IDs <!-- P0, depends: T02 -->
- [ ] T04: Build state machine engine — task progression through stages using Rust enums + guards/actions <!-- P0, depends: T02 -->
- [ ] T05: Implement actor-per-task concurrency — each active task gets a tokio task owning its state machine <!-- P0, depends: T03, T04 -->
- [ ] T06: Build WebSocket multiplexing server — single connection, subscribe/unsubscribe per agent stream <!-- P0, depends: T01 -->

## Phase 2: Agent Core

- [ ] T07: Define AgentAdapter trait — spawn, send, on_message, status, terminate, abort <!-- P0, depends: T02 -->
- [ ] T08: Implement Claude Code SDK adapter via CLI integration — structured streaming, session persistence, cost tracking <!-- P0, depends: T07 -->
- [ ] T09: Implement generic CLI adapter — stdin/stdout/stderr capture, heuristic progress detection, output parser plugins <!-- P0, depends: T07 -->
- [ ] T10: Build process supervisor — tokio::process + pty-process, graceful shutdown, orphan detection, restart policies <!-- P0, depends: T07 -->
- [ ] T11: Implement git worktree lifecycle manager — create on task assign, agent works in worktree, PR/merge on completion <!-- P0, depends: T07 -->
- [ ] T11b: Build dev environment hook (container-as-hook model) — Container lifecycle is a hook, not a separate layer. New hook kinds: `StartDevEnvironment` (on_enter for stages needing dev servers) and `TeardownDevEnvironment` (on_enter for terminal stages). Hook creates worktree, starts container(s), polls health checks, registers reverse proxy route, emits `DevEnvironmentReady` event. Two auto-detected paths: (1) No compose file → bollard single container, worktree as volume. (2) Has compose file → shell out to `docker compose --project-name molt-{slug} up`. Agent (acpx) runs on host, writes code to worktree. Container runs dev server, mounts same worktree as volume. Filesystem is the integration point — HMR picks up changes. Stage config declares preview services (`services: [{name: app, port: 3000}]`). <!-- P0, depends: T11, T15 -->
- [ ] T11c: Implement cost management — pipeline-level and per-stage budget caps, real-time spend tracking (from Claude SDK `total_cost_usd` + usage), auto-pause at configurable threshold (80% warn → P1 triage item, 100% → pause agents → P0 triage item), cost attribution per task/agent/stage/day in event store <!-- P0, depends: T08, T03 -->
- [ ] T11d: Implement API rate limiting — shared token bucket across all agents for LLM API concurrency, priority-based queue (P0 agents get slots first), backpressure surfaced to UI ("3 agents waiting for API capacity") <!-- P0, depends: T07, T25 -->
- [ ] T11e: Build crash recovery system — WIP checkpoint commits at stage boundaries (configurable timer), worktree health assessment on agent failure (clean/dirty/broken build), recovery options surfaced in triage queue (resume with new agent / reset to checkpoint / assign to human / abandon), container restart semantics when container dies but volume survives <!-- P0, depends: T10, T11, T23 -->
- [ ] T12: Implement activity-based health monitoring — output timestamps, file change detection, stuck detection <!-- P1, depends: T10 -->
- [ ] T13: Build audit logging at adapter boundary — every spawn/send/terminate logged with agent+task IDs <!-- P1, depends: T07 -->
- [ ] T13b: Implement fd-based credential injection — credentials flow from keychain (via keyring crate) through anonymous pipe/memfd to agent process. Write end closed after injection, fd set close-on-exec so child processes don't inherit. Secrets redacted from event store and audit logs. Pipeline-scoped: credentials have aliases, pipelines declare which aliases they need, injection path only retrieves declared subset. <!-- P1, depends: T32, T07 -->
- [ ] T13c: Build isolated audit log writer — separate long-lived tokio task writing to dedicated SQLite DB via bounded channel. Agent processes never hold a handle to audit DB. Harness sends audit events through channel, writer task persists them. <!-- P1, depends: T03, T07 -->

## Phase 3: Pipeline Engine

- [ ] T14: Define stage configuration schema — YAML declaration with JSON Schema validation, hot-reload support <!-- P0, depends: T02 -->
- [ ] T15: Build hook executor engine — ordered handler list, sequential/parallel execution, failure policies, rich context object <!-- P0, depends: T14, T05 -->
- [ ] T16: Implement instruction templating — Handlebars-style with restricted variable set, base instruction inheritance <!-- P0, depends: T14 -->
- [ ] T17: Build transition rules engine — condition-action pairs, guard evaluation against task state <!-- P0, depends: T04, T14 -->
- [ ] T18: Implement human-gated transitions — approval tracking, multi-approver, timeout + escalation <!-- P0, depends: T17 -->
- [ ] T19: Implement agent_dispatch hook type — spawn sub-agent as hook, async completion tracking <!-- P1, depends: T15, T07 -->
- [ ] T20: Build time-based transition scheduler — delayed jobs for timeouts and auto-escalation <!-- P1, depends: T17 -->
- [ ] T21: Implement pipeline versioning — snapshot-on-entry, tasks pinned to creation-time version <!-- P2, depends: T14 -->

## Phase 3b: Context Management

- [ ] T54: Build context assembly pipeline — per-stage context builder that composes the agent's initial prompt from layers: epic context (distilled), task description, previous stage output (summarized), stage instructions (from T16 templates), project knowledge (CLAUDE.md, rules files, Beads), and custom context sources. Each layer has a token budget. Assembly is deterministic and inspectable (UI shows exactly what context was sent). <!-- P0, depends: T16, T03 -->
- [ ] T55: Implement stage transition context distillation — when a task moves between stages, the previous stage's output is automatically summarized/distilled before being injected into the next stage's context. Configurable per-transition: `context_carry: full | summarized | key_decisions_only | none`. Distiller engine is pluggable (default: fast LLM call, alternatives: local model, custom script, Beads compression). Prevents context bloat across long pipelines. <!-- P0, depends: T54, T17 -->
- [ ] T56: Build external context source registry — pluggable sources that feed into the context assembly pipeline. Built-in sources: file glob (load specific files from worktree), Beads (compressed context bundles), CLAUDE.md / project rules, git diff (changes since branch point), epic/parent task context, URL fetch (docs, wikis). Each source declares its token cost. Stage config references sources by name: `context_sources: [project_rules, epic_summary, beads:architecture]`. <!-- P0, depends: T54 -->
- [ ] T57: Implement context budget manager — each stage has a configurable max context token budget (e.g., 80K of 200K window). The assembly pipeline allocates tokens across layers by priority: instructions (highest) → task description → previous stage output → project knowledge → epic context → supplementary sources. If total exceeds budget, lower-priority layers are truncated or summarized. Budget and actual usage shown in UI per agent. <!-- P1, depends: T54 -->
- [ ] T58: Build context health monitoring — detect when an agent's effective context is degrading (session too long, too many tool calls, context window filling up). Configurable thresholds trigger: auto-summarize and restart session (preserving work), surface P1 triage item ("agent X context degraded, recommend restart"), or auto-checkpoint and spawn fresh agent on same task. Integrates with Claude SDK session management (resume/checkpoint). <!-- P1, depends: T54, T12, T08 -->
- [ ] T59: Implement epic context management — epics carry structured context (goals, constraints, architecture decisions, domain terminology) that flows down to all child tasks. Epic context is distilled to fit within a token budget and injected into every task's context assembly. Epic owners can edit the distilled context directly. Changes propagate to in-flight tasks on next agent restart. <!-- P1, depends: T54, T02 -->
- [ ] T60: Build context inspector UI — panel in Agent Detail view showing exactly what context was assembled for the current session: each layer with its source, token count, and content preview. Expandable to see full content per layer. Diff view between what was sent at session start vs what the agent has now. "Re-inject context" button to restart agent with updated context without losing worktree state. <!-- P1, depends: T54, T26 -->
- [ ] T61: Implement Beads adapter — read/write Beads context bundles as one context source plugin. Project-level Beads auto-included for all tasks. Stage-level Beads per-stage config. Support creating Beads from agent output (compress plan into Bead for next stage). This is one adapter in the registry, not a hard dependency — context source and distiller interfaces are format-agnostic. <!-- P2, depends: T56 -->

## Phase 4: UI Core

- [ ] T22: Scaffold SolidJS frontend with TanStack Virtual — project setup, WebSocket client, virtualized list primitives <!-- P0, depends: T06 -->
- [ ] T23: Build Triage Queue view — priority-sorted decision queue with quick-action buttons <!-- P0, depends: T22, T18 -->
- [ ] T24: Build Kanban board with expandable agent cards — columns as stages, drag-and-drop transitions, 3-state cards (collapsed/expanded/focused) <!-- P0, depends: T22, T04 -->
- [ ] T25: Implement four-level interrupt classification — P0-P3 event classification, notification routing <!-- P0, depends: T05 -->
- [ ] T26: Build Agent Detail Panel — split-pane with output stream + diff viewer (Monaco), session history, embedded preview iframe (hot-switches between worktrees), "Open in IDE" button via URI schemes (vscode://, cursor://, zed://) <!-- P1, depends: T22, T11b -->
- [ ] T27: Implement AI-summarized status — fast model summarization pipeline with caching <!-- P1, depends: T22, T12 -->
- [ ] T28: Build sidebar + main layout with persistent attention badges — nav, agent list, P0/P1 count <!-- P1, depends: T22 -->
- [ ] T29: Implement keyboard navigation — j/k nav, Enter/Escape, g+key view switching, Cmd+K palette <!-- P1, depends: T22 -->
- [ ] T30: Build colorblind-safe status indicator system — color + shape encoding, Okabe-Ito palette <!-- P1, depends: T22 -->

## Phase 5: Integrations

- [ ] T31: Define MoltIntegration plugin interface — authenticate, read, write, subscribe, healthCheck <!-- P1, depends: T02 -->
- [ ] T32: Build credential store on system keychain — keyring crate wrapping macOS Keychain / Linux secret-service. Credentials keyed by `molt:{alias}`. Pipeline-scoped access: pipelines declare required credential aliases, injection path only retrieves declared subset. No custom encryption — keychain handles encryption at rest. <!-- P1, depends: T01 -->
- [ ] T33: Implement GitHub integration — PRs, issues, checks, branch operations <!-- P1, depends: T31 -->
- [ ] T34: Implement generic webhook integration — configurable HTTP callbacks for Slack/CI/custom <!-- P1, depends: T31 -->

## Phase 6: Polish & Advanced

- [ ] T35: Implement Focus Mode — suppress non-P0 interrupts, queue-and-summarize on exit <!-- P1, depends: T25 -->
- [ ] T36: Build decision batching — detect batchable items, grouped review UI <!-- P2, depends: T23 -->
- [ ] T37: Build Command Center dashboard — agent status grid, progress bars, resource usage, real-time cost burn rate <!-- P2, depends: T22, T12, T11c -->
- [ ] T38: Build Activity Feed — chronological event stream, filterable by agent/type/severity <!-- P2, depends: T22, T03 -->
- [ ] T39: Implement attention budget indicator — pending P0/P1 count, cognitive load projection <!-- P2, depends: T25 -->
- [ ] T40: Implement customizable panel layouts with saved presets <!-- P2, depends: T28 -->
- [ ] T41: Add ARIA live regions for screen reader support <!-- P2, depends: T22, T25 -->
- [ ] T42: Implement pipeline templates with variable substitution <!-- P2, depends: T14 -->
- [ ] T43: Implement pipeline inheritance via extends + deep merge <!-- P3, depends: T42 -->
- [ ] T44: Add on_stall lifecycle event for stuck task detection <!-- P2, depends: T15, T12 -->
- [ ] T45: Add DuckDB analytics layer for event log querying <!-- P3, depends: T03 -->

## Phase 7: Distribution

- [ ] T46: Build web delivery — `molt-hub serve` starts backend + opens UI in default browser. No CLI UX beyond starting the server. <!-- P1, depends: T06, T22 -->
- [ ] T46b: Build managed Caddy reverse proxy — Molt Hub bundles/downloads Caddy binary, starts as child process with admin API enabled. StartDevEnvironment hook registers routes via Caddy admin API (`POST /config/apps/http/servers/.../routes`). TeardownDevEnvironment deregisters. Maps `{service}.{worktree-slug}.molt.localhost` → container port. No user-facing Caddy config. <!-- P1, depends: T11b -->
- [ ] T47: Wrap in Tauri 2.0 shell — native window, system tray, IPC for commands + WebSocket for streams <!-- P2, depends: T46 -->

## Phase 8: Onboarding & Testing

- [ ] T49: Build first-run setup wizard — detect Docker/OrbStack, connect to repo, create first pipeline from template, verify agent harness connectivity <!-- P1, depends: T46, T14, T11b -->
- [ ] T50: Build mock agent adapter — fake agent that simulates work (configurable delays, canned file writes, mock output streams) for integration testing and demo mode <!-- P1, depends: T07 -->
- [ ] T51: Implement event replay for UI testing — replay real event logs through the UI to test rendering without live agents <!-- P2, depends: T03, T22 -->
- [ ] T52: Implement log retention and rotation — configurable max event store size, compress/archive old events (export to Parquet), keep hot DB small <!-- P1, depends: T03 -->
- [ ] T53: Build SQLite schema migration system — versioned migrations for event store schema changes on Molt Hub upgrade, backward-compatible reads <!-- P1, depends: T03 -->

## Future (Post-MVP)

- [ ] F01: Mobile companion — responsive web UI for status/approvals from phone
- [ ] F02: Merge conflict resolution strategy for multi-worktree agents
- [ ] F03: Agent-to-agent communication protocol
- [ ] F04: Tree/Graph view for task dependencies
- [ ] F05: Issue tracker integration (Linear/Jira)
- [ ] F06: Parallel fork-join paths for multi-reviewer scenarios
- [ ] F07: Expression language for transition conditions (safe evaluator)
- [ ] F08: Integration plugin sandboxing
- [ ] F09: Cross-project task dependencies — "backend API change blocks frontend integration"
- [ ] F10: Inter-container networking — shared Docker network for integration testing between worktree branches
- [ ] F11: Chaos mode — randomly fail agents, crash containers, inject API errors to validate recovery paths
- [ ] F12: Demo mode — pre-loaded sample project with mock agents for exploring UI without real agent setup
