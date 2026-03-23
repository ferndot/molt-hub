# Learnings: architect

## Codebase Patterns
- Project uses Rust workspace monorepo with crates: core, server, harness
- Event sourcing + CQRS: events table is write side, projection tables are read side
- State machines use Rust enums with `transition(self, event) -> Result<TaskState, TransitionError>`
- Stages are user-defined strings in project config, not Rust enum variants

## Gotchas
- `std::time::Duration` needs custom serde module (serialize as u64 seconds) (added: 2026-03-23, dispatch: T02)
- EventEnvelope wrapping keeps DomainEvent variants free of boilerplate (added: 2026-03-23, dispatch: T02)
- Handlebars 6 strict mode: missing variables → RenderError. Detection needs heuristic string match. (added: 2026-03-23, dispatch: T16)
- `TransitionTrigger::Manual`/`Timeout` have no DomainEvent mappings yet — update when T18/T20 land. (added: 2026-03-23, dispatch: T17)

## Preferences
- events module as directory (mod.rs, types.rs, store.rs, schema.rs) works well (added: 2026-03-23, dispatch: T03)
- `serde_yaml = "0.9"` correct pinned version (added: 2026-03-23, dispatch: T14)
- Native RPIT in traits eliminates async-trait dep (added: 2026-03-23, dispatch: T03)
- Guard evaluator: recursive function over serde_json::Value with all/any combinators (added: 2026-03-23, dispatch: T17)
- `HashMap<String,String>` custom fields → `{{custom.key}}` in Handlebars (added: 2026-03-23, dispatch: T16)

## Cross-Agent Notes
- RESOLVED: Rust toolchain now installed. (updated: 2026-03-23)
- MoltIntegration trait uses synchronous methods — server crate wraps in async (spawn_blocking). Implementors must be Send + Sync. (added: 2026-03-23, dispatch: T31+Jira)
- When adding fields to PipelineConfig, update test helpers in transitions.rs (two_stage_pipeline, three_stage_pipeline) and templates.rs struct literals. Use `#[serde(default)]` for optional fields. (added: 2026-03-23, dispatch: T31+Jira)
- machine.rs::event_name() is manual &'static str match — must update when adding DomainEvent variants. (added: 2026-03-23, dispatch: T31+Jira)
