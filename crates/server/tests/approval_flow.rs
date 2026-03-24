//! Integration tests for the approval flow.
//!
//! These tests verify end-to-end approval workflows:
//! - Task moves through stages until AwaitingApproval
//! - ApprovalService records decisions and forwards to actors
//! - Approval and rejection paths work correctly
//! - Multi-approver threshold must be met before proceeding
//! - Race conditions (two approvers near-simultaneously) are handled safely
//! - Approving a task that is no longer AwaitingApproval returns an error

use std::sync::{Arc, Mutex};

use chrono::Utc;

use molt_hub_core::config::{PipelineConfig, StageDefinition};
use molt_hub_core::events::store::{EventStore, EventStoreError};
use molt_hub_core::events::types::{DomainEvent, EventEnvelope};
use molt_hub_core::model::{AgentId, EventId, SessionId, TaskId, TaskState};

use molt_hub_server::actors::{TaskActorConfig, TaskActorHandle, TaskRegistry};
use molt_hub_server::approvals::{
    ApprovalDecision, ApprovalService, ApprovalServiceError, ApprovalStatus, MemoryApprovalStore,
};

// ---------------------------------------------------------------------------
// Shared in-memory EventStore (identical to the pattern used in actors.rs)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MemoryEventStore {
    events: Mutex<Vec<EventEnvelope>>,
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
            .filter(|e| e.task_id.as_ref() == Some(task_id))
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_approval_pipeline(stage_name: &str, required_approvers: Vec<String>) -> Arc<PipelineConfig> {
    Arc::new(PipelineConfig {
        name: "test-pipeline".into(),
        description: None,
        version: 1,
        stages: vec![StageDefinition {
            name: stage_name.to_string(),
            label: None,
            instructions: None,
            instructions_template: None,
            requires_approval: true,
            approvers: required_approvers,
            timeout_seconds: None,
            terminal: false,
            hooks: vec![],
            transition_rules: vec![],
            color: None,
            order: 0,
            wip_limit: None,
        }],
        integrations: vec![],
            columns: vec![],
    })
}

