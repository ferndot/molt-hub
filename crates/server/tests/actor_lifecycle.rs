//! Integration tests for actor lifecycle.
//!
//! These tests verify end-to-end actor lifecycle workflows:
//! - Full task lifecycle: Created → InProgress → AwaitingApproval → CompletedSuccess
//! - State watch channel broadcasts every transition
//! - Concurrent actors maintain independent state (no cross-contamination)
//! - Shutdown cleans up actors and closes their channels
//! - Invalid transitions are rejected and leave state unchanged
//! - Registry correctly tracks and retrieves actors

use std::sync::{Arc, Mutex};

use chrono::Utc;

use molt_hub_core::config::{PipelineConfig, StageDefinition};
use molt_hub_core::events::store::{EventStore, EventStoreError};
use molt_hub_core::events::types::{DomainEvent, EventEnvelope, HumanDecisionKind};
use molt_hub_core::model::{AgentId, EventId, SessionId, TaskId, TaskState};

use molt_hub_server::actors::{ActorError, TaskActorConfig, TaskActorHandle, TaskRegistry};

// ---------------------------------------------------------------------------
// Shared in-memory EventStore
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn make_store() -> Arc<MemoryEventStore> {
    Arc::new(MemoryEventStore::default())
}

fn make_registry(store: Arc<MemoryEventStore>) -> TaskRegistry<MemoryEventStore> {
    TaskRegistry::new(store)
}

fn make_config(task_id: TaskId, stage: &str, pipeline: Arc<PipelineConfig>) -> TaskActorConfig {
    TaskActorConfig {
        project_id: "default".to_owned(),
        task_id,
        session_id: SessionId::new(),
        initial_stage: stage.to_string(),
        pipeline_config: pipeline,
    }
}

