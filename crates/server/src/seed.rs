//! Demo seed data for Hackweek presentations.
//!
//! Set `MOLT_DEMO=1` to activate on startup.  A non-empty event store is
//! never overwritten — the check is "zero events", so the seed is idempotent
//! across restarts as long as one earlier seed run succeeded.

use std::sync::Arc;

use chrono::{Duration, Utc};
use tracing::{info, warn};

use molt_hub_core::events::store::EventStore;
use molt_hub_core::events::types::{DomainEvent, EventEnvelope};
use molt_hub_core::events::SqliteEventStore;
use molt_hub_core::model::{AgentId, EventId, Priority, SessionId, TaskId, TaskState};

/// Seed the store with realistic demo tasks if it is empty and `MOLT_DEMO=1`.
pub async fn maybe_seed_demo_data(store: &Arc<SqliteEventStore>) {
    if std::env::var("MOLT_DEMO").as_deref() != Ok("1") {
        return;
    }

    let since = chrono::DateTime::<Utc>::MIN_UTC;
    match store.get_events_since(since).await {
        Ok(existing) if !existing.is_empty() => {
            info!(
                count = existing.len(),
                "seed: store already populated, skipping demo seed"
            );
            return;
        }
        Err(e) => {
            warn!(error = %e, "seed: could not check event store, skipping");
            return;
        }
        _ => {}
    }

    info!("seed: inserting demo tasks...");
    let batch = build_demo_batch();
    match store.append_batch(batch).await {
        Ok(()) => info!("seed: demo events inserted"),
        Err(e) => warn!(error = %e, "seed: failed to insert demo events"),
    }
}

// ---------------------------------------------------------------------------
// Demo event batch
// ---------------------------------------------------------------------------