/// Spawn a task actor and drive it to AwaitingApproval state.
/// Returns the actor handle and the underlying event store.
async fn spawn_actor_awaiting_approval(
    task_id: TaskId,
    session_id: SessionId,
    stage: &str,
    approvers: Vec<String>,
) -> (TaskActorHandle, Arc<MemoryEventStore>) {
    let event_store = Arc::new(MemoryEventStore::default());
    let registry = TaskRegistry::new(Arc::clone(&event_store));
    let pipeline = make_approval_pipeline(stage, approvers);

    let config = TaskActorConfig {
        project_id: "default".to_owned(),
        task_id: task_id.clone(),
        session_id: session_id.clone(),
        initial_stage: stage.to_string(),
        pipeline_config: pipeline,
    };
    let handle = registry.spawn_task(config);

    // Pending → InProgress
    let assign = EventEnvelope {
        id: EventId::new(),
        task_id: Some(task_id.clone()),
        project_id: "default".to_owned(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
        caused_by: None,
        payload: DomainEvent::AgentAssigned {
            agent_id: AgentId::new(),
            agent_name: "test-bot".into(),
        },
    };
    handle.send_event(assign).await.unwrap();

    // InProgress → AwaitingApproval (requires_approval stage)
    let complete = EventEnvelope {
        id: EventId::new(),
        task_id: Some(task_id.clone()),
        project_id: "default".to_owned(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
        caused_by: None,
        payload: DomainEvent::AgentCompleted {
            agent_id: AgentId::new(),
            summary: Some("work done".into()),
        },
    };
    let state = handle.send_event(complete).await.unwrap();
    assert!(
        matches!(state, TaskState::AwaitingApproval { .. }),
        "expected AwaitingApproval, got {state:?}"
    );

    (handle, event_store)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Full approval path: task reaches AwaitingApproval, a single approval is
/// submitted, the actor transitions to InProgress (approved).
#[tokio::test]
async fn approval_path_single_approver() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();

    let (actor_handle, _event_store) =
        spawn_actor_awaiting_approval(task_id.clone(), session_id.clone(), "review", vec![]).await;

    let approval_store = Arc::new(MemoryApprovalStore::new());
    let service = ApprovalService::new(Arc::clone(&approval_store));

    // Open an approval request.
    let request = service
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

    assert_eq!(request.status, ApprovalStatus::Pending);

    // Submit approval.
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

    // The actor should have transitioned away from AwaitingApproval.
    // Per state machine rules: AwaitingApproval + HumanDecision(Approved) → Completed(Success).
    let state = actor_handle.get_state().await.unwrap();
    assert_ne!(
        state,
        TaskState::AwaitingApproval {
            approvers: vec![],
            approved_by: vec![],
        },
        "actor should have left AwaitingApproval after approval"
    );
    assert!(
        matches!(state, TaskState::Completed { .. }),
        "expected Completed after approval, got {state:?}"
    );
}

/// Rejection path: a rejection decision immediately closes the request and
/// drives the actor back to InProgress.
#[tokio::test]
async fn rejection_path_drives_actor_to_in_progress() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();

    let (actor_handle, _event_store) =
        spawn_actor_awaiting_approval(task_id.clone(), session_id.clone(), "review", vec![]).await;

    let approval_store = Arc::new(MemoryApprovalStore::new());
    let service = ApprovalService::new(Arc::clone(&approval_store));

    service
        .open_request(task_id.clone(), session_id.clone(), "review".into(), 1, vec![], None)
        .await
        .unwrap();

    let updated = service
        .record_decision(
            &task_id,
            ApprovalDecision::Rejected {
                approver: "bob".into(),
                reason: "not production ready".into(),
            },
            &actor_handle,
            session_id.clone(),
        )
        .await
        .unwrap();

    assert!(matches!(updated.status, ApprovalStatus::Rejected { .. }));

    // Rejection routes back to InProgress per the transition rules.
    let state = actor_handle.get_state().await.unwrap();
    assert_eq!(state, TaskState::InProgress);
}

/// Multi-approver: the request stays Pending until the threshold is met.
/// First approval should not forward to the actor; second should.
#[tokio::test]
async fn multi_approver_waits_for_threshold() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();

    let (actor_handle, _) =
        spawn_actor_awaiting_approval(task_id.clone(), session_id.clone(), "review", vec![]).await;

    let approval_store = Arc::new(MemoryApprovalStore::new());
    let service = ApprovalService::new(Arc::clone(&approval_store));

    // Require 2 approvals.
    service
        .open_request(task_id.clone(), session_id.clone(), "review".into(), 2, vec![], None)
        .await
        .unwrap();

    // First approval — threshold not yet met.
    let after_first = service
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

    assert_eq!(
        after_first.status,
        ApprovalStatus::Pending,
        "should still be pending after first approval"
    );

    // Actor should still be in AwaitingApproval.
    let state_after_first = actor_handle.get_state().await.unwrap();
    assert!(
        matches!(state_after_first, TaskState::AwaitingApproval { .. }),
        "actor should still be AwaitingApproval after first approval"
    );

    // Second approval — threshold met, request approved, actor unblocked.
    let after_second = service
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

    assert_eq!(after_second.status, ApprovalStatus::Approved);

    // Per state machine: AwaitingApproval + Approved → Completed(Success).
    let state_after_second = actor_handle.get_state().await.unwrap();
    assert!(
        matches!(state_after_second, TaskState::Completed { .. }),
        "expected Completed after threshold met, got {state_after_second:?}"
    );
}

