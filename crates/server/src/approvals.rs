//! Human-gated transitions — approval workflows for sensitive pipeline stages.
//!
//! This module implements the server-side mechanics for pausing task progression
//! until one or more humans make an explicit decision (approve, reject, redirect).
//!
//! # Architecture
//!
//! When a task enters `AwaitingApproval`, the [`ApprovalService`] creates an
//! [`ApprovalRequest`] in the [`ApprovalStore`]. When a human submits an
//! [`ApprovalDecision`], the service validates it against the request, updates
//! the store, and (if the threshold is met) feeds a `HumanDecision` domain event
//! into the actor system via the [`TaskRegistry`].
//!
//! Multi-approver support: an `ApprovalRequest` carries a `required_count` field.
//! The decision is only forwarded to the actor once the threshold is met.  For
//! `Rejected` or `Redirected` decisions, the first matching decision is
//! immediately forwarded (no threshold required for rejection).

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use thiserror::Error;

use molt_hub_core::events::types::{DomainEvent, EventEnvelope, HumanDecisionKind};
use molt_hub_core::model::{EventId, SessionId, TaskId};

use crate::actors::{ActorError, TaskActorHandle};

// ---------------------------------------------------------------------------
// ApprovalId
// ---------------------------------------------------------------------------

/// Unique identifier for an approval request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApprovalId(pub String);

impl ApprovalId {
    /// Generate a new unique ID using the current timestamp as a ULID.
    pub fn new() -> Self {
        Self(ulid::Ulid::new().to_string())
    }
}

impl Default for ApprovalId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ApprovalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// ApprovalDecision
// ---------------------------------------------------------------------------

/// A decision submitted by a human reviewer for an approval request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// The approver approves the task to continue.
    Approved {
        approver: String,
        comment: Option<String>,
    },
    /// The approver rejects the task; work must be revised or abandoned.
    Rejected { approver: String, reason: String },
    /// The approver redirects the task to a different stage.
    Redirected {
        approver: String,
        target_stage: String,
        reason: String,
    },
}

impl ApprovalDecision {
    /// Return the approver's identity for this decision.
    pub fn approver(&self) -> &str {
        match self {
            Self::Approved { approver, .. } => approver,
            Self::Rejected { approver, .. } => approver,
            Self::Redirected { approver, .. } => approver,
        }
    }

    /// Convert this decision into the `HumanDecisionKind` used by the domain model.
    pub fn to_domain_kind(&self) -> HumanDecisionKind {
        match self {
            Self::Approved { .. } => HumanDecisionKind::Approved,
            Self::Rejected { reason, .. } => HumanDecisionKind::Rejected {
                reason: reason.clone(),
            },
            Self::Redirected {
                target_stage,
                reason,
                ..
            } => HumanDecisionKind::Redirected {
                to_stage: target_stage.clone(),
                reason: reason.clone(),
            },
        }
    }