fn assign_event(task_id: &TaskId, session_id: &SessionId) -> EventEnvelope {
    EventEnvelope {
        id: EventId::new(),
        task_id: task_id.clone(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
        caused_by: None,
        payload: DomainEvent::AgentAssigned {
            agent_id: AgentId::new(),
            agent_name: "test-agent".into(),
        },
    }
}

fn complete_event(task_id: &TaskId, session_id: &SessionId) -> EventEnvelope {
    EventEnvelope {
        id: EventId::new(),
        task_id: task_id.clone(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
        caused_by: None,
        payload: DomainEvent::AgentCompleted {
            agent_id: AgentId::new(),
            summary: Some("work done".into()),
        },
    }
}

fn human_approved_event(task_id: &TaskId, session_id: &SessionId) -> EventEnvelope {
    EventEnvelope {
        id: EventId::new(),
        task_id: task_id.clone(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
        caused_by: None,
        payload: DomainEvent::HumanDecision {
            decided_by: "alice".into(),
            decision: HumanDecisionKind::Approved,
            note: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Tests: full lifecycle
// ---------------------------------------------------------------------------

/// Full lifecycle without approval gate:
/// Pending → InProgress → Pending (completed, no approval needed).
#[tokio::test]
async fn full_lifecycle_without_approval() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let pipeline = make_pipeline(&[("work", false)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let config = make_config(task_id.clone(), "work", pipeline);
    let handle = registry.spawn_task(config);

    // Initial: Pending
    let initial = handle.get_state().await.unwrap();
    assert_eq!(initial, TaskState::Pending);

    // AgentAssigned → InProgress
    let state = handle.send_event(assign_event(&task_id, &session_id)).await.unwrap();
    assert_eq!(state, TaskState::InProgress);

    // AgentCompleted (no approval needed) → Completed(Success)
    // Per state machine: without requires_approval, AgentCompleted terminates the task.
    let state = handle.send_event(complete_event(&task_id, &session_id)).await.unwrap();
    assert!(
        matches!(state, TaskState::Completed { .. }),
        "expected Completed(Success) without approval gate, got {state:?}"
    );

    // All three events should be persisted.
    let events = store.get_events_for_task(&task_id).await.unwrap();
    assert_eq!(events.len(), 2, "2 events should be persisted");
}

/// Full lifecycle with approval gate:
/// Pending → InProgress → AwaitingApproval → Completed(Success) (approved).
///
/// Per state machine rules: AwaitingApproval + HumanDecision(Approved) → Completed(Success).
#[tokio::test]
async fn full_lifecycle_with_approval_gate() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let pipeline = make_pipeline(&[("review", true)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let config = make_config(task_id.clone(), "review", pipeline);
    let handle = registry.spawn_task(config);

    // Pending → InProgress
    let state = handle.send_event(assign_event(&task_id, &session_id)).await.unwrap();
    assert_eq!(state, TaskState::InProgress);

    // InProgress → AwaitingApproval (requires_approval stage)
    let state = handle.send_event(complete_event(&task_id, &session_id)).await.unwrap();
    assert!(
        matches!(state, TaskState::AwaitingApproval { .. }),
        "expected AwaitingApproval, got {state:?}"
    );

    // AwaitingApproval + HumanDecision(Approved) → Completed(Success)
    let state = handle
        .send_event(human_approved_event(&task_id, &session_id))
        .await
        .unwrap();
    assert!(
        matches!(state, TaskState::Completed { .. }),
        "expected Completed(Success) after approval, got {state:?}"
    );

    // All 3 events persisted.
    let events = store.get_events_for_task(&task_id).await.unwrap();
    assert_eq!(events.len(), 3);
}

/// Rejection path: AwaitingApproval → InProgress via rejection.
#[tokio::test]
async fn lifecycle_rejection_routes_to_in_progress() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let pipeline = make_pipeline(&[("review", true)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let config = make_config(task_id.clone(), "review", pipeline);
    let handle = registry.spawn_task(config);

    handle.send_event(assign_event(&task_id, &session_id)).await.unwrap();
    handle.send_event(complete_event(&task_id, &session_id)).await.unwrap();

    let rejected_event = EventEnvelope {
        id: EventId::new(),
        task_id: task_id.clone(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
        caused_by: None,
        payload: DomainEvent::HumanDecision {
            decided_by: "bob".into(),
            decision: HumanDecisionKind::Rejected {
                reason: "needs rework".into(),
            },
            note: None,
        },
    };

    let state = handle.send_event(rejected_event).await.unwrap();
    assert_eq!(state, TaskState::InProgress);
}

// ---------------------------------------------------------------------------
// Tests: state watch channel
// ---------------------------------------------------------------------------

/// Every state change broadcasts on the watch channel in order.
#[tokio::test]
async fn state_watch_tracks_each_transition() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let pipeline = make_pipeline(&[("review", true)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let config = make_config(task_id.clone(), "review", pipeline);
    let handle = registry.spawn_task(config);
    let mut state_rx = handle.state_rx.clone();

    // Pending → InProgress
    handle.send_event(assign_event(&task_id, &session_id)).await.unwrap();
    state_rx.changed().await.unwrap();
    {
        let update = state_rx.borrow_and_update().clone();
        assert_eq!(update.new_state, TaskState::InProgress);
        assert_eq!(update.task_id, task_id);
    }

    // InProgress → AwaitingApproval
    handle.send_event(complete_event(&task_id, &session_id)).await.unwrap();
    state_rx.changed().await.unwrap();
    {
        let update = state_rx.borrow_and_update().clone();
        assert!(
            matches!(update.new_state, TaskState::AwaitingApproval { .. }),
            "expected AwaitingApproval on watch, got {:?}",
            update.new_state
        );
    }

    // AwaitingApproval → Completed(Success) (per state machine rules)
    handle.send_event(human_approved_event(&task_id, &session_id)).await.unwrap();
    state_rx.changed().await.unwrap();
    {
        let update = state_rx.borrow_and_update().clone();
        assert!(
            matches!(update.new_state, TaskState::Completed { .. }),
            "expected Completed on watch after approval, got {:?}",
            update.new_state
        );
    }
}

/// The current_stage field on the watch update matches the actor's pipeline stage.
#[tokio::test]
async fn state_watch_includes_current_stage() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let pipeline = make_pipeline(&[("deploy", false)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let config = make_config(task_id.clone(), "deploy", pipeline);
    let handle = registry.spawn_task(config);
    let mut state_rx = handle.state_rx.clone();

    handle.send_event(assign_event(&task_id, &session_id)).await.unwrap();
    state_rx.changed().await.unwrap();
    let update = state_rx.borrow_and_update().clone();

    assert_eq!(update.current_stage, "deploy");
}

// ---------------------------------------------------------------------------
// Tests: concurrent actors
// ---------------------------------------------------------------------------

/// Three independent actors process events without cross-contamination.
#[tokio::test]
async fn concurrent_actors_maintain_independent_state() {
    let pipeline = make_pipeline(&[("work", false)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    // Spawn 3 actors.
    let mut handles: Vec<(TaskId, SessionId, TaskActorHandle)> = Vec::new();
    for _ in 0..3 {
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let config = make_config(task_id.clone(), "work", Arc::clone(&pipeline));
        let handle = registry.spawn_task(config);
        handles.push((task_id, session_id, handle));
    }

    // Send AgentAssigned only to the first actor.
    let (first_task_id, first_session_id, first_handle) = &handles[0];
    first_handle
        .send_event(assign_event(first_task_id, first_session_id))
        .await
        .unwrap();

    // First actor is InProgress.
    let first_state = first_handle.get_state().await.unwrap();
    assert_eq!(first_state, TaskState::InProgress);

    // Second and third actors are still Pending.
    let (_, _, second_handle) = &handles[1];
    let (_, _, third_handle) = &handles[2];
    assert_eq!(second_handle.get_state().await.unwrap(), TaskState::Pending);
    assert_eq!(third_handle.get_state().await.unwrap(), TaskState::Pending);
}

/// All three concurrent actors can progress independently through the full lifecycle.
#[tokio::test]
async fn concurrent_actors_all_progress_independently() {
    let pipeline = make_pipeline(&[("work", false)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let mut handles: Vec<(TaskId, SessionId, TaskActorHandle)> = Vec::new();
    for _ in 0..3 {
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let config = make_config(task_id.clone(), "work", Arc::clone(&pipeline));
        let handle = registry.spawn_task(config);
        handles.push((task_id, session_id, handle));
    }

    // Drive all actors to InProgress concurrently.
    let futures: Vec<_> = handles
        .iter()
        .map(|(task_id, session_id, handle)| {
            let envelope = assign_event(task_id, session_id);
            handle.send_event(envelope)
        })
        .collect();

    for f in futures {
        let state = f.await.unwrap();
        assert_eq!(state, TaskState::InProgress);
    }

    // Verify each actor independently stored its event.
    for (task_id, _, _) in &handles {
        let events = store.get_events_for_task(task_id).await.unwrap();
        assert_eq!(events.len(), 1, "each actor should have 1 stored event");
    }
}

// ---------------------------------------------------------------------------
// Tests: shutdown
// ---------------------------------------------------------------------------

/// Shutdown via handle: subsequent commands return ChannelClosed.
#[tokio::test]
async fn actor_shutdown_closes_channel() {
    let task_id = TaskId::new();
    let pipeline = make_pipeline(&[("work", false)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let config = make_config(task_id.clone(), "work", pipeline);
    let handle = registry.spawn_task(config);

    handle.shutdown().await.unwrap();

    // Give the actor a moment to stop.
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let result = handle.get_state().await;
    assert!(
        matches!(result, Err(ActorError::ChannelClosed(_))),
        "expected ChannelClosed after shutdown, got {result:?}"
    );
}

/// Registry shutdown_task removes the actor from the registry.
#[tokio::test]
async fn registry_shutdown_task_removes_from_registry() {
    let task_id = TaskId::new();
    let pipeline = make_pipeline(&[("work", false)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let config = make_config(task_id.clone(), "work", pipeline);
    registry.spawn_task(config);
    assert!(registry.get(&task_id).is_some());

    registry.shutdown_task(&task_id).await.unwrap();
    assert!(registry.get(&task_id).is_none());
}

/// shutdown_all clears all actors and their channels.
#[tokio::test]
async fn registry_shutdown_all_clears_all_actors() {
    let pipeline = make_pipeline(&[("work", false)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let mut task_ids = Vec::new();
    for _ in 0..3 {
        let task_id = TaskId::new();
        let config = make_config(task_id.clone(), "work", Arc::clone(&pipeline));
        registry.spawn_task(config);
        task_ids.push(task_id);
    }

    registry.shutdown_all().await;

    // All actors removed from registry.
    for task_id in &task_ids {
        assert!(
            registry.get(task_id).is_none(),
            "actor {task_id} should be removed after shutdown_all"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: invalid transitions
// ---------------------------------------------------------------------------

/// Invalid transition does not change state and does not persist an event.
#[tokio::test]
async fn invalid_transition_rejected_state_unchanged() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let pipeline = make_pipeline(&[("work", false)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let config = make_config(task_id.clone(), "work", pipeline);
    let handle = registry.spawn_task(config);

    // Pending → AgentCompleted is invalid (must be InProgress first).
    let result = handle
        .send_event(complete_event(&task_id, &session_id))
        .await;

    assert!(
        matches!(result, Err(ActorError::Transition { .. })),
        "expected Transition error, got {result:?}"
    );

    // State unchanged (still Pending).
    let state = handle.get_state().await.unwrap();
    assert_eq!(state, TaskState::Pending);

    // No events persisted.
    let events = store.get_events_for_task(&task_id).await.unwrap();
    assert_eq!(events.len(), 0, "no events should be stored for invalid transition");
}

/// Sending HumanDecision to a Pending actor (wrong state) is rejected.
#[tokio::test]
async fn human_decision_on_pending_actor_is_rejected() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let pipeline = make_pipeline(&[("review", true)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let config = make_config(task_id.clone(), "review", pipeline);
    let handle = registry.spawn_task(config);

    // Actor is Pending — HumanDecision is only valid from AwaitingApproval.
    let result = handle
        .send_event(human_approved_event(&task_id, &session_id))
        .await;

    assert!(
        matches!(result, Err(ActorError::Transition { .. })),
        "expected Transition error for HumanDecision in Pending state, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Tests: registry retrieval
// ---------------------------------------------------------------------------

/// Spawned actor is retrievable by task_id.
#[tokio::test]
async fn registry_get_returns_spawned_actor() {
    let task_id = TaskId::new();
    let pipeline = make_pipeline(&[("work", false)]);
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    assert!(registry.get(&task_id).is_none(), "should not exist before spawn");

    let config = make_config(task_id.clone(), "work", pipeline);
    registry.spawn_task(config);

    assert!(registry.get(&task_id).is_some(), "should exist after spawn");
}

/// shutdown_task on non-existent task returns ActorNotFound.
#[tokio::test]
async fn shutdown_nonexistent_task_returns_error() {
    let task_id = TaskId::new();
    let store = make_store();
    let registry = make_registry(Arc::clone(&store));

    let result = registry.shutdown_task(&task_id).await;
    assert!(
        matches!(result, Err(ActorError::ActorNotFound(_))),
        "expected ActorNotFound, got {result:?}"
    );
}
