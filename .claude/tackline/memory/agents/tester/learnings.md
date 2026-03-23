# Learnings: tester

## Codebase Patterns
- Product is a mission control for managing many AI coding agents simultaneously
- Primary user workflow: triage queue surfaces decisions → human approves/rejects → agents proceed
- Key UX contract: human-in-the-loop without context-switch overload
- Integration tests go in crates/server/tests/ as separate .rs files (added: 2026-03-23, dispatch: integration-tests)
- ApprovalService, InterruptClassifier, NotificationRouter are the key cross-cutting services (added: 2026-03-23, dispatch: integration-tests)

## Gotchas
- ApprovalStore and NotificationStore traits use RPITIT (native async), not async_trait (added: 2026-03-23, dispatch: integration-tests)
- TaskActorHandle.send_event takes an EventEnvelope — must construct full envelope with IDs (added: 2026-03-23, dispatch: integration-tests)

## Preferences
- Think like a product engineer who uses this tool daily to manage 10+ agents
- Test user journeys end-to-end, not just individual components
- Catch UX friction, not just crashes — if a workflow is confusing, that's a bug
- Use helper functions for repetitive test setup (event construction, actor spawning) (added: 2026-03-23, dispatch: integration-tests)

## Cross-Agent Notes
- (none yet)