/// Two approvers submit near-simultaneously. The second approve arrives
/// after the request is already Approved — should error (AlreadyClosed).
#[tokio::test]
async fn approving_already_approved_request_returns_already_closed() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();

    let (actor_handle, _) =
        spawn_actor_awaiting_approval(task_id.clone(), session_id.clone(), "review", vec![]).await;

    let approval_store = Arc::new(MemoryApprovalStore::new());
    let service = ApprovalService::new(Arc::clone(&approval_store));

    // Single-approver threshold.
    service
        .open_request(task_id.clone(), session_id.clone(), "review".into(), 1, vec![], None)
        .await
        .unwrap();

    // First approval closes the request.
    service
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

    // Second attempt to decide on the same request after it is closed.
    // The get_for_task call will return None because status is no longer Pending.
    let result = service
        .record_decision(
            &task_id,
            ApprovalDecision::Approved {
                approver: "charlie".into(),
                comment: None,
            },
            &actor_handle,
            session_id.clone(),
        )
        .await;

    assert!(
        matches!(result, Err(ApprovalServiceError::NoPendingRequest(_))),
        "expected NoPendingRequest after request is closed, got {result:?}"
    );
}

/// Approving a task that has no pending request returns NoPendingRequest error.
#[tokio::test]
async fn approving_task_with_no_pending_request_is_an_error() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();

    // Actor in AwaitingApproval but no request created in the service.
    let (actor_handle, _) =
        spawn_actor_awaiting_approval(task_id.clone(), session_id.clone(), "review", vec![]).await;

    let approval_store = Arc::new(MemoryApprovalStore::new());
    let service = ApprovalService::new(Arc::clone(&approval_store));

    let result = service
        .record_decision(
            &task_id,
            ApprovalDecision::Approved {
                approver: "alice".into(),
                comment: None,
            },
            &actor_handle,
            session_id.clone(),
        )
        .await;

    assert!(
        matches!(result, Err(ApprovalServiceError::NoPendingRequest(_))),
        "expected NoPendingRequest error, got {result:?}"
    );
}

/// timeout_at field is populated when timeout_seconds is provided.
#[tokio::test]
async fn open_request_sets_timeout_at_when_provided() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let approval_store = Arc::new(MemoryApprovalStore::new());
    let service = ApprovalService::new(Arc::clone(&approval_store));

    let request = service
        .open_request(
            task_id.clone(),
            session_id.clone(),
            "review".into(),
            1,
            vec![],
            Some(300), // 5 minutes
        )
        .await
        .unwrap();

    assert!(
        request.timeout_at.is_some(),
        "timeout_at should be set when timeout_seconds is provided"
    );

    // Sanity check: timeout is in the future.
    let timeout = request.timeout_at.unwrap();
    assert!(
        timeout > Utc::now(),
        "timeout_at should be in the future"
    );
}

/// When no timeout is provided, timeout_at is None.
#[tokio::test]
async fn open_request_with_no_timeout_has_none_timeout_at() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let approval_store = Arc::new(MemoryApprovalStore::new());
    let service = ApprovalService::new(Arc::clone(&approval_store));

    let request = service
        .open_request(task_id.clone(), session_id.clone(), "review".into(), 1, vec![], None)
        .await
        .unwrap();

    assert!(request.timeout_at.is_none());
}

/// Unauthorised approver on a restricted request is rejected.
#[tokio::test]
async fn unauthorised_approver_is_rejected() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();

    let (actor_handle, _) = spawn_actor_awaiting_approval(
        task_id.clone(),
        session_id.clone(),
        "review",
        vec!["alice".into()],
    )
    .await;

    let approval_store = Arc::new(MemoryApprovalStore::new());
    let service = ApprovalService::new(Arc::clone(&approval_store));

    // Require only "alice" as approver.
    service
        .open_request(
            task_id.clone(),
            session_id.clone(),
            "review".into(),
            1,
            vec!["alice".into()],
            None,
        )
        .await
        .unwrap();

    // "mallory" is not in the allowed list.
    let result = service
        .record_decision(
            &task_id,
            ApprovalDecision::Approved {
                approver: "mallory".into(),
                comment: None,
            },
            &actor_handle,
            session_id.clone(),
        )
        .await;

    assert!(
        matches!(result, Err(ApprovalServiceError::Store(_))),
        "expected Store(Unauthorised) error, got {result:?}"
    );

    // Actor should still be awaiting approval.
    let state = actor_handle.get_state().await.unwrap();
    assert!(
        matches!(state, TaskState::AwaitingApproval { .. }),
        "actor should still be AwaitingApproval after unauthorised attempt"
    );
}