    /// Return a note string for the `HumanDecision` event.
    pub fn note(&self) -> Option<String> {
        match self {
            Self::Approved { comment, .. } => comment.clone(),
            Self::Rejected { reason, .. } => Some(reason.clone()),
            Self::Redirected { reason, .. } => Some(reason.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// ApprovalStatus
// ---------------------------------------------------------------------------

/// Current lifecycle state of an approval request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalStatus {
    /// Waiting for one or more approvers.
    Pending,
    /// The threshold has been met; the task may proceed.
    Approved,
    /// The request was rejected; the task goes back to InProgress.
    Rejected { reason: String },
    /// The request was redirected to a different stage.
    Redirected { target_stage: String },
}

// ---------------------------------------------------------------------------
// ApprovalRecord — a single recorded decision
// ---------------------------------------------------------------------------

/// A single decision recorded against an [`ApprovalRequest`].
#[derive(Debug, Clone)]
pub struct ApprovalRecord {
    pub approver: String,
    pub decision: ApprovalDecision,
    pub recorded_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// ApprovalRequest
// ---------------------------------------------------------------------------

/// Tracks outstanding approval requirements for a single task.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    /// Unique identifier for this request.
    pub id: ApprovalId,
    /// The task awaiting approval.
    pub task_id: TaskId,
    /// The session context when the request was created.
    pub session_id: SessionId,
    /// The pipeline stage that triggered this approval requirement.
    pub stage: String,
    /// Number of approvals required before the task can proceed.
    pub required_count: usize,
    /// Named approvers, if the stage restricts who may approve.  Empty means
    /// any authenticated user may approve.
    pub required_approvers: Vec<String>,
    /// When this request was opened.
    pub created_at: DateTime<Utc>,
    /// Optional wall-clock deadline; enforced externally (see T20).
    pub timeout_at: Option<DateTime<Utc>>,
    /// Decisions recorded so far.
    pub decisions: Vec<ApprovalRecord>,
    /// Current status of the request.
    pub status: ApprovalStatus,
}

impl ApprovalRequest {
    /// Count approvals received so far.
    pub fn approval_count(&self) -> usize {
        self.decisions
            .iter()
            .filter(|r| matches!(r.decision, ApprovalDecision::Approved { .. }))
            .count()
    }

    /// Return the names of approvers who have already submitted a decision.
    pub fn decided_by(&self) -> Vec<&str> {
        self.decisions.iter().map(|r| r.approver.as_str()).collect()
    }

    /// Return `true` if this approver has already recorded a decision.
    pub fn has_decided(&self, approver: &str) -> bool {
        self.decisions.iter().any(|r| r.approver == approver)
    }

    /// Return `true` if the approval threshold has been reached.
    pub fn threshold_met(&self) -> bool {
        self.approval_count() >= self.required_count
    }
}

// ---------------------------------------------------------------------------
// ApprovalStore trait
// ---------------------------------------------------------------------------

/// Errors returned by [`ApprovalStore`] operations.
#[derive(Debug, Error)]
pub enum ApprovalStoreError {
    /// No request found for the given ID.
    #[error("approval request {0} not found")]
    NotFound(ApprovalId),

    /// A request for this task already exists.
    #[error("approval request already exists for task {0}")]
    AlreadyExists(TaskId),

    /// An approver attempted to record a second decision.
    #[error("approver '{approver}' has already decided on request {id}")]
    AlreadyDecided { approver: String, id: ApprovalId },

    /// The request is no longer pending.
    #[error("request {0} is no longer pending")]
    AlreadyClosed(ApprovalId),

    /// The approver is not in the allowed approvers list.
    #[error("approver '{0}' is not authorised for this request")]
    Unauthorised(String),

    /// Generic storage error.
    #[error("storage error: {0}")]
    Storage(String),
}

/// Persistence interface for approval requests.
pub trait ApprovalStore: Send + Sync + 'static {
    /// Persist a new approval request.
    fn create(
        &self,
        request: ApprovalRequest,
    ) -> impl std::future::Future<Output = Result<(), ApprovalStoreError>> + Send;

    /// Retrieve a request by its ID.
    fn get(
        &self,
        id: &ApprovalId,
    ) -> impl std::future::Future<Output = Result<ApprovalRequest, ApprovalStoreError>> + Send;

    /// Retrieve the pending request for a task, if one exists.
    fn get_for_task(
        &self,
        task_id: &TaskId,
    ) -> impl std::future::Future<Output = Result<Option<ApprovalRequest>, ApprovalStoreError>> + Send;

    /// Return all requests that are currently in `Pending` status.
    fn list_pending(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<ApprovalRequest>, ApprovalStoreError>> + Send;

    /// Record a decision against the request with the given ID and update its
    /// status if the decision is terminal (reject / redirect) or if the
    /// approval threshold has been met.
    ///
    /// Returns the updated [`ApprovalRequest`].
    fn record_decision(
        &self,
        id: &ApprovalId,
        decision: ApprovalDecision,
    ) -> impl std::future::Future<Output = Result<ApprovalRequest, ApprovalStoreError>> + Send;
}

// ---------------------------------------------------------------------------
// MemoryApprovalStore — in-process implementation
// ---------------------------------------------------------------------------

/// In-memory [`ApprovalStore`] for tests and early development.
///
/// Uses `std::sync::Mutex` (not `tokio::sync::Mutex`) consistent with the
/// pattern established in `actors.rs` for `MemoryStore`.
#[derive(Default)]
pub struct MemoryApprovalStore {
    requests: Mutex<Vec<ApprovalRequest>>,
}

impl MemoryApprovalStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ApprovalStore for MemoryApprovalStore {
    async fn create(&self, request: ApprovalRequest) -> Result<(), ApprovalStoreError> {
        let mut requests = self.requests.lock().unwrap();

        // Guard: no duplicate open requests for the same task.
        if requests
            .iter()
            .any(|r| r.task_id == request.task_id && r.status == ApprovalStatus::Pending)
        {
            return Err(ApprovalStoreError::AlreadyExists(request.task_id.clone()));
        }

        requests.push(request);
        Ok(())
    }

    async fn get(&self, id: &ApprovalId) -> Result<ApprovalRequest, ApprovalStoreError> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .find(|r| &r.id == id)
            .cloned()
            .ok_or_else(|| ApprovalStoreError::NotFound(id.clone()))
    }

    async fn get_for_task(
        &self,
        task_id: &TaskId,
    ) -> Result<Option<ApprovalRequest>, ApprovalStoreError> {
        Ok(self
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|r| &r.task_id == task_id && r.status == ApprovalStatus::Pending)
            .cloned())
    }

    async fn list_pending(&self) -> Result<Vec<ApprovalRequest>, ApprovalStoreError> {
        Ok(self
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.status == ApprovalStatus::Pending)
            .cloned()
            .collect())
    }

