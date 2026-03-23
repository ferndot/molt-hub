## Session Handoff: 2026-03-23 — Wave 2 Complete

### What Got Done
- Fixed 2 pre-existing compile errors (ulid serde feature, store.rs import) on main
- Installed Rust toolchain (`rustup`)
- Dispatched and completed Wave 2 sprint: 8/8 tasks via parallel worktree agents
- Merged all 8 implementations into main: 130 tests passing across 3 crates
- Updated all agent learnings (architect, backend, infra)
- Ran /retro — captured patterns and updated memory
- Cleaned up all 8 worktree branches

### Key Decisions
- **Merge order**: core → harness → server (follows Cargo dependency graph). Rejected: alphabetical or completion order.
- **T09 model change**: Added `AgentStatus::Completed` and `AgentStatus::Failed` variants to core model. These are distinct from `Terminated` (explicit shutdown) and `Crashed` (unexpected exit). Decided during T09 implementation because CLI adapter needs to distinguish exit code 0 vs non-zero.
- **Handlebars 6**: Added as workspace dependency for T16 instruction templating. Strict mode enabled by default.

### Patterns & Discoveries
- All 8 agents independently fixed the same 2 pre-existing bugs — fix compile errors BEFORE dispatching
- Worktree isolation works perfectly now that git repo exists — 7 agents ran in parallel with zero conflicts
- DashMap shard deadlock: `drop(entry)` before `remove()` is critical
- mpsc + watch = clean actor pattern without requiring an actor framework crate

### In-Progress Work
None — all Wave 2 tasks completed and merged.

### Uncommitted Changes
- `.claude/` directory (team config, tackline memory, learnings) — intentionally untracked
- `TODO.md` — untracked scratch file

### Blocked Work
None.

### Open Questions
- **`crates/core/src/events/store.rs`**: The `serde_json::Error::custom` was replaced differently by different agents (some used `serde::de::Error as _`, one used `EventStoreError::Parse`, one used `Error::io`). The current main has `serde::de::Error as _` which is correct, but future agents may encounter similar issues. Consider adding a workspace-level lint or CLAUDE.md note.
- **`crates/server/src/actors.rs` MemoryStore**: Uses `std::sync::Mutex` (not tokio). This is fine for tests but won't work if the test store is ever used in production code paths. Not urgent — just awareness.

### Recommended Next Steps
1. Run `~/.cargo/bin/cargo check` to validate compilation
2. Run `/sprint` for Wave 3: T18 (human-gated transitions), T22 (SolidJS scaffold), T25 (interrupt classification), T46 (CLI delivery)
3. Frontend agent activates for first time on T22 — will need SolidJS + Vite scaffolding
4. T32 (credential store) is also Wave 3-adjacent (P1, depends only on T01)

### Risks & Warnings
- `.gitignore` has `.claude/worktrees/` but not `.claude/` itself — tackline memory is untracked. Consider whether it should be committed.
- The large a24ad7e commit bundles T10+T11+T08+T09+T05+T15 — harder to revert individual tasks if issues are found. Not blocking but worth noting for future merge discipline.
- `workspace.package.name` warning in Cargo.toml — harmless but noisy. Remove the `name` key from `[workspace.package]` to silence it.
