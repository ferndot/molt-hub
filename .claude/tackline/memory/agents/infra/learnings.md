# Learnings: infra

## Codebase Patterns
- Distribution: `molt-hub serve` (Phase 1), Tauri 2.0 shell (Phase 2)
- Container-as-hook: agent on host, container runs dev server, filesystem integration
- Credentials: system keychain via `keyring`, pipeline-scoped aliases, fd injection
- workspace Cargo.toml accepts `clap`, `tower-http`, `open` as workspace deps; crates reference with `{ workspace = true }` (added: 2026-03-23, dispatch: T46)
- SPA routing: Router::fallback_service(ServeDir::new(dist).fallback(ServeFile::new(index))) — non-API paths return index.html (added: 2026-03-23, dispatch: T46)

## Gotchas
- RESOLVED: Rust toolchain now installed (updated: 2026-03-23)
- workspace.package fields inherited with `.workspace = true` (added: 2026-03-23, dispatch: T01)
- DashMap: `drop(entry)` before `remove()` to avoid shard deadlock (added: 2026-03-23, dispatch: T10)
- Worktree paths: make absolute relative to `repo_root` when `base_dir` is relative (added: 2026-03-23, dispatch: T11)
- Run `cargo check -p <target>` before writing — pre-existing dep errors are invisible otherwise (added: 2026-03-23, dispatch: T10)
- `open::that()` takes &str, not Path — build URL string before calling (added: 2026-03-23, dispatch: T46)

## Preferences
- `tempfile::TempDir` + `git init` for worktree integration tests — reliable, self-cleaning (added: 2026-03-23, dispatch: T11)
- `AtomicUsize` counters in MockAdapter: lightweight, no process needed (added: 2026-03-23, dispatch: T10)

## Cross-Agent Notes
- RESOLVED: Worktree isolation via Agent tool now works. (updated: 2026-03-23)
