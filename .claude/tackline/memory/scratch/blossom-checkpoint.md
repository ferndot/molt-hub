# Blossom Checkpoint — Molt Hub

**Phase**: 2 (Executing Spikes)
**Date**: 2026-03-22

## Completed Spikes

### 1. Core Architecture & State Model — COMPLETE
- 10 items (4 CONFIRMED, 5 LIKELY, 1 CONFIRMED)
- Key decisions: Event sourcing + CQRS, XState v5 statecharts, actor-per-task concurrency, 6 core entities (Pipeline, Stage, Task, Agent, Team, Session), three-tier human attention model, TypeScript monorepo + SQLite
- 5 deeper spikes identified (conflict resolution, pipeline UX, output streaming, persistence validation, recovery semantics)

### 2. Agent Harness Abstraction — PENDING

### 3. Stage Pipeline & Extensibility — COMPLETE
- 23 items (12 CONFIRMED, 6 LIKELY, 0 POSSIBLE)
- Key decisions: YAML declaration + code behavior (layered config), hook system with ordered handlers + rich context, Handlebars templating for instructions, MoltIntegration plugin interface, condition-action transition rules, human-gated transitions as first-class concept
- 4 deeper spikes identified (agent dispatch hook lifecycle, parallel paths, integration sandboxing, expression language)

### 4. UI Paradigm & Attention Management — COMPLETE
- 24 items (17 CONFIRMED, 4 LIKELY, 1 POSSIBLE)
- Key insight: Triage Queue is the primary active surface, Kanban board is organizational
- Key decisions: Multi-view system (Kanban + Triage + Dashboard + Agent Detail + Activity), four-level interrupt system (P0-P3), decision batching, AI-summarized status, Focus Mode, keyboard-driven navigation, desktop-first
- 5 deeper spikes identified (batching algorithm, summarization pipeline, interrupt classification, WebSocket protocol, client state management)

### 5. Tech Stack & Performance — PENDING

## Emerging Consensus Across Spikes

- TypeScript monorepo (Core Architecture)
- Event sourcing + CQRS (Core Architecture)
- XState v5 for state machines (Core Architecture + Stage Pipeline)
- SQLite for local-first (Core Architecture)
- WebSocket multiplexing (UI)
- YAML + code layered config (Stage Pipeline)
- Triage queue as primary interaction surface (UI)
- Four-level interrupt priority (Core Architecture + UI)

## User Context
- Product engineer managing many concurrent agents
- Primary goal: reduce context-switch burnout
- Secondary: quality + meaningful human involvement
- Future: mobile companion app (status, approvals, steering from phone)
