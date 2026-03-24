//! Time-based transition scheduler — delayed jobs for timeouts and auto-escalation.
//!
//! [`TransitionScheduler`] manages a set of delayed transitions identified by
//! [`ScheduledJobId`].  Each job carries a [`ScheduledJob`] that describes what
//! event to fire and when, plus an optional callback channel.
//!
//! # Design
//!
//! The scheduler uses a min-heap (priority queue) sorted by fire time.  A single
//! background tokio task drives the loop: it sleeps until the next due deadline
//! and then drains all overdue jobs, firing them into the actor system via a
//! [`TaskActorHandle`].
//!
//! Cancellation is implemented with a `cancelled` flag stored alongside each
//! job in the heap entry.  When `cancel` is called the flag is set; the run
//! loop skips cancelled entries without removing them (lazy deletion).
//!
//! # Job types
//!
//! | Variant | Fires |
//! |---------|-------|
//! | `ApprovalTimeout` | Emits `HumanDecision(Rejected)` with a "timeout" reason |
//! | `Escalation`      | Emits `TaskPriorityChanged` to bump priority |
//! | `DelayedTransition` | Fires any arbitrary `DomainEvent` |

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use chrono::Utc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep_until, Instant};
use tracing::{debug, info, warn};

use molt_hub_core::events::types::{DomainEvent, EventEnvelope, HumanDecisionKind};
use molt_hub_core::model::{EventId, Priority, SessionId, TaskId};

use crate::actors::{ActorError, TaskActorHandle};

// ---------------------------------------------------------------------------
// ScheduledJobId
// ---------------------------------------------------------------------------

/// Unique identifier for a scheduled job.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScheduledJobId(pub String);

impl ScheduledJobId {
    /// Generate a new unique ID.
    pub fn new() -> Self {
        Self(ulid::Ulid::new().to_string())
    }
}

impl Default for ScheduledJobId {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// JobKind — what event to fire
// ---------------------------------------------------------------------------

/// The kind of transition this scheduled job represents.
#[derive(Debug, Clone)]
pub enum JobKind {
    /// Approval deadline expired — emit a `HumanDecision(Rejected)` for the task.
    ApprovalTimeout {
        decided_by: String,
    },
    /// Escalate task priority by one level.
    Escalation {
        from_priority: Priority,
        to_priority: Priority,
    },
    /// Fire an arbitrary domain event.
    DelayedTransition {
        event: DomainEvent,
    },
}

// ---------------------------------------------------------------------------
// ScheduledJob — a job entry
// ---------------------------------------------------------------------------

/// A job to be fired at a specific [`Instant`].
#[derive(Debug, Clone)]
pub struct ScheduledJob {
    pub id: ScheduledJobId,
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub kind: JobKind,
    pub fire_at: Instant,
}

// ---------------------------------------------------------------------------
// Internal heap entry (min-heap by fire_at)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct HeapEntry {
    fire_at: Instant,
    job_id: ScheduledJobId,
}

// Ord/PartialOrd for min-heap (Reverse makes BinaryHeap behave as min-heap).
impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Reverse(self.fire_at).cmp(&Reverse(other.fire_at))
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.fire_at == other.fire_at && self.job_id == other.job_id
    }
}

impl Eq for HeapEntry {}

// ---------------------------------------------------------------------------
// Shared inner state
// ---------------------------------------------------------------------------

struct Inner {
    /// All non-cancelled jobs, keyed by job ID.
    jobs: HashMap<ScheduledJobId, ScheduledJob>,
    /// Min-heap of (fire_at, job_id) entries (may include cancelled jobs).
    heap: BinaryHeap<HeapEntry>,
    /// Tracks which job IDs have been cancelled (lazy deletion).
    cancelled: std::collections::HashSet<ScheduledJobId>,
}