    async fn record_decision(
        &self,
        id: &ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<ApprovalRequest, ApprovalStoreError> {
        let mut requests = self.requests.lock().unwrap();

        let request = requests
            .iter_mut()
            .find(|r| &r.id == id)
            .ok_or_else(|| ApprovalStoreError::NotFound(id.clone()))?;

        // Guard: must be pending.
        if request.status != ApprovalStatus::Pending {
            return Err(ApprovalStoreError::AlreadyClosed(id.clone()));
        }

        // Guard: approver must not have already decided.
        if request.has_decided(decision.approver()) {
            return Err(ApprovalStoreError::AlreadyDecided {
                approver: decision.approver().to_string(),
                id: id.clone(),
            });
        }

        // Guard: if named approvers are specified, enforce the list.
        if !request.required_approvers.is_empty()
            && !request
                .required_approvers
                .iter()
                .any(|a| a == decision.approver())
        {
            return Err(ApprovalStoreError::Unauthorised(
                decision.approver().to_string(),
            ));
        }

        // Record the decision.
        let record = ApprovalRecord {
            approver: decision.approver().to_string(),
            decision: decision.clone(),
            recorded_at: Utc::now(),
        };
        request.decisions.push(record);

        // Update the request status based on the decision type.
        match &decision {
            ApprovalDecision::Rejected { reason, .. } => {
                request.status = ApprovalStatus::Rejected {
                    reason: reason.clone(),
                };
            }
            ApprovalDecision::Redirected { target_stage, .. } => {
                request.status = ApprovalStatus::Redirected {
                    target_stage: target_stage.clone(),
                };
            }
            ApprovalDecision::Approved { .. } => {
                if request.threshold_met() {
                    request.status = ApprovalStatus::Approved;
                }
                // If threshold not yet met, status stays Pending.
            }
        }

        Ok(request.clone())
    }
}

// ---------------------------------------------------------------------------
// ApprovalServiceError
// ---------------------------------------------------------------------------

/// Errors returned by the [`ApprovalService`].
#[derive(Debug, Error)]
pub enum ApprovalServiceError {
    /// An error occurred in the approval store.
    #[error("approval store error: {0}")]
    Store(#[from] ApprovalStoreError),

    /// An error occurred sending the decision event to the actor.
    #[error("actor error: {0}")]
    Actor(#[from] ActorError),

    /// No pending approval request exists for the task.
    #[error("no pending approval request for task {0}")]
    NoPendingRequest(TaskId),

    /// The request is pending but the threshold has not yet been met.
    #[error("approval threshold not yet met for request {0}")]
    ThresholdNotMet(ApprovalId),
}

// ---------------------------------------------------------------------------
// ApprovalService
// ---------------------------------------------------------------------------

/// Coordinates approval lifecycle: creates requests, records decisions,
/// and forwards completed decisions to the actor system.
pub struct ApprovalService<S: ApprovalStore> {
    store: Arc<S>,
}

impl<S: ApprovalStore> ApprovalService<S> {
    /// Create a new service backed by the given store.
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    /// Open an approval request for a task that has just entered `AwaitingApproval`.
    ///
    /// `required_count` controls the multi-approver threshold.
    /// `required_approvers` is the list of named approvers (empty = any).
    /// `timeout_seconds` optionally sets the `timeout_at` field.
    pub async fn open_request(
        &self,
        task_id: TaskId,
        session_id: SessionId,
        stage: String,
        required_count: usize,
        required_approvers: Vec<String>,
        timeout_seconds: Option<u64>,
    ) -> Result<ApprovalRequest, ApprovalServiceError> {
        let timeout_at =
            timeout_seconds.map(|secs| Utc::now() + chrono::Duration::seconds(secs as i64));

        let request = ApprovalRequest {
            id: ApprovalId::new(),
            task_id,
            session_id,
            stage,
            required_count: required_count.max(1),
            required_approvers,
            created_at: Utc::now(),
            timeout_at,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };

        self.store.create(request.clone()).await?;
        Ok(request)
    }

    /// Record a human decision and, if the request is now resolved, emit the
    /// appropriate `HumanDecision` domain event via the actor handle.
    ///
    /// Returns the updated [`ApprovalRequest`].  The caller can inspect
    /// `request.status` to see whether the actor received an event.
    pub async fn record_decision(
        &self,
        task_id: &TaskId,
        decision: ApprovalDecision,
        actor_handle: &TaskActorHandle,
        session_id: SessionId,
    ) -> Result<ApprovalRequest, ApprovalServiceError> {
        // Find the pending request for this task.
        let request = self
            .store
            .get_for_task(task_id)
            .await?
            .ok_or_else(|| ApprovalServiceError::NoPendingRequest(task_id.clone()))?;

        // Record the decision in the store.
        let updated = self.store.record_decision(&request.id, decision).await?;

        // If the request is now resolved (approved threshold met, rejected, or
        // redirected), forward the decision to the actor.
        match &updated.status {
            ApprovalStatus::Pending => {
                // Threshold not yet met.  Nothing to do.
            }
            ApprovalStatus::Approved
            | ApprovalStatus::Rejected { .. }
            | ApprovalStatus::Redirected { .. } => {
                // Construct the HumanDecision domain event from the last decision.
                let last = updated
                    .decisions
                    .last()
                    .expect("at least one decision was recorded");

                let payload = DomainEvent::HumanDecision {
                    decided_by: last.approver.clone(),
                    decision: last.decision.to_domain_kind(),
                    note: last.decision.note(),
                };

                let envelope = EventEnvelope {
                    id: EventId::new(),
                    task_id: task_id.clone(),
                    session_id,
                    timestamp: Utc::now(),
                    caused_by: None,
                    payload,
                };

                actor_handle.send_event(envelope).await?;
            }
        }

        Ok(updated)
    }

    /// Return the approval store for direct inspection (useful for tests).
    pub fn store(&self) -> &Arc<S> {
        &self.store
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    use molt_hub_core::config::{PipelineConfig, StageDefinition};
    use molt_hub_core::events::store::{EventStore, EventStoreError};
    use molt_hub_core::events::types::EventEnvelope;
    use molt_hub_core::model::{AgentId, EventId, SessionId, TaskId, TaskState};

    use crate::actors::{TaskActorConfig, TaskRegistry};

    // ── Event store stub (same pattern as actors.rs) ─────────────────────────

    #[derive(Default)]
    struct MemoryEventStore {
        events: StdMutex<Vec<EventEnvelope>>,
    }

    impl EventStore for MemoryEventStore {
        async fn append(&self, envelope: EventEnvelope) -> Result<(), EventStoreError> {
            self.events.lock().unwrap().push(envelope);
            Ok(())
        }

        async fn append_batch(&self, envelopes: Vec<EventEnvelope>) -> Result<(), EventStoreError> {
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
                .filter(|e| &e.task_id == task_id)
                .cloned()
                .collect())
        }

        async fn get_events_since(
            &self,
            since: chrono::DateTime<Utc>,
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
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn make_pipeline_with_approval(stage_name: &str) -> Arc<PipelineConfig> {
        Arc::new(PipelineConfig {
            name: "test".into(),
            description: None,
            version: 1,
            stages: vec![StageDefinition {
                name: stage_name.to_string(),
                instructions: None,
                instructions_template: None,
                requires_approval: true,
                approvers: vec![],
                timeout_seconds: None,
                terminal: false,
                hooks: vec![],
                transition_rules: vec![],
            }],
            integrations: vec![],
            columns: vec![],
        })
    }

    fn make_approval_store() -> Arc<MemoryApprovalStore> {
        Arc::new(MemoryApprovalStore::new())
    }

    /// Spin up a task actor that is in the InProgress state, ready for
    /// `AgentCompleted` + `HumanDecision` events.
    async fn spawn_awaiting_approval_actor(
        task_id: TaskId,
        session_id: SessionId,
    ) -> (TaskActorHandle, Arc<MemoryEventStore>) {
        let event_store = Arc::new(MemoryEventStore::default());
        let registry = TaskRegistry::new(Arc::clone(&event_store));
        let pipeline = make_pipeline_with_approval("review");

        let config = TaskActorConfig {
            task_id: task_id.clone(),
            session_id: session_id.clone(),
            initial_stage: "review".to_string(),
            pipeline_config: pipeline,
        };
        let handle = registry.spawn_task(config);

        // Drive to InProgress.
        let assign_event = EventEnvelope {
            id: EventId::new(),
            task_id: task_id.clone(),
            session_id: session_id.clone(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::AgentAssigned {
                agent_id: AgentId::new(),
                agent_name: "bot".into(),
            },
        };
        handle.send_event(assign_event).await.unwrap();

        // Drive to AwaitingApproval (requires_approval stage).
        let complete_event = EventEnvelope {
            id: EventId::new(),
            task_id: task_id.clone(),
            session_id: session_id.clone(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::AgentCompleted {
                agent_id: AgentId::new(),
                summary: None,
            },
        };
        let state = handle.send_event(complete_event).await.unwrap();
        assert!(
            matches!(state, TaskState::AwaitingApproval { .. }),
            "expected AwaitingApproval, got {state:?}"
        );

        (handle, event_store)
    }

    // ── Unit tests: ApprovalRequest ───────────────────────────────────────────

    #[test]
    fn approval_request_counts_approvals() {
        let task_id = TaskId::new();
        let mut req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: task_id.clone(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 2,
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };

        assert_eq!(req.approval_count(), 0);
        assert!(!req.threshold_met());

        req.decisions.push(ApprovalRecord {
            approver: "alice".into(),
            decision: ApprovalDecision::Approved {
                approver: "alice".into(),
                comment: None,
            },
            recorded_at: Utc::now(),
        });

        assert_eq!(req.approval_count(), 1);
        assert!(!req.threshold_met());

        req.decisions.push(ApprovalRecord {
            approver: "bob".into(),
            decision: ApprovalDecision::Approved {
                approver: "bob".into(),
                comment: None,
            },
            recorded_at: Utc::now(),
        });

        assert_eq!(req.approval_count(), 2);
        assert!(req.threshold_met());
    }

    #[test]
    fn has_decided_detects_prior_approver() {
        let mut req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 1,
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };

        assert!(!req.has_decided("alice"));

        req.decisions.push(ApprovalRecord {
            approver: "alice".into(),
            decision: ApprovalDecision::Approved {
                approver: "alice".into(),
                comment: None,
            },
            recorded_at: Utc::now(),
        });

        assert!(req.has_decided("alice"));
        assert!(!req.has_decided("bob"));
    }

    // ── Unit tests: MemoryApprovalStore ───────────────────────────────────────

    #[tokio::test]
    async fn store_create_and_get() {
        let store = make_approval_store();
        let task_id = TaskId::new();

        let req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: task_id.clone(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 1,
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };
        let id = req.id.clone();

        store.create(req).await.unwrap();
        let fetched = store.get(&id).await.unwrap();
        assert_eq!(fetched.task_id, task_id);
    }

    #[tokio::test]
    async fn store_rejects_duplicate_for_same_task() {
        let store = make_approval_store();
        let task_id = TaskId::new();

        let make_req = || ApprovalRequest {
            id: ApprovalId::new(),
            task_id: task_id.clone(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 1,
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };

        store.create(make_req()).await.unwrap();
        let err = store.create(make_req()).await.unwrap_err();
        assert!(
            matches!(err, ApprovalStoreError::AlreadyExists(_)),
            "expected AlreadyExists, got {err:?}"
        );
    }

    #[tokio::test]
    async fn store_list_pending_returns_only_pending() {
        let store = make_approval_store();

        // Create two pending requests for different tasks.
        for _ in 0..2 {
            let req = ApprovalRequest {
                id: ApprovalId::new(),
                task_id: TaskId::new(),
                session_id: SessionId::new(),
                stage: "review".into(),
                required_count: 1,
                required_approvers: vec![],
                created_at: Utc::now(),
                timeout_at: None,
                decisions: vec![],
                status: ApprovalStatus::Pending,
            };
            store.create(req).await.unwrap();
        }

        let pending = store.list_pending().await.unwrap();
        assert_eq!(pending.len(), 2);
    }

    #[tokio::test]
    async fn store_record_approval_updates_status_when_threshold_met() {
        let store = make_approval_store();
        let req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 1,
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };
        let id = req.id.clone();
        store.create(req).await.unwrap();

        let updated = store
            .record_decision(
                &id,
                ApprovalDecision::Approved {
                    approver: "alice".into(),
                    comment: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.status, ApprovalStatus::Approved);
    }

    #[tokio::test]
    async fn store_record_approval_keeps_pending_until_threshold() {
        let store = make_approval_store();
        let req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 2, // Need 2 approvals
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };
        let id = req.id.clone();
        store.create(req).await.unwrap();

        // First approval — threshold not yet met.
        let updated = store
            .record_decision(
                &id,
                ApprovalDecision::Approved {
                    approver: "alice".into(),
                    comment: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.status, ApprovalStatus::Pending);
        assert_eq!(updated.approval_count(), 1);

        // Second approval — threshold met.
        let updated = store
            .record_decision(
                &id,
                ApprovalDecision::Approved {
                    approver: "bob".into(),
                    comment: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.status, ApprovalStatus::Approved);
        assert_eq!(updated.approval_count(), 2);
    }

    #[tokio::test]
    async fn store_record_rejection_closes_immediately() {
        let store = make_approval_store();
        let req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 3,
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };
        let id = req.id.clone();
        store.create(req).await.unwrap();

        let updated = store
            .record_decision(
                &id,
                ApprovalDecision::Rejected {
                    approver: "alice".into(),
                    reason: "not ready".into(),
                },
            )
            .await
            .unwrap();

        assert!(
            matches!(updated.status, ApprovalStatus::Rejected { .. }),
            "expected Rejected, got {:?}",
            updated.status
        );
    }

    #[tokio::test]
    async fn store_record_redirected_closes_immediately() {
        let store = make_approval_store();
        let req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 2,
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };
        let id = req.id.clone();
        store.create(req).await.unwrap();

        let updated = store
            .record_decision(
                &id,
                ApprovalDecision::Redirected {
                    approver: "alice".into(),
                    target_stage: "impl".into(),
                    reason: "needs rework".into(),
                },
            )
            .await
            .unwrap();

        assert!(
            matches!(
                updated.status,
                ApprovalStatus::Redirected { ref target_stage } if target_stage == "impl"
            ),
            "expected Redirected(impl), got {:?}",
            updated.status
        );
    }

    #[tokio::test]
    async fn store_rejects_duplicate_decision_from_same_approver() {
        let store = make_approval_store();
        let req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 2,
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };
        let id = req.id.clone();
        store.create(req).await.unwrap();

        store
            .record_decision(
                &id,
                ApprovalDecision::Approved {
                    approver: "alice".into(),
                    comment: None,
                },
            )
            .await
            .unwrap();

        let err = store
            .record_decision(
                &id,
                ApprovalDecision::Approved {
                    approver: "alice".into(),
                    comment: Some("changed my mind".into()),
                },
            )
            .await
            .unwrap_err();

        assert!(
            matches!(err, ApprovalStoreError::AlreadyDecided { .. }),
            "expected AlreadyDecided, got {err:?}"
        );
    }

    #[tokio::test]
    async fn store_rejects_decision_on_closed_request() {
        let store = make_approval_store();
        let req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 1,
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };
        let id = req.id.clone();
        store.create(req).await.unwrap();

        // Close it.
        store
            .record_decision(
                &id,
                ApprovalDecision::Approved {
                    approver: "alice".into(),
                    comment: None,
                },
            )
            .await
            .unwrap();

        // Second decision should fail.
        let err = store
            .record_decision(
                &id,
                ApprovalDecision::Approved {
                    approver: "bob".into(),
                    comment: None,
                },
            )
            .await
            .unwrap_err();

        assert!(
            matches!(err, ApprovalStoreError::AlreadyClosed(_)),
            "expected AlreadyClosed, got {err:?}"
        );
    }

    #[tokio::test]
    async fn store_enforces_named_approvers_list() {
        let store = make_approval_store();
        let req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 1,
            required_approvers: vec!["alice".into(), "bob".into()],
            created_at: Utc::now(),
            timeout_at: None,
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };
        let id = req.id.clone();
        store.create(req).await.unwrap();

        // Unauthorised approver.
        let err = store
            .record_decision(
                &id,
                ApprovalDecision::Approved {
                    approver: "charlie".into(),
                    comment: None,
                },
            )
            .await
            .unwrap_err();

        assert!(
            matches!(err, ApprovalStoreError::Unauthorised(_)),
            "expected Unauthorised, got {err:?}"
        );

        // Authorised approver.
        store
            .record_decision(
                &id,
                ApprovalDecision::Approved {
                    approver: "alice".into(),
                    comment: None,
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn store_timeout_field_populated() {
        let store = make_approval_store();
        let timeout_at = Utc::now() + chrono::Duration::seconds(3600);
        let req = ApprovalRequest {
            id: ApprovalId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            stage: "review".into(),
            required_count: 1,
            required_approvers: vec![],
            created_at: Utc::now(),
            timeout_at: Some(timeout_at),
            decisions: vec![],
            status: ApprovalStatus::Pending,
        };
        let id = req.id.clone();
        store.create(req).await.unwrap();

        let fetched = store.get(&id).await.unwrap();
        assert!(fetched.timeout_at.is_some());
    }

    // ── Integration tests: ApprovalService + actor ────────────────────────────

    #[tokio::test]
    async fn service_open_request_creates_pending_entry() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let approval_store = make_approval_store();
        let service = ApprovalService::new(Arc::clone(&approval_store));

        service
            .open_request(
                task_id.clone(),
                session_id.clone(),
                "review".into(),
                1,
                vec![],
                None,
            )
            .await
            .unwrap();

        let pending = approval_store.list_pending().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].task_id, task_id);
    }

    #[tokio::test]
    async fn service_approval_decision_forwards_event_to_actor() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();

        // Set up actor in AwaitingApproval state.
        let (actor_handle, event_store) =
            spawn_awaiting_approval_actor(task_id.clone(), session_id.clone()).await;

        // Set up approval service.
        let approval_store = make_approval_store();
        let service = ApprovalService::new(Arc::clone(&approval_store));

        service
            .open_request(
                task_id.clone(),
                session_id.clone(),
                "review".into(),
                1,
                vec![],
                None,
            )
            .await
            .unwrap();

        // Submit an approval decision.
        let updated = service
            .record_decision(
                &task_id,
                ApprovalDecision::Approved {
                    approver: "alice".into(),
                    comment: Some("LGTM".into()),
                },
                &actor_handle,
                session_id.clone(),
            )
            .await
            .unwrap();

        assert_eq!(updated.status, ApprovalStatus::Approved);

        // The actor should now be in Completed state.
        let state = actor_handle.get_state().await.unwrap();
        assert!(
            matches!(state, TaskState::Completed { .. }),
            "expected Completed, got {state:?}"
        );

        // The HumanDecision event should be in the event store (3 total: assign + complete + human).
        let events = event_store.get_events_for_task(&task_id).await.unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(
            events[2].payload,
            DomainEvent::HumanDecision { .. }
        ));
    }