fn build_demo_batch() -> Vec<EventEnvelope> {
    let now = Utc::now();
    let session = SessionId::new();
    let project = "default".to_owned();

    // Helper: create an envelope offset by `hours` into the past.
    let ev = |task_id: &TaskId, hours: i64, payload: DomainEvent| EventEnvelope {
        id: EventId::new(),
        task_id: Some(task_id.clone()),
        project_id: project.clone(),
        session_id: session.clone(),
        timestamp: now - Duration::hours(hours),
        caused_by: None,
        payload,
    };

    let mut b: Vec<EventEnvelope> = Vec::new();

    // -----------------------------------------------------------------------
    // 1. Implement rate limiting for public API endpoints
    //    Stage: backlog  |  Priority: P1  |  Status: waiting
    // -----------------------------------------------------------------------
    let t1 = TaskId::new();
    b.push(ev(
        &t1,
        48,
        DomainEvent::TaskCreated {
            title: "Implement rate limiting for public API endpoints".into(),
            description: "Add per-IP and per-token rate limiting using a sliding-window \
                           algorithm. Target: 100 req/min for free tier, 1 000 req/min for pro."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P1,
        },
    ));

    // -----------------------------------------------------------------------
    // 2. Migrate auth service to JWT RS256 signing
    //    Stage: backlog  |  Priority: P2  |  Status: waiting
    // -----------------------------------------------------------------------
    let t2 = TaskId::new();
    b.push(ev(
        &t2,
        46,
        DomainEvent::TaskCreated {
            title: "Migrate auth service to JWT RS256 signing".into(),
            description: "Replace HS256 symmetric signing with RS256 asymmetric keypair. \
                           Rotate keys, update token verification in all downstream services."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P2,
        },
    ));

    // -----------------------------------------------------------------------
    // 3. Add OpenTelemetry spans to inference pipeline
    //    Stage: backlog  |  Priority: P3  |  Status: waiting
    // -----------------------------------------------------------------------
    let t3 = TaskId::new();
    b.push(ev(
        &t3,
        44,
        DomainEvent::TaskCreated {
            title: "Add OpenTelemetry spans to inference pipeline".into(),
            description: "Instrument the model inference path with OTEL spans for latency \
                           profiling. Export to Honeycomb via OTLP exporter."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P3,
        },
    ));

    // -----------------------------------------------------------------------
    // NEW-A. Design multi-tenant data isolation architecture
    //    Stage: planning  |  Priority: P1  |  Status: awaiting human review
    // -----------------------------------------------------------------------
    let ta = TaskId::new();
    let aa = AgentId::new();
    b.push(ev(
        &ta,
        42,
        DomainEvent::TaskCreated {
            title: "Design multi-tenant data isolation architecture".into(),
            description: "Define row-level security strategy for the analytics database. \
                           Each tenant's data must be logically isolated at the query layer."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P1,
        },
    ));
    b.push(ev(
        &ta,
        38,
        DomainEvent::TaskStageChanged {
            from_stage: "backlog".into(),
            to_stage: "planning".into(),
            new_state: TaskState::Pending,
        },
    ));
    b.push(ev(
        &ta,
        37,
        DomainEvent::AgentAssigned {
            agent_id: aa.clone(),
            agent_name: "claude-planner-1".into(),
        },
    ));
    b.push(ev(
        &ta,
        36,
        DomainEvent::AgentOutput {
            agent_id: aa.clone(),
            output: "Planning session: proposing 3 isolation strategies.\n\
                     Option A: Shared schema with tenant_id column + RLS policies (simpler, lower cost).\n\
                     Option B: Separate schema per tenant (good isolation, more migrations overhead).\n\
                     Option C: Separate database per tenant (max isolation, 10× infra cost).\n\
                     Recommendation: Option A with Postgres RLS. Awaiting human sign-off."
                .into(),
        },
    ));
    b.push(ev(
        &ta,
        35,
        DomainEvent::AgentCompleted {
            agent_id: aa.clone(),
            summary: Some(
                "Architecture proposal complete: 3 isolation strategies analyzed. \
                 Recommendation: Option A (shared schema + RLS). Awaiting human approval to proceed."
                    .into(),
            ),
        },
    ));

    // -----------------------------------------------------------------------
    // NEW-B. Evaluate vector database options for semantic search
    //    Stage: planning  |  Priority: P2  |  Status: awaiting human review
    // -----------------------------------------------------------------------
    let tb = TaskId::new();
    let ab = AgentId::new();
    b.push(ev(
        &tb,
        43,
        DomainEvent::TaskCreated {
            title: "Evaluate vector database options for semantic search".into(),
            description: "Compare pgvector, Pinecone, and Qdrant for the semantic search \
                           feature. Deliver a recommendation with latency and cost analysis."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P2,
        },
    ));
    b.push(ev(
        &tb,
        39,
        DomainEvent::TaskStageChanged {
            from_stage: "backlog".into(),
            to_stage: "planning".into(),
            new_state: TaskState::Pending,
        },
    ));
    b.push(ev(
        &tb,
        38,
        DomainEvent::AgentAssigned {
            agent_id: ab.clone(),
            agent_name: "claude-planner-2".into(),
        },
    ));
    b.push(ev(
        &tb,
        37,
        DomainEvent::AgentOutput {
            agent_id: ab.clone(),
            output: "Benchmark results (10 M vectors, p99 latency):\n\
                     • pgvector: 45 ms — lowest ops cost, already in our stack.\n\
                     • Qdrant: 8 ms — best latency, self-hosted.\n\
                     • Pinecone: 12 ms — managed, but $$$.\n\
                     Recommendation: pgvector for MVP (zero new infra). \
                     Migrate to Qdrant if p99 > 100 ms at scale. Awaiting human approval."
                .into(),
        },
    ));
    b.push(ev(
        &tb,
        36,
        DomainEvent::AgentCompleted {
            agent_id: ab.clone(),
            summary: Some(
                "Vector DB evaluation complete: pgvector recommended for MVP. \
                 Benchmarks and cost analysis ready for review. Awaiting human approval."
                    .into(),
            ),
        },
    ));

    // -----------------------------------------------------------------------
    // 4. Fix memory leak in streaming event handler
    //    Stage: in-progress  |  Priority: P0  |  Status: BLOCKED
    // -----------------------------------------------------------------------
    let t4 = TaskId::new();
    let a4 = AgentId::new();
    b.push(ev(
        &t4,
        36,
        DomainEvent::TaskCreated {
            title: "Fix memory leak in streaming event handler".into(),
            description: "Production instances OOM after ~6 h of uptime. Heap profile shows \
                           unbounded growth in the SSE subscriber map."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P0,
        },
    ));
    b.push(ev(
        &t4,
        30,
        DomainEvent::TaskStageChanged {
            from_stage: "backlog".into(),
            to_stage: "in-progress".into(),
            new_state: TaskState::Pending,
        },
    ));
    b.push(ev(
        &t4,
        29,
        DomainEvent::AgentAssigned {
            agent_id: a4.clone(),
            agent_name: "claude-agent-1".into(),
        },
    ));
    b.push(ev(
        &t4,
        28,
        DomainEvent::AgentOutput {
            agent_id: a4.clone(),
            output: "Analysing the event handler codebase…\n\
                     Found potential leak candidates in `sse_handler.rs` and `subscriber_map.rs`.\n\
                     The `SubscriberMap` uses a `DashMap<SessionId, mpsc::Sender<…>>` but dead \
                     senders are never pruned."
                .into(),
        },
    ));
    b.push(ev(
        &t4,
        27,
        DomainEvent::AgentOutput {
            agent_id: a4.clone(),
            output: "Confirmed: dropped WebSocket connections leave dangling `Arc<Sender>` \
                     references in the map.\nEach dead connection leaks ~4 KB. At 1 500 req/min \
                     this accumulates to ~350 MB/hr.\n\
                     Proposed fix: background task scans for closed senders every 30 s."
                .into(),
        },
    ));
    b.push(ev(
        &t4,
        26,
        DomainEvent::TaskBlocked {
            reason: "Cannot reproduce in staging — need a production memory dump to confirm \
                     the fix does not introduce a race condition. Waiting on SRE access."
                .into(),
        },
    ));

    // -----------------------------------------------------------------------
    // 5. Refactor database connection pool (pgbouncer)
    //    Stage: in-progress  |  Priority: P1  |  Status: running
    // -----------------------------------------------------------------------
    let t5 = TaskId::new();
    let a5 = AgentId::new();
    b.push(ev(
        &t5,
        24,
        DomainEvent::TaskCreated {
            title: "Refactor database connection pool (pgbouncer)".into(),
            description: "Current direct pg connection pool saturates under load. \
                           Introduce pgbouncer in transaction mode; update pool to 5 \
                           connections per worker."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P1,
        },
    ));
    b.push(ev(
        &t5,
        20,
        DomainEvent::TaskStageChanged {
            from_stage: "backlog".into(),
            to_stage: "in-progress".into(),
            new_state: TaskState::Pending,
        },
    ));
    b.push(ev(
        &t5,
        19,
        DomainEvent::AgentAssigned {
            agent_id: a5.clone(),
            agent_name: "claude-agent-2".into(),
        },
    ));
    b.push(ev(
        &t5,
        18,
        DomainEvent::AgentOutput {
            agent_id: a5.clone(),
            output: "Reading current pool config in `crates/server/src/db.rs`…\n\
                     Found `max_connections = 20` on `SqlitePoolOptions`.\n\
                     Locating the Postgres connection in `crates/analytics/src/store.rs`…"
                .into(),
        },
    ));
    b.push(ev(
        &t5,
        17,
        DomainEvent::AgentOutput {
            agent_id: a5.clone(),
            output: "Found `sqlx::PgPool` with hard-coded `min_connections = 1, max_connections = 10`.\n\
                     Drafting pgbouncer config: pool_mode = transaction, max_client_conn = 100, \
                     default_pool_size = 5.\n\
                     Creating `infra/pgbouncer/pgbouncer.ini`…"
                .into(),
        },
    ));
    b.push(ev(
        &t5,
        16,
        DomainEvent::AgentOutput {
            agent_id: a5.clone(),
            output: "Updated `docker-compose.yml` with pgbouncer sidecar.\n\
                     Updating `DATABASE_URL` env to point to pgbouncer port 6432.\n\
                     Running migration smoke tests…"
                .into(),
        },
    ));

    // -----------------------------------------------------------------------
    // 6. Implement OAuth refresh token rotation
    //    Stage: in-progress  |  Priority: P2  |  Status: running
    // -----------------------------------------------------------------------
    let t6 = TaskId::new();
    let a6 = AgentId::new();
    b.push(ev(
        &t6,
        22,
        DomainEvent::TaskCreated {
            title: "Implement OAuth refresh token rotation".into(),
            description: "Refresh tokens must be single-use. On each refresh, invalidate \
                           the old token and issue a new pair. Track token families to \
                           detect replay attacks."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P2,
        },
    ));
    b.push(ev(
        &t6,
        18,
        DomainEvent::TaskStageChanged {
            from_stage: "backlog".into(),
            to_stage: "in-progress".into(),
            new_state: TaskState::Pending,
        },
    ));
    b.push(ev(
        &t6,
        17,
        DomainEvent::AgentAssigned {
            agent_id: a6.clone(),
            agent_name: "claude-agent-3".into(),
        },
    ));
    b.push(ev(
        &t6,
        16,
        DomainEvent::AgentOutput {
            agent_id: a6.clone(),
            output: "Reading OAuth token store in `crates/server/src/integrations/oauth.rs`…\n\
                     Current implementation issues long-lived refresh tokens with no rotation. \
                     Tokens stored in the OS keychain via the `keyring` crate."
                .into(),
        },
    ));
    b.push(ev(
        &t6,
        15,
        DomainEvent::AgentOutput {
            agent_id: a6.clone(),
            output: "Implementing token family tracking: adding `token_family_id` column.\n\
                     Detection logic: if a revoked refresh token is replayed, invalidate the \
                     entire family.\n\
                     Writing tests for the rotation flow…"
                .into(),
        },
    ));

    // -----------------------------------------------------------------------
    // 7. Add streaming support to TypeScript SDK
    //    Stage: review  |  Priority: P1  |  Status: waiting
    // -----------------------------------------------------------------------
    let t7 = TaskId::new();
    let a7 = AgentId::new();
    b.push(ev(
        &t7,
        40,
        DomainEvent::TaskCreated {
            title: "Add streaming support to TypeScript SDK".into(),
            description: "Expose Server-Sent Events from the inference API in the TS SDK. \
                           Support both async iterators and callback-based APIs for \
                           backwards compatibility."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P1,
        },
    ));
    b.push(ev(
        &t7,
        36,
        DomainEvent::TaskStageChanged {
            from_stage: "backlog".into(),
            to_stage: "in-progress".into(),
            new_state: TaskState::Pending,
        },
    ));
    b.push(ev(
        &t7,
        35,
        DomainEvent::AgentAssigned {
            agent_id: a7.clone(),
            agent_name: "claude-agent-4".into(),
        },
    ));
    b.push(ev(
        &t7,
        34,
        DomainEvent::AgentOutput {
            agent_id: a7.clone(),
            output: "Scaffolding streaming client in `sdk/typescript/src/streaming.ts`…\n\
                     Using native `EventSource` in browsers and `eventsource` package in Node."
                .into(),
        },
    ));
    b.push(ev(
        &t7,
        33,
        DomainEvent::AgentOutput {
            agent_id: a7.clone(),
            output: "Implemented `createStream(options)` returning `AsyncIterable<StreamChunk>`.\n\
                     Added `onChunk` callback shim for backwards compat.\n\
                     Writing Jest tests with MSW interceptors…"
                .into(),
        },
    ));
    b.push(ev(
        &t7,
        32,
        DomainEvent::AgentOutput {
            agent_id: a7.clone(),
            output: "All 23 tests passing. Bundle size delta +1.4 KB gzipped (acceptable).\n\
                     Updating CHANGELOG and JSDoc…"
                .into(),
        },
    ));
    // Move to review BEFORE AgentCompleted so the task stays InProgress
    // and the stage scan shows it as "waiting" in review.
    b.push(ev(
        &t7,
        31,
        DomainEvent::TaskStageChanged {
            from_stage: "in-progress".into(),
            to_stage: "review".into(),
            new_state: TaskState::InProgress,
        },
    ));
    b.push(ev(
        &t7,
        30,
        DomainEvent::AgentCompleted {
            agent_id: a7.clone(),
            summary: Some(
                "Streaming support implemented: AsyncIterable + callback APIs, 23 tests passing, \
                 +1.4 KB bundle delta. Ready for code review."
                    .into(),
            ),
        },
    ));

    // -----------------------------------------------------------------------
    // 8. Harden CORS policy across all production API routes
    //    Stage: testing  |  Priority: P1  |  Status: waiting
    // -----------------------------------------------------------------------
    let t8 = TaskId::new();
    let a8 = AgentId::new();
    b.push(ev(
        &t8,
        55,
        DomainEvent::TaskCreated {
            title: "Harden CORS policy across all production API routes".into(),
            description: "Current CORS config uses wildcard `Access-Control-Allow-Origin: *`. \
                           Lock down to explicit allowlist; block credentials on public endpoints."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P1,
        },
    ));
    b.push(ev(
        &t8,
        50,
        DomainEvent::TaskStageChanged {
            from_stage: "backlog".into(),
            to_stage: "in-progress".into(),
            new_state: TaskState::Pending,
        },
    ));
    b.push(ev(
        &t8,
        49,
        DomainEvent::AgentAssigned {
            agent_id: a8.clone(),
            agent_name: "claude-agent-5".into(),
        },
    ));
    b.push(ev(
        &t8,
        48,
        DomainEvent::AgentOutput {
            agent_id: a8.clone(),
            output: "Auditing all CORS middleware across the codebase…\n\
                     Found 3 locations: `crates/server/src/serve.rs`, \
                     `api-gateway/middleware/cors.ts`, and `cdn/edge-functions/cors.js`.\n\
                     All currently use wildcard origin."
                .into(),
        },
    ));
    b.push(ev(
        &t8,
        47,
        DomainEvent::AgentOutput {
            agent_id: a8.clone(),
            output: "Updating allowlists: `[\"https://app.molthub.dev\", \"https://molthub.dev\"]`.\n\
                     Adding `Vary: Origin` header to prevent cache poisoning.\n\
                     Blocking `credentials: include` on public endpoints."
                .into(),
        },
    ));
    b.push(ev(
        &t8,
        46,
        DomainEvent::TaskStageChanged {
            from_stage: "in-progress".into(),
            to_stage: "review".into(),
            new_state: TaskState::InProgress,
        },
    ));
    b.push(ev(
        &t8,
        12,
        DomainEvent::TaskStageChanged {
            from_stage: "review".into(),
            to_stage: "testing".into(),
            new_state: TaskState::InProgress,
        },
    ));
    b.push(ev(
        &t8,
        11,
        DomainEvent::AgentCompleted {
            agent_id: a8.clone(),
            summary: Some(
                "CORS policy hardened across all 3 locations. Allowlist configured, \
                 Vary header added, credentials blocked on public endpoints. In testing."
                    .into(),
            ),
        },
    ));

    // -----------------------------------------------------------------------
    // 9. Update deprecated openssl 0.10 → 1.0 dependency
    //    Stage: testing  |  Priority: P2  |  Status: waiting
    // -----------------------------------------------------------------------
    let t9 = TaskId::new();
    let a9 = AgentId::new();
    b.push(ev(
        &t9,
        70,
        DomainEvent::TaskCreated {
            title: "Update deprecated openssl 0.10 → 1.0 dependency".into(),
            description: "openssl 0.10 has 3 known CVEs. Upgrade to 1.0, run \
                           `cargo audit` to confirm no remaining advisories."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P2,
        },
    ));
    b.push(ev(
        &t9,
        65,
        DomainEvent::TaskStageChanged {
            from_stage: "backlog".into(),
            to_stage: "in-progress".into(),
            new_state: TaskState::Pending,
        },
    ));
    b.push(ev(
        &t9,
        64,
        DomainEvent::AgentAssigned {
            agent_id: a9.clone(),
            agent_name: "claude-agent-6".into(),
        },
    ));
    b.push(ev(
        &t9,
        63,
        DomainEvent::AgentOutput {
            agent_id: a9.clone(),
            output: "Running `cargo tree -i openssl`…\n\
                     Found 4 transitive dependants.\n\
                     Updating `Cargo.toml` to pin `openssl = \"1.0\"`…"
                .into(),
        },
    ));
    b.push(ev(
        &t9,
        62,
        DomainEvent::AgentOutput {
            agent_id: a9.clone(),
            output: "Build succeeded. Running `cargo audit`… 0 vulnerabilities found.\n\
                     All existing tests pass. No API surface changes."
                .into(),
        },
    ));
    b.push(ev(
        &t9,
        61,
        DomainEvent::TaskStageChanged {
            from_stage: "in-progress".into(),
            to_stage: "review".into(),
            new_state: TaskState::InProgress,
        },
    ));
    b.push(ev(
        &t9,
        8,
        DomainEvent::TaskStageChanged {
            from_stage: "review".into(),
            to_stage: "testing".into(),
            new_state: TaskState::InProgress,
        },
    ));
    b.push(ev(
        &t9,
        7,
        DomainEvent::AgentCompleted {
            agent_id: a9.clone(),
            summary: Some(
                "openssl upgraded to 1.0: 0 CVEs remaining per cargo audit. \
                 All tests pass. No API surface changes. In testing."
                    .into(),
            ),
        },
    ));

    // -----------------------------------------------------------------------
    // 10. Deploy inference caching layer (Redis)
    //     Stage: deployment  |  Priority: P2  |  Status: complete
    // -----------------------------------------------------------------------
    let t10 = TaskId::new();
    let a10 = AgentId::new();
    b.push(ev(
        &t10,
        168,
        DomainEvent::TaskCreated {
            title: "Deploy inference caching layer (Redis)".into(),
            description: "Cache repeat inference requests by prompt hash + model version. \
                           Target 40 % cache hit rate for common prompts. Use Redis with 1 hr TTL."
                .into(),
            initial_stage: "backlog".into(),
            priority: Priority::P2,
        },
    ));
    b.push(ev(
        &t10,
        160,
        DomainEvent::TaskStageChanged {
            from_stage: "backlog".into(),
            to_stage: "in-progress".into(),
            new_state: TaskState::Pending,
        },
    ));
    b.push(ev(
        &t10,
        159,
        DomainEvent::AgentAssigned {
            agent_id: a10.clone(),
            agent_name: "claude-agent-7".into(),
        },
    ));
    b.push(ev(
        &t10,
        158,
        DomainEvent::AgentOutput {
            agent_id: a10.clone(),
            output: "Designing cache key schema: `sha256(model_id + normalize(prompt))` \
                     truncated to 32 chars.\n\
                     Adding `redis` crate dependency. Wiring `RedisPool` into the \
                     inference handler…"
                .into(),
        },
    ));
    b.push(ev(
        &t10,
        156,
        DomainEvent::AgentOutput {
            agent_id: a10.clone(),
            output: "Cache hit path implemented. Adding Prometheus metrics: \
                     `cache_hit_total`, `cache_miss_total`.\n\
                     Writing integration tests with `fakeredis`…"
                .into(),
        },
    ));
    b.push(ev(
        &t10,
        154,
        DomainEvent::AgentOutput {
            agent_id: a10.clone(),
            output: "Tests passing. Cache hit rate in smoke tests: 43 % (above 40 % target).\n\
                     Updating Helm chart to add Redis sidecar."
                .into(),
        },
    ));
    b.push(ev(
        &t10,
        150,
        DomainEvent::TaskStageChanged {
            from_stage: "in-progress".into(),
            to_stage: "review".into(),
            new_state: TaskState::InProgress,
        },
    ));
    b.push(ev(
        &t10,
        100,
        DomainEvent::TaskStageChanged {
            from_stage: "review".into(),
            to_stage: "testing".into(),
            new_state: TaskState::InProgress,
        },
    ));
    // Move to deployment while still InProgress so the next AgentCompleted
    // terminates the machine in the "deployment" stage.
    b.push(ev(
        &t10,
        49,
        DomainEvent::TaskStageChanged {
            from_stage: "testing".into(),
            to_stage: "deployment".into(),
            new_state: TaskState::InProgress,
        },
    ));
    b.push(ev(
        &t10,
        48,
        DomainEvent::AgentCompleted {
            agent_id: a10.clone(),
            summary: Some(
                "Redis caching deployed: 43 % hit rate in smoke tests, 1 hr TTL. \
                 Prometheus metrics added."
                    .into(),
            ),
        },
    ));

    b
}
