# Learnings: backend

## Codebase Patterns
- WebSocket: multiplexed single connection, 3 message types (subscribe/unsubscribe/event)
- Actor model: mpsc for commands in, watch for state broadcast — no actor framework
- Hook executor: ordered handlers, seq/parallel execution, failure policies (abort/continue/retry)
- Server crate uses native RPITIT for async traits, not async_trait crate (added: 2026-03-23, dispatch: T18)
- ApprovalService integrates with actors via TaskActorHandle.send_event — HumanDecision is just a domain event (added: 2026-03-23, dispatch: T18)

## Gotchas
- DashMap v6 for ConnectionManager and TaskRegistry (added: 2026-03-23, dispatch: T06)
- AgentHandle uses `Box<dyn Any + Send + Sync>` — requires downcast_ref (added: 2026-03-23, dispatch: T07)
- `ulid` needs explicit `features = ["serde"]` at workspace level (added: 2026-03-23, dispatch: T05)
- `serde_json::Error::custom` needs `use serde::de::Error as _` — prefer purpose-built error variants (added: 2026-03-23, dispatch: T08)
- `ChildStdin` in `Arc<Mutex>` can't cleanly close for EOF — use `Option<Mutex<ChildStdin>>` for take/drop (added: 2026-03-23, dispatch: T08)
- async_trait is not in server/Cargo.toml — always use native RPITIT in server crate (added: 2026-03-23, dispatch: T18)

## Preferences
- mpsc + watch = clean actor pattern without framework dep (added: 2026-03-23, dispatch: T05)
- `MemoryStore` with `std::sync::Mutex` (not tokio) fine for simple async trait tests (added: 2026-03-23, dispatch: T05)
- `OutputMode` enum cleanly separates parsing (Raw/JsonLines/Delimiter) (added: 2026-03-23, dispatch: T09)
- `execution_mode` on first hook's config is awkward — stage-level field would be cleaner (added: 2026-03-23, dispatch: T15)

## Cross-Agent Notes
- RESOLVED: Rust toolchain now installed. (updated: 2026-03-23)