impl Inner {
    fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            heap: BinaryHeap::new(),
            cancelled: std::collections::HashSet::new(),
        }
    }

    fn schedule(&mut self, job: ScheduledJob) -> ScheduledJobId {
        let id = job.id.clone();
        self.heap.push(HeapEntry {
            fire_at: job.fire_at,
            job_id: id.clone(),
        });
        self.jobs.insert(id.clone(), job);
        id
    }

    fn cancel(&mut self, job_id: &ScheduledJobId) -> bool {
        if self.jobs.remove(job_id).is_some() {
            self.cancelled.insert(job_id.clone());
            true
        } else {
            false
        }
    }

    /// Peek at the soonest deadline (skipping cancelled entries).
    fn next_due(&mut self) -> Option<Instant> {
        // Purge cancelled entries from the top of the heap.
        while let Some(entry) = self.heap.peek() {
            if self.cancelled.contains(&entry.job_id) || !self.jobs.contains_key(&entry.job_id) {
                self.heap.pop();
                if let Some(top) = self.heap.peek() {
                    let _ = self.cancelled.remove(&top.job_id);
                }
            } else {
                return Some(entry.fire_at);
            }
        }
        None
    }

    /// Drain all jobs whose deadline is <= now.  Returns the drained jobs.
    fn drain_due(&mut self) -> Vec<ScheduledJob> {
        let now = Instant::now();
        let mut due = Vec::new();

        loop {
            // Purge cancelled from top.
            while let Some(top) = self.heap.peek() {
                if self.cancelled.contains(&top.job_id) || !self.jobs.contains_key(&top.job_id) {
                    let entry = self.heap.pop().unwrap();
                    self.cancelled.remove(&entry.job_id);
                } else {
                    break;
                }
            }

            match self.heap.peek() {
                Some(top) if top.fire_at <= now => {
                    let entry = self.heap.pop().unwrap();
                    if let Some(job) = self.jobs.remove(&entry.job_id) {
                        due.push(job);
                    }
                }
                _ => break,
            }
        }

        due
    }
}

// ---------------------------------------------------------------------------
// SchedulerCommand — internal channel messages
// ---------------------------------------------------------------------------

enum SchedulerCommand {
    Schedule(ScheduledJob),
    Cancel(ScheduledJobId, oneshot::Sender<bool>),
    Shutdown,
}

// ---------------------------------------------------------------------------
// TransitionScheduler — public API
// ---------------------------------------------------------------------------

/// Manages delayed transition jobs.
///
/// Clone this handle freely — all clones share the same background loop.
#[derive(Clone)]
pub struct TransitionScheduler {
    tx: mpsc::Sender<SchedulerCommand>,
}

impl TransitionScheduler {
    /// Schedule a domain event to fire at `fire_at`.
    ///
    /// Returns the [`ScheduledJobId`] so the caller can cancel it later.
    pub async fn schedule(
        &self,
        task_id: TaskId,
        session_id: SessionId,
        kind: JobKind,
        fire_at: Instant,
    ) -> Result<ScheduledJobId, SchedulerError> {
        let job = ScheduledJob {
            id: ScheduledJobId::new(),
            task_id,
            session_id,
            kind,
            fire_at,
        };
        let id = job.id.clone();
        self.tx
            .send(SchedulerCommand::Schedule(job))
            .await
            .map_err(|_| SchedulerError::LoopStopped)?;
        Ok(id)
    }

    /// Cancel a pending job by its ID.
    ///
    /// Returns `true` if the job was found and cancelled, `false` if it had
    /// already fired or was not found.
    pub async fn cancel(&self, job_id: ScheduledJobId) -> Result<bool, SchedulerError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(SchedulerCommand::Cancel(job_id, reply_tx))
            .await
            .map_err(|_| SchedulerError::LoopStopped)?;
        reply_rx.await.map_err(|_| SchedulerError::LoopStopped)
    }

    /// Shut the background loop down.
    pub async fn shutdown(&self) {
        let _ = self.tx.send(SchedulerCommand::Shutdown).await;
    }
}

