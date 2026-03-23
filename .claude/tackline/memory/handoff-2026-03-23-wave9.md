## Session Handoff: 2026-03-23 — Waves 7-9 (Jira + OAuth + UX)

### What Got Done
- **Jira integration end-to-end**: Data model (T31), API client, import service, 4 REST endpoints, OAuth 2.0 with PKCE (no client secret), JiraImport dialog, Settings view
- **OAuth PKCE**: Converted from client-secret flow to PKCE for distributable binary. Client ID baked at build time, no secret needed. sha2 + rand + base64 crates added.
- **UX overhaul**: Removed top header bar, added bottom status bar (connection + agent metrics + CPU/MEM), sidebar collapse chevrons (« / »), resizable sidebars (drag handles), "Triage" → "Inbox", "Attention Only" → "Focus", agent status labels simplified
- **Column config on Board**: Moved from Settings to BoardView with ColumnEditor component. Columns now support behavior (WIP limits, auto-assign, require approval) and hooks (on_enter, on_exit, on_stall)
- **Infrastructure**: Credential store (T32, keyring-backed + FD injection), agent_dispatch hook (T19), audit logging (T13), settings localStorage persistence
- **Status bar metrics**: Active agents, pending decisions, CPU/MEM (mocked), metricsStore for future WebSocket wiring
- **Settings backend sync**: Frontend saves to both localStorage and PUT /api/settings (debounced 500ms)
- **534+ tests** (365 Rust + 169 frontend), all passing

### Key Decisions
- **OAuth PKCE over client secret**: App is distributable — users download a binary. Can't embed secrets. PKCE eliminates the secret entirely. Client ID: `3yQWy34WyjCn0wtOfawofBTMmtK3gUgs` (baked in at build time in oauth.rs)
- **Column config on Board, not Settings**: User feedback — column config is board workflow, not app preferences. Each column has behavior + hooks config.
- **Settings persistence**: localStorage as offline cache, SQLite backend as source of truth. Debounced saves. Server as dumb key-value store (JSON blob under "app_settings" key).
- **Status bar over header**: Header wasted space. Status bar shows operational metrics like a real mission control. Will wire to health monitoring WebSocket.
- **Naming**: Triage→Inbox, Attention Only→Focus, Implementing→Working, Awaiting Review→Needs Review, Needs Decision→Blocked

### Patterns & Discoveries
- After merging new DomainEvent variants to core, run `cargo check` on main before dispatching downstream agents — classifier.rs and router.rs have exhaustive matches that break
- PipelineConfig test literals in 5 server files need `columns: vec![]` and `integrations: vec![]` when fields are added. Consider `..Default::default()` pattern.
- base64 crate v0.22 uses engine API: `URL_SAFE_NO_PAD.encode(...)` not the old free function
- keyring v3 has no prefix-scan — can't enumerate credentials by prefix
- Frontend agents sometimes skip worktree isolation and commit directly to main

### In-Progress Work
- **Backend settings SQLite store**: Agent dispatched (worktree agent-a41c6c6f), no commits yet. Building `crates/server/src/settings/` with SettingsStore (SQLite-backed key-value) + GET/PUT/DELETE /api/settings endpoints.

### Uncommitted Changes
- Memory files only (learnings, retro-history, sprint checkpoint, feedback memos). Not source code.
- `.claude/launch.json` — preview server config (untracked)

### Blocked Work
None.

### Resumable Agents
- **backend settings agent** (a41c6c6f): Building SQLite settings store. Worktree at `.claude/worktrees/agent-a41c6c6f`. May complete after session ends — check worktree for commits.

### Open Questions
- **Settings API route prefix**: `/api/settings` confirmed by user intent, but oauth_handlers uses `/api/integrations/jira/oauth/` — confirm consistency of API prefix structure
- **Status bar real metrics**: CPU/MEM are mocked. Need to pipe health monitoring data (T12) through WebSocket to the frontend metricsStore. Design question: poll interval vs push on change?
- **OAuth redirect route**: `handleOAuthCallback()` in settingsStore needs a SolidJS route (e.g., `/oauth/jira/callback`) to handle the redirect from Atlassian. Not yet wired.

### Recommended Next Steps
1. **Merge backend settings agent** — check `.claude/worktrees/agent-a41c6c6f` for commits, merge if complete
2. **Wire OAuth redirect route** — add `/oauth/jira/callback` route in App.tsx that calls `handleOAuthCallback(code, state)` from settingsStore
3. **Wire stores to real WebSocket** — triage, board, agent detail, and metrics stores all have stubs. This is the biggest remaining gap to a working demo.
4. **GitHub integration (T33)** — next P1 integration after Jira
5. **Tauri shell (T47)** — user wants a distributable binary. This wraps the web UI as a native macOS app.

### Risks & Warnings
- Session dispatched 13 agents total — context is overloaded. Start fresh for next work.
- Backend settings agent may still be running in worktree. Check before starting new backend work.
- The Rust server on port 3001 may still be running from this session — kill it before rebuilding.
- 10 worktree directories in `.claude/worktrees/` — clean up stale ones with `git worktree prune`.