    #[tokio::test]
    async fn service_rejection_forwards_event_to_actor() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();

        let (actor_handle, _event_store) =
            spawn_awaiting_approval_actor(task_id.clone(), session_id.clone()).await;

        let approval_store = make_approval_store();
        let service = ApprovalService::new(Arc::clone(&approval_store));

        service
            .open_request(
                task_id.clone(),
                session_id.clone(),
                "review".into(),
                1,
                vec![],
                None,
            )
            .await
            .unwrap();

        let updated = service
            .record_decision(
                &task_id,
                ApprovalDecision::Rejected {
                    approver: "alice".into(),
                    reason: "not ready".into(),
                },
                &actor_handle,
                session_id.clone(),
            )
            .await
            .unwrap();

        assert!(matches!(updated.status, ApprovalStatus::Rejected { .. }));

        // Rejected → InProgress in the state machine.
        let state = actor_handle.get_state().await.unwrap();
        assert_eq!(state, TaskState::InProgress);
    }

    #[tokio::test]
    async fn service_redirect_forwards_event_to_actor() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();

        let (actor_handle, _) =
            spawn_awaiting_approval_actor(task_id.clone(), session_id.clone()).await;

        let approval_store = make_approval_store();
        let service = ApprovalService::new(Arc::clone(&approval_store));