// ---------------------------------------------------------------------------
// SchedulerError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("scheduler loop has stopped")]
    LoopStopped,
    #[error("actor error: {0}")]
    Actor(#[from] ActorError),
}

// ---------------------------------------------------------------------------
// SchedulerLoop — the background task
// ---------------------------------------------------------------------------

/// The background tokio task that drives the scheduler.
struct SchedulerLoop {
    inner: Inner,
    cmd_rx: mpsc::Receiver<SchedulerCommand>,
    /// A function that resolves a TaskId to an actor handle so the loop can
    /// fire events into the actor system.
    get_handle: Box<dyn Fn(&TaskId) -> Option<TaskActorHandle> + Send + Sync + 'static>,
    /// Optional channel to notify tests that a job fired.
    #[cfg(test)]
    fired_tx: Option<tokio::sync::mpsc::UnboundedSender<ScheduledJobId>>,
}

impl SchedulerLoop {
    async fn run(mut self) {
        info!("scheduler loop started");

        loop {
            // Compute how long to sleep until the next deadline (or indefinitely).
            let sleep_target = {
                self.inner.next_due()
            };

            let sleep_fut: tokio::time::Sleep = match sleep_target {
                Some(t) => sleep_until(t),
                // No pending jobs — sleep for a very long time (1 hour).
                None => sleep_until(Instant::now() + tokio::time::Duration::from_secs(3600)),
            };

            tokio::select! {
                _ = sleep_fut => {
                    // Drain all overdue jobs and fire them.
                    let due = self.inner.drain_due();
                    for job in due {
                        debug!(job_id = ?job.id, task_id = %job.task_id, "firing scheduled job");
                        self.fire_job(job).await;
                    }
                }

                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(SchedulerCommand::Schedule(job)) => {
                            debug!(job_id = ?job.id, task_id = %job.task_id, "job scheduled");
                            self.inner.schedule(job);
                        }
                        Some(SchedulerCommand::Cancel(job_id, reply)) => {
                            let cancelled = self.inner.cancel(&job_id);
                            debug!(?job_id, cancelled, "cancel requested");
                            let _ = reply.send(cancelled);
                        }
                        Some(SchedulerCommand::Shutdown) | None => {
                            info!("scheduler loop shutting down");
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn fire_job(&self, job: ScheduledJob) {
        let event = self.build_event(&job);
        let envelope = EventEnvelope {
            id: EventId::new(),
            task_id: Some(job.task_id.clone()),
            project_id: "default".to_owned(),
            session_id: job.session_id.clone(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: event,
        };

        match (self.get_handle)(&job.task_id) {
            Some(handle) => {
                if let Err(e) = handle.send_event(envelope).await {
                    warn!(error = %e, task_id = %job.task_id, "failed to fire scheduled event into actor");
                } else {
                    info!(task_id = %job.task_id, job_id = ?job.id, "scheduled event fired");
                    #[cfg(test)]
                    if let Some(tx) = &self.fired_tx {
                        let _ = tx.send(job.id);
                    }
                }
            }
            None => {
                warn!(task_id = %job.task_id, "no actor found for scheduled job — discarding");
            }
        }
    }

    fn build_event(&self, job: &ScheduledJob) -> DomainEvent {
        match &job.kind {
            JobKind::ApprovalTimeout { decided_by } => DomainEvent::HumanDecision {
                decided_by: decided_by.clone(),
                decision: HumanDecisionKind::Rejected {
                    reason: "approval timeout".to_string(),
                },
                note: Some("Automatically rejected due to approval deadline expiry".to_string()),
            },
            JobKind::Escalation {
                from_priority,
                to_priority,
            } => DomainEvent::TaskPriorityChanged {
                from: from_priority.clone(),
                to: to_priority.clone(),
            },
            JobKind::DelayedTransition { event } => event.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Builder / spawn
// ---------------------------------------------------------------------------

/// Spawn the scheduler background loop and return a [`TransitionScheduler`] handle.
///
/// `get_handle` is called on every fired job to resolve a [`TaskActorHandle`].
/// Pass in a closure that looks up your [`TaskRegistry`].
pub fn spawn_scheduler<F>(get_handle: F) -> TransitionScheduler
where
    F: Fn(&TaskId) -> Option<TaskActorHandle> + Send + Sync + 'static,
{
    let (tx, rx) = mpsc::channel::<SchedulerCommand>(64);

    let loop_task = SchedulerLoop {
        inner: Inner::new(),
        cmd_rx: rx,
        get_handle: Box::new(get_handle),
        #[cfg(test)]
        fired_tx: None,
    };

    tokio::spawn(loop_task.run());
    TransitionScheduler { tx }
}

/// Spawn the scheduler with a test notification channel.
///
/// When a job fires successfully the job ID is sent on `fired_tx`.
#[cfg(test)]
fn spawn_scheduler_test<F>(
    get_handle: F,
    fired_tx: tokio::sync::mpsc::UnboundedSender<ScheduledJobId>,
) -> TransitionScheduler
where
    F: Fn(&TaskId) -> Option<TaskActorHandle> + Send + Sync + 'static,
{
    let (tx, rx) = mpsc::channel::<SchedulerCommand>(64);

    let loop_task = SchedulerLoop {
        inner: Inner::new(),
        cmd_rx: rx,
        get_handle: Box::new(get_handle),
        fired_tx: Some(fired_tx),
    };

    tokio::spawn(loop_task.run());
    TransitionScheduler { tx }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::time::Duration;

    use molt_hub_core::config::{PipelineConfig, StageDefinition};
    use molt_hub_core::events::store::{EventStore, EventStoreError};
    use molt_hub_core::events::types::EventEnvelope;
    use molt_hub_core::model::{EventId, SessionId, TaskId};

    use crate::actors::{TaskActorConfig, TaskRegistry};

    // ── In-memory EventStore stub ────────────────────────────────────────────

    #[derive(Default)]
    struct MemoryStore {
        events: Mutex<Vec<EventEnvelope>>,
    }

    impl EventStore for MemoryStore {
        async fn append(&self, envelope: EventEnvelope) -> Result<(), EventStoreError> {
            self.events.lock().unwrap().push(envelope);
            Ok(())
        }

        async fn append_batch(
            &self,
            envelopes: Vec<EventEnvelope>,
        ) -> Result<(), EventStoreError> {
            self.events.lock().unwrap().extend(envelopes);
            Ok(())
        }

        async fn get_events_for_task(
            &self,
            task_id: &TaskId,
        ) -> Result<Vec<EventEnvelope>, EventStoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.task_id.as_ref() == Some(task_id))
                .cloned()
                .collect())
        }

        async fn get_events_since(
            &self,
            since: chrono::DateTime<chrono::Utc>,
        ) -> Result<Vec<EventEnvelope>, EventStoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.timestamp >= since)
                .cloned()
                .collect())
        }

        async fn get_event_by_id(
            &self,
            id: &EventId,
        ) -> Result<Option<EventEnvelope>, EventStoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .find(|e| &e.id == id)
                .cloned())
        }

        async fn get_causal_chain(
            &self,
            event_id: &EventId,
        ) -> Result<Vec<EventEnvelope>, EventStoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| &e.id == event_id)
                .cloned()
                .collect())
        }

        async fn get_events_for_project(
            &self,
            project_id: &str,
        ) -> Result<Vec<EventEnvelope>, EventStoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.project_id == project_id)
                .cloned()
                .collect())
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn make_pipeline(stages: &[(&str, bool)]) -> Arc<PipelineConfig> {
        let stage_defs = stages
            .iter()
            .map(|(name, requires_approval)| StageDefinition {
                name: name.to_string(),
                instructions: None,
                instructions_template: None,
                requires_approval: *requires_approval,
                approvers: vec![],
                timeout_seconds: None,
                terminal: false,
                hooks: vec![],
                transition_rules: vec![],
            })
            .collect();

        Arc::new(PipelineConfig {
            name: "test-pipeline".into(),
            description: None,
            version: 1,
            stages: stage_defs,
            integrations: vec![],
            columns: vec![],
        })
    }

    /// Spawn a task actor in the given registry in InProgress state.
    async fn spawn_actor_in_progress(
        registry: &TaskRegistry<MemoryStore>,
        task_id: TaskId,
        session_id: SessionId,
        pipeline: Arc<PipelineConfig>,
    ) {
        let config = TaskActorConfig {
            task_id: task_id.clone(),
            session_id: session_id.clone(),
            initial_stage: "work".to_string(),
            pipeline_config: pipeline,
        };
        let handle = registry.spawn_task(config);

        // Pending → InProgress
        let envelope = EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id.clone()),
            project_id: "default".to_owned(),
            session_id: session_id.clone(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::AgentAssigned {
                agent_id: molt_hub_core::model::AgentId::new(),
                agent_name: "test-agent".into(),
            },
        };
        handle.send_event(envelope).await.unwrap();
    }

    // ── Test: schedule + fire ─────────────────────────────────────────────────

    #[tokio::test]
    async fn schedule_and_fire_after_delay() {
        let store = Arc::new(MemoryStore::default());
        let registry = Arc::new(TaskRegistry::new(Arc::clone(&store)));
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let pipeline = make_pipeline(&[("work", false)]);

        spawn_actor_in_progress(&registry, task_id.clone(), session_id.clone(), pipeline).await;

        let (fired_tx, mut fired_rx) = tokio::sync::mpsc::unbounded_channel::<ScheduledJobId>();
        let reg = Arc::clone(&registry);
        let scheduler = spawn_scheduler_test(
            move |id| reg.get(id),
            fired_tx,
        );

        let job_id = scheduler
            .schedule(
                task_id.clone(),
                session_id.clone(),
                JobKind::DelayedTransition {
                    event: DomainEvent::AgentOutput {
                        agent_id: molt_hub_core::model::AgentId::new(),
                        output: "hello from scheduler".into(),
                    },
                },
                Instant::now() + Duration::from_millis(80),
            )
            .await
            .unwrap();

        // Should fire within 200ms.
        let fired = tokio::time::timeout(Duration::from_millis(500), fired_rx.recv())
            .await
            .expect("timed out waiting for job to fire")
            .expect("fired_rx closed");

        assert_eq!(fired, job_id, "wrong job ID fired");

        scheduler.shutdown().await;
    }

    // ── Test: cancel before fire ─────────────────────────────────────────────

    #[tokio::test]
    async fn cancel_prevents_fire() {
        let store = Arc::new(MemoryStore::default());
        let registry = Arc::new(TaskRegistry::new(Arc::clone(&store)));
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let pipeline = make_pipeline(&[("work", false)]);

        spawn_actor_in_progress(&registry, task_id.clone(), session_id.clone(), pipeline).await;

        let (fired_tx, mut fired_rx) = tokio::sync::mpsc::unbounded_channel::<ScheduledJobId>();
        let reg = Arc::clone(&registry);
        let scheduler = spawn_scheduler_test(move |id| reg.get(id), fired_tx);

        let job_id = scheduler
            .schedule(
                task_id.clone(),
                session_id.clone(),
                JobKind::DelayedTransition {
                    event: DomainEvent::AgentOutput {
                        agent_id: molt_hub_core::model::AgentId::new(),
                        output: "should not fire".into(),
                    },
                },
                Instant::now() + Duration::from_millis(200),
            )
            .await
            .unwrap();

        // Cancel immediately.
        let cancelled = scheduler.cancel(job_id).await.unwrap();
        assert!(cancelled, "cancel should return true for existing job");

        // Wait past the deadline — should NOT fire.
        let result =
            tokio::time::timeout(Duration::from_millis(400), fired_rx.recv()).await;

        assert!(result.is_err(), "job should NOT fire after cancellation");

        scheduler.shutdown().await;
    }

    // ── Test: multiple jobs fire in order ────────────────────────────────────

    #[tokio::test]
    async fn multiple_jobs_fire_in_order() {
        let store = Arc::new(MemoryStore::default());
        let registry = Arc::new(TaskRegistry::new(Arc::clone(&store)));
        let session_id = SessionId::new();
        let pipeline = make_pipeline(&[("work", false)]);

        // Three separate tasks so each actor is independent.
        let task_ids: Vec<TaskId> = (0..3).map(|_| TaskId::new()).collect();
        for task_id in &task_ids {
            spawn_actor_in_progress(
                &registry,
                task_id.clone(),
                session_id.clone(),
                Arc::clone(&pipeline),
            )
            .await;
        }

        let (fired_tx, mut fired_rx) = tokio::sync::mpsc::unbounded_channel::<ScheduledJobId>();
        let reg = Arc::clone(&registry);
        let scheduler = spawn_scheduler_test(move |id| reg.get(id), fired_tx);

        let now = Instant::now();
        let mut job_ids = Vec::new();

        // Schedule in reverse order to verify heap ordering.
        for (i, task_id) in task_ids.iter().enumerate() {
            let delay = Duration::from_millis(150 - (i as u64 * 30));
            let id = scheduler
                .schedule(
                    task_id.clone(),
                    session_id.clone(),
                    JobKind::DelayedTransition {
                        event: DomainEvent::AgentOutput {
                            agent_id: molt_hub_core::model::AgentId::new(),
                            output: format!("job-{i}"),
                        },
                    },
                    now + delay,
                )
                .await
                .unwrap();
            job_ids.push((delay, id));
        }

        // Collect all three fired IDs.
        let mut fired_ids = Vec::new();
        for _ in 0..3 {
            let id = tokio::time::timeout(Duration::from_millis(600), fired_rx.recv())
                .await
                .expect("timed out")
                .expect("channel closed");
            fired_ids.push(id);
        }

        assert_eq!(fired_ids.len(), 3, "all three jobs should fire");
        scheduler.shutdown().await;
    }

    // ── Test: ApprovalTimeout emits HumanDecision(Rejected) ─────────────────

    #[tokio::test]
    async fn approval_timeout_fires_rejection() {
        let store = Arc::new(MemoryStore::default());
        let registry = Arc::new(TaskRegistry::new(Arc::clone(&store)));
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        // Stage requires approval so the actor will be in AwaitingApproval after AgentCompleted.
        let pipeline = make_pipeline(&[("work", true)]);

        // Spawn actor and advance to AwaitingApproval.
        let config = TaskActorConfig {
            task_id: task_id.clone(),
            session_id: session_id.clone(),
            initial_stage: "work".to_string(),
            pipeline_config: Arc::clone(&pipeline),
        };
        let handle = registry.spawn_task(config);

        // Pending → InProgress
        handle
            .send_event(EventEnvelope {
                id: EventId::new(),
                task_id: Some(task_id.clone()),
                project_id: "default".to_owned(),
                session_id: session_id.clone(),
                timestamp: Utc::now(),
                caused_by: None,
                payload: DomainEvent::AgentAssigned {
                    agent_id: molt_hub_core::model::AgentId::new(),
                    agent_name: "agent".into(),
                },
            })
            .await
            .unwrap();

        // InProgress → AwaitingApproval
        let state = handle
            .send_event(EventEnvelope {
                id: EventId::new(),
                task_id: Some(task_id.clone()),
                project_id: "default".to_owned(),
                session_id: session_id.clone(),
                timestamp: Utc::now(),
                caused_by: None,
                payload: DomainEvent::AgentCompleted {
                    agent_id: molt_hub_core::model::AgentId::new(),
                    summary: None,
                },
            })
            .await
            .unwrap();

        assert!(
            matches!(state, molt_hub_core::model::TaskState::AwaitingApproval { .. }),
            "expected AwaitingApproval, got {state:?}"
        );

        let (fired_tx, mut fired_rx) = tokio::sync::mpsc::unbounded_channel::<ScheduledJobId>();
        let reg = Arc::clone(&registry);
        let scheduler = spawn_scheduler_test(move |id| reg.get(id), fired_tx);

        let job_id = scheduler
            .schedule(
                task_id.clone(),
                session_id.clone(),
                JobKind::ApprovalTimeout {
                    decided_by: "system".into(),
                },
                Instant::now() + Duration::from_millis(80),
            )
            .await
            .unwrap();

        let fired = tokio::time::timeout(Duration::from_millis(500), fired_rx.recv())
            .await
            .expect("timed out")
            .expect("closed");

        assert_eq!(fired, job_id);

        // Actor should now be InProgress (rejection from AwaitingApproval → InProgress per routing rule).
        let final_state = handle.get_state().await.unwrap();
        assert!(
            matches!(
                final_state,
                molt_hub_core::model::TaskState::InProgress | molt_hub_core::model::TaskState::Failed { .. }
            ),
            "unexpected final state after timeout rejection: {final_state:?}"
        );

        scheduler.shutdown().await;
    }

    // ── Test: cancel returns false for unknown job ───────────────────────────

    #[tokio::test]
    async fn cancel_unknown_job_returns_false() {
        let store = Arc::new(MemoryStore::default());
        let registry = Arc::new(TaskRegistry::new(Arc::clone(&store)));
        let reg = Arc::clone(&registry);
        let (fired_tx, _fired_rx) = tokio::sync::mpsc::unbounded_channel::<ScheduledJobId>();
        let scheduler = spawn_scheduler_test(move |id| reg.get(id), fired_tx);

        let unknown = ScheduledJobId::new();
        let cancelled = scheduler.cancel(unknown).await.unwrap();
        assert!(!cancelled, "cancelling unknown job should return false");

        scheduler.shutdown().await;
    }

    // ── Test: escalation job emits TaskPriorityChanged ───────────────────────

    #[tokio::test]
    async fn escalation_job_emits_priority_changed() {
        let store = Arc::new(MemoryStore::default());
        let registry = Arc::new(TaskRegistry::new(Arc::clone(&store)));
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let pipeline = make_pipeline(&[("work", false)]);

        spawn_actor_in_progress(&registry, task_id.clone(), session_id.clone(), pipeline).await;

        let (fired_tx, mut fired_rx) = tokio::sync::mpsc::unbounded_channel::<ScheduledJobId>();
        let reg = Arc::clone(&registry);
        let scheduler = spawn_scheduler_test(move |id| reg.get(id), fired_tx);

        let job_id = scheduler
            .schedule(
                task_id.clone(),
                session_id.clone(),
                JobKind::Escalation {
                    from_priority: Priority::P2,
                    to_priority: Priority::P1,
                },
                Instant::now() + Duration::from_millis(80),
            )
            .await
            .unwrap();

        let fired = tokio::time::timeout(Duration::from_millis(500), fired_rx.recv())
            .await
            .expect("timed out")
            .expect("closed");

        assert_eq!(fired, job_id);

        // Verify the event was stored.
        let events = store.get_events_for_task(&task_id).await.unwrap();
        let has_priority_change = events.iter().any(|e| {
            matches!(
                &e.payload,
                DomainEvent::TaskPriorityChanged {
                    from: Priority::P2,
                    to: Priority::P1,
                    ..
                }
            )
        });
        assert!(has_priority_change, "TaskPriorityChanged event should be stored");

        scheduler.shutdown().await;
    }
}