        service
            .open_request(
                task_id.clone(),
                session_id.clone(),
                "review".into(),
                1,
                vec![],
                None,
            )
            .await
            .unwrap();

        service
            .record_decision(
                &task_id,
                ApprovalDecision::Redirected {
                    approver: "alice".into(),
                    target_stage: "impl".into(),
                    reason: "needs rework".into(),
                },
                &actor_handle,
                session_id.clone(),
            )
            .await
            .unwrap();

        // Redirected → InProgress in the state machine.
        let state = actor_handle.get_state().await.unwrap();
        assert_eq!(state, TaskState::InProgress);
    }

    #[tokio::test]
    async fn service_multi_approver_only_forwards_when_threshold_met() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();

        let (actor_handle, _) =
            spawn_awaiting_approval_actor(task_id.clone(), session_id.clone()).await;

        let approval_store = make_approval_store();
        let service = ApprovalService::new(Arc::clone(&approval_store));

        // Require 2 of 3 approvers.
        service
            .open_request(
                task_id.clone(),
                session_id.clone(),
                "review".into(),
                2,
                vec![],
                None,
            )
            .await
            .unwrap();

        // First approval — no event emitted yet.
        let updated = service
            .record_decision(
                &task_id,
                ApprovalDecision::Approved {
                    approver: "alice".into(),
                    comment: None,
                },
                &actor_handle,
                session_id.clone(),
            )
            .await
            .unwrap();

        assert_eq!(updated.status, ApprovalStatus::Pending);

        // Actor should still be AwaitingApproval.
        let state = actor_handle.get_state().await.unwrap();
        assert!(
            matches!(state, TaskState::AwaitingApproval { .. }),
            "expected AwaitingApproval after first approval, got {state:?}"
        );

        // Second approval — threshold met, event forwarded.
        let updated = service
            .record_decision(
                &task_id,
                ApprovalDecision::Approved {
                    approver: "bob".into(),
                    comment: None,
                },
                &actor_handle,
                session_id.clone(),
            )
            .await
            .unwrap();

        assert_eq!(updated.status, ApprovalStatus::Approved);

        let state = actor_handle.get_state().await.unwrap();
        assert!(
            matches!(state, TaskState::Completed { .. }),
            "expected Completed after threshold met, got {state:?}"
        );
    }

    #[tokio::test]
    async fn service_no_pending_request_returns_error() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let (actor_handle, _) =
            spawn_awaiting_approval_actor(task_id.clone(), session_id.clone()).await;

        let approval_store = make_approval_store();
        let service = ApprovalService::new(Arc::clone(&approval_store));

        // No request was opened — should get NoPendingRequest.
        let err = service
            .record_decision(
                &task_id,
                ApprovalDecision::Approved {
                    approver: "alice".into(),
                    comment: None,
                },
                &actor_handle,
                session_id.clone(),
            )
            .await
            .unwrap_err();

        assert!(
            matches!(err, ApprovalServiceError::NoPendingRequest(_)),
            "expected NoPendingRequest, got {err:?}"
        );
    }

    #[tokio::test]
    async fn service_open_request_with_timeout_populates_timeout_at() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let approval_store = make_approval_store();
        let service = ApprovalService::new(Arc::clone(&approval_store));

        let req = service
            .open_request(
                task_id.clone(),
                session_id.clone(),
                "review".into(),
                1,
                vec![],
                Some(3600), // 1 hour timeout
            )
            .await
            .unwrap();

        assert!(req.timeout_at.is_some());
        let timeout_at = req.timeout_at.unwrap();
        assert!(timeout_at > Utc::now());
    }
}
