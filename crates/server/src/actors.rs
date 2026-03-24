//! Task concurrency — actor model for managing parallel agent execution.
//!
//! Each active task gets its own `TaskActor` driven by a tokio task. The actor
//! owns a `TaskMachine` and processes `TaskCommand` messages sent over an mpsc
//! channel. A `TaskRegistry` manages the mapping from `TaskId` to actor handles
//! using DashMap for concurrent access.

use std::sync::Arc;

use dashmap::DashMap;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, error, info, warn};

use molt_hub_core::config::PipelineConfig;
use molt_hub_core::events::store::EventStore;
use molt_hub_core::events::types::{DomainEvent, EventEnvelope};
use molt_hub_core::machine::{TaskMachine, TransitionError};
use molt_hub_core::model::{SessionId, TaskId, TaskState};

use crate::ws::ConnectionManager;
use crate::ws_broadcast::{
    broadcast_agent_output, broadcast_board_update, broadcast_triage_new,
    broadcast_triage_resolved, TriageItemPayload,
};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur when interacting with task actors or the registry.
#[derive(Debug, Error)]
pub enum ActorError {
    /// The target actor could not be found in the registry.
    #[error("no actor found for task {0}")]
    ActorNotFound(TaskId),

    /// Sending a command to the actor's channel failed (actor is stopped).
    #[error("actor channel closed for task {0}")]
    ChannelClosed(TaskId),

    /// The oneshot reply channel was dropped before a response arrived.
    #[error("reply channel dropped for task {0}")]
    ReplyDropped(TaskId),

    /// The underlying event store returned an error.
    #[error("event store error: {0}")]
    EventStore(#[from] molt_hub_core::events::store::EventStoreError),

    /// The state machine rejected the event.
    #[error("transition error for task {task_id}: {source}")]
    Transition {
        task_id: TaskId,
        source: TransitionError,
    },
}

// ---------------------------------------------------------------------------
// TaskCommand — messages sent TO an actor
// ---------------------------------------------------------------------------

/// Commands that can be sent to a running `TaskActor`.
pub enum TaskCommand {
    /// Apply a domain event to the actor's state machine.
    ApplyEvent {
        envelope: EventEnvelope,
        /// Channel to receive the result (new state or error).
        reply: oneshot::Sender<Result<TaskState, ActorError>>,
    },
    /// Query the actor's current state without modifying it.
    GetState {
        reply: oneshot::Sender<TaskState>,
    },
    /// Ask the actor to shut down gracefully.
    Shutdown,
}

// ---------------------------------------------------------------------------
// StateUpdate — broadcast outward when state changes
// ---------------------------------------------------------------------------

/// Sent on the watch channel each time the actor's state changes.
#[derive(Debug, Clone)]
pub struct StateUpdate {
    pub task_id: TaskId,
    pub new_state: TaskState,
    pub current_stage: String,
}

// ---------------------------------------------------------------------------
// TaskActor — the per-task async loop
// ---------------------------------------------------------------------------

/// Configuration passed when spawning a new actor.
pub struct TaskActorConfig {
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub initial_stage: String,
    pub pipeline_config: Arc<PipelineConfig>,
}

/// Runs a per-task event loop, owning a `TaskMachine` and responding to commands.
struct TaskActor<S: EventStore + 'static> {
    task_id: TaskId,
    #[allow(dead_code)] // will be used when constructing outbound event envelopes
    session_id: SessionId,
    machine: TaskMachine,
    pipeline_config: Arc<PipelineConfig>,
    store: Arc<S>,
    rx: mpsc::Receiver<TaskCommand>,
    state_tx: watch::Sender<StateUpdate>,
    /// Optional WebSocket connection manager for broadcasting events to UI clients.
    ws_manager: Option<Arc<ConnectionManager>>,
}

impl<S: EventStore + 'static> TaskActor<S> {
    fn new(
        config: TaskActorConfig,
        store: Arc<S>,
        rx: mpsc::Receiver<TaskCommand>,
        state_tx: watch::Sender<StateUpdate>,
        ws_manager: Option<Arc<ConnectionManager>>,
    ) -> Self {
        let machine = TaskMachine::new(config.initial_stage.clone());
        Self {
            task_id: config.task_id,
            session_id: config.session_id,
            machine,
            pipeline_config: config.pipeline_config,
            store,
            rx,
            state_tx,
            ws_manager,
        }
    }

    /// Look up the `requires_approval` flag for the machine's current stage.
    fn requires_approval_for_current_stage(&self) -> bool {
        self.pipeline_config
            .stages
            .iter()
            .find(|s| s.name == self.machine.current_stage)
            .map(|s| s.requires_approval)
            .unwrap_or(false)
    }

    /// Run the actor event loop until a `Shutdown` command is received or the
    /// channel is closed.
    async fn run(mut self) {
        info!(task_id = %self.task_id, "actor started");

        while let Some(cmd) = self.rx.recv().await {
            match cmd {
                TaskCommand::ApplyEvent { envelope, reply } => {
                    let result = self.handle_apply_event(envelope).await;
                    let _ = reply.send(result);
                }
                TaskCommand::GetState { reply } => {
                    let _ = reply.send(self.machine.state.clone());
                }
                TaskCommand::Shutdown => {
                    info!(task_id = %self.task_id, "actor shutting down on request");
                    break;
                }
            }
        }

        info!(task_id = %self.task_id, "actor stopped");
    }

    /// Handle an `ApplyEvent` command: apply to the state machine, persist,
    /// and broadcast the new state via both the watch channel and WebSocket.
    async fn handle_apply_event(
        &mut self,
        envelope: EventEnvelope,
    ) -> Result<TaskState, ActorError> {
        let requires_approval = self.requires_approval_for_current_stage();
        let event_payload = envelope.payload.clone();

        let new_state = self
            .machine
            .apply_with_approval_flag(&envelope.payload, requires_approval)
            .map_err(|e| ActorError::Transition {
                task_id: self.task_id.clone(),
                source: e,
            })?;

        debug!(
            task_id = %self.task_id,
            ?new_state,
            "state machine transitioned"
        );

        // Persist event to the store.
        self.store
            .append(envelope)
            .await
            .map_err(ActorError::EventStore)?;

        // Broadcast the state update to any watchers (internal watch channel).
        let update = StateUpdate {
            task_id: self.task_id.clone(),
            new_state: new_state.clone(),
            current_stage: self.machine.current_stage.clone(),
        };
        if self.state_tx.send(update).is_err() {
            warn!(task_id = %self.task_id, "no watchers on state channel");
        }

        // Broadcast to WebSocket clients if a connection manager is available.
        if let Some(ref mgr) = self.ws_manager {
            self.broadcast_ws_events(mgr, &event_payload, &new_state);
        }

        Ok(new_state)
    }

    /// Broadcast appropriate WebSocket events based on the domain event and new state.
    fn broadcast_ws_events(
        &self,
        mgr: &ConnectionManager,
        event: &DomainEvent,
        new_state: &TaskState,
    ) {
        let task_id_str = self.task_id.to_string();
        let stage = &self.machine.current_stage;

        // Map TaskState to a board status string.
        let status = match new_state {
            TaskState::Pending => "waiting",
            TaskState::InProgress => "running",
            TaskState::Blocked { .. } => "blocked",
            TaskState::AwaitingApproval { .. } => "waiting",
            TaskState::Completed { .. } => "complete",
            TaskState::Failed { .. } => "blocked",
        };

        // Board update — always broadcast state changes.
        broadcast_board_update(mgr, &task_id_str, stage, status);

        // Event-specific broadcasts.
        match event {
            // Agent output → stream to agent:{id} channel.
            DomainEvent::AgentOutput { agent_id, output } => {
                broadcast_agent_output(mgr, &agent_id.to_string(), output);
            }

            // Task blocked → triage:new (P0 item).
            DomainEvent::TaskBlocked { reason } => {
                let item = TriageItemPayload {
                    id: ulid::Ulid::new().to_string(),
                    task_id: task_id_str.clone(),
                    task_name: String::new(), // task name not available in actor
                    agent_name: String::new(),
                    stage: stage.clone(),
                    priority: "p0".to_string(),
                    item_type: "decision".to_string(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    summary: format!("Task blocked: {reason}"),
                };
                broadcast_triage_new(mgr, &item);
            }

            // AwaitingApproval → triage:new (P1 item).
            DomainEvent::AgentCompleted { .. }
                if matches!(new_state, TaskState::AwaitingApproval { .. }) =>
            {
                let item = TriageItemPayload {
                    id: ulid::Ulid::new().to_string(),
                    task_id: task_id_str.clone(),
                    task_name: String::new(),
                    agent_name: String::new(),
                    stage: stage.clone(),
                    priority: "p1".to_string(),
                    item_type: "decision".to_string(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    summary: "Awaiting human approval".to_string(),
                };
                broadcast_triage_new(mgr, &item);
            }

            // Task unblocked → triage:resolved.
            DomainEvent::TaskUnblocked { .. } => {
                // We don't have the original triage item ID; broadcast with
                // task_id so the frontend can match and remove it.
                broadcast_triage_resolved(mgr, &task_id_str);
            }

            // Human decision (approved) → triage:resolved.
            DomainEvent::HumanDecision { .. }
                if matches!(new_state, TaskState::Completed { .. }) =>
            {
                broadcast_triage_resolved(mgr, &task_id_str);
            }

            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// TaskActorHandle — caller-side handle
// ---------------------------------------------------------------------------

/// A cloneable handle to a running `TaskActor`.
///
/// Provides ergonomic methods for sending commands without exposing the raw
/// mpsc channel.
#[derive(Clone)]
pub struct TaskActorHandle {
    task_id: TaskId,
    tx: mpsc::Sender<TaskCommand>,
    /// Subscribe to state changes from this actor.
    pub state_rx: watch::Receiver<StateUpdate>,
}

impl TaskActorHandle {
    /// Apply a domain event to the actor's state machine.
    ///
    /// Returns the new `TaskState` on success, or an `ActorError` if the
    /// transition is invalid or the actor is unreachable.
    pub async fn send_event(
        &self,
        envelope: EventEnvelope,
    ) -> Result<TaskState, ActorError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(TaskCommand::ApplyEvent {
                envelope,
                reply: reply_tx,
            })
            .await
            .map_err(|_| ActorError::ChannelClosed(self.task_id.clone()))?;

        reply_rx
            .await
            .map_err(|_| ActorError::ReplyDropped(self.task_id.clone()))?
    }

    /// Query the actor's current state without modifying it.
    pub async fn get_state(&self) -> Result<TaskState, ActorError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(TaskCommand::GetState { reply: reply_tx })
            .await
            .map_err(|_| ActorError::ChannelClosed(self.task_id.clone()))?;

        reply_rx
            .await
            .map_err(|_| ActorError::ReplyDropped(self.task_id.clone()))
    }

    /// Send a shutdown command to the actor.
    ///
    /// This is a fire-and-forget operation; it does not wait for the actor to
    /// stop. The actor will finish processing its current command before
    /// exiting.
    pub async fn shutdown(&self) -> Result<(), ActorError> {
        self.tx
            .send(TaskCommand::Shutdown)
            .await
            .map_err(|_| ActorError::ChannelClosed(self.task_id.clone()))
    }
}

// ---------------------------------------------------------------------------
// TaskRegistry — manages all active task actors
// ---------------------------------------------------------------------------

/// Manages all active `TaskActor` instances.
///
/// Uses `DashMap` for concurrent access so multiple async tasks can look up
/// or register actors simultaneously without holding a global lock.
pub struct TaskRegistry<S: EventStore + 'static> {
    actors: DashMap<TaskId, TaskActorHandle>,
    store: Arc<S>,
    /// Optional WebSocket connection manager — when set, actors will broadcast
    /// state changes to connected UI clients.
    ws_manager: Option<Arc<ConnectionManager>>,
}

impl<S: EventStore + 'static> TaskRegistry<S> {
    /// Create a new, empty registry backed by `store`.
    pub fn new(store: Arc<S>) -> Self {
        Self {
            actors: DashMap::new(),
            store,
            ws_manager: None,
        }
    }

    /// Create a new registry that broadcasts actor state changes via WebSocket.
    pub fn with_ws(store: Arc<S>, ws_manager: Arc<ConnectionManager>) -> Self {
        Self {
            actors: DashMap::new(),
            store,
            ws_manager: Some(ws_manager),
        }
    }

    /// Return the number of currently active actors.
    pub fn active_count(&self) -> usize {
        self.actors.len()
    }

    /// Spawn a new actor for the given task configuration and insert it into
    /// the registry.
    ///
    /// Returns the handle to the newly spawned actor.
    ///
    /// # Panics
    ///
    /// If an actor for the same `TaskId` already exists in the registry, this
    /// method will replace it without shutting down the old one. Callers should
    /// ensure they don't double-spawn actors for the same task.
    pub fn spawn_task(&self, config: TaskActorConfig) -> TaskActorHandle {
        let task_id = config.task_id.clone();
        let initial_state = StateUpdate {
            task_id: task_id.clone(),
            new_state: TaskState::Pending,
            current_stage: config.initial_stage.clone(),
        };

        let (tx, rx) = mpsc::channel::<TaskCommand>(32);
        let (state_tx, state_rx) = watch::channel(initial_state);

        let actor = TaskActor::new(
            config,
            Arc::clone(&self.store),
            rx,
            state_tx,
            self.ws_manager.clone(),
        );
        tokio::spawn(actor.run());

        let handle = TaskActorHandle {
            task_id: task_id.clone(),
            tx,
            state_rx,
        };

        self.actors.insert(task_id, handle.clone());
        handle
    }

    /// Retrieve the handle for a task, if an actor is currently running.
    pub fn get(&self, task_id: &TaskId) -> Option<TaskActorHandle> {
        self.actors.get(task_id).map(|e| e.value().clone())
    }

    /// Send a shutdown command to a specific actor and remove it from the registry.
    ///
    /// Returns an error if no actor exists for the given `TaskId`.
    pub async fn shutdown_task(&self, task_id: &TaskId) -> Result<(), ActorError> {
        let handle = self
            .actors
            .remove(task_id)
            .map(|(_, h)| h)
            .ok_or_else(|| ActorError::ActorNotFound(task_id.clone()))?;

        handle.shutdown().await
    }

    /// Send shutdown commands to all registered actors and clear the registry.
    pub async fn shutdown_all(&self) {
        // Drain the map first to avoid holding references while awaiting.
        let handles: Vec<_> = self
            .actors
            .iter()
            .map(|e| e.value().clone())
            .collect();
        self.actors.clear();

        for handle in handles {
            if let Err(e) = handle.shutdown().await {
                // Log but continue — other actors should still be shut down.
                error!(error = %e, "error shutting down actor");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::sync::Mutex;

    use molt_hub_core::config::{PipelineConfig, StageDefinition};
    use molt_hub_core::events::store::{EventStore, EventStoreError};
    use molt_hub_core::events::types::{DomainEvent, EventEnvelope};
    use molt_hub_core::model::{AgentId, EventId, SessionId, TaskId, TaskState};

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
            // Simple implementation: just return the single event for tests.
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

    fn make_store() -> Arc<MemoryStore> {
        Arc::new(MemoryStore::default())
    }

    fn make_registry(store: Arc<MemoryStore>) -> TaskRegistry<MemoryStore> {
        TaskRegistry::new(store)
    }

    fn make_config(
        task_id: TaskId,
        initial_stage: &str,
        pipeline: Arc<PipelineConfig>,
    ) -> TaskActorConfig {
        TaskActorConfig {
            task_id,
            session_id: SessionId::new(),
            initial_stage: initial_stage.to_string(),
            pipeline_config: pipeline,
        }
    }

    fn agent_assigned_envelope(task_id: TaskId, session_id: SessionId) -> EventEnvelope {
        EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id),
            project_id: "default".to_owned(),
            session_id,
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::AgentAssigned {
                agent_id: AgentId::new(),
                agent_name: "test-agent".into(),
            },
        }
    }

    fn agent_completed_envelope(task_id: TaskId, session_id: SessionId) -> EventEnvelope {
        EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id),
            project_id: "default".to_owned(),
            session_id,
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::AgentCompleted {
                agent_id: AgentId::new(),
                summary: None,
            },
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn actor_processes_event_and_transitions_state() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let pipeline = make_pipeline(&[("work", false)]);
        let store = make_store();
        let registry = make_registry(Arc::clone(&store));

        let config = make_config(task_id.clone(), "work", pipeline);
        let handle = registry.spawn_task(config);

        // Initial state should be Pending.
        let state = handle.get_state().await.unwrap();
        assert_eq!(state, TaskState::Pending);

        // Apply AgentAssigned → should become InProgress.
        let envelope = agent_assigned_envelope(task_id.clone(), session_id.clone());
        let new_state = handle.send_event(envelope).await.unwrap();
        assert_eq!(new_state, TaskState::InProgress);

        // Event should be persisted in the store.
        let stored = store.get_events_for_task(&task_id).await.unwrap();
        assert_eq!(stored.len(), 1);
    }

    #[tokio::test]
    async fn actor_respects_requires_approval_flag() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        // Stage with requires_approval = true
        let pipeline = make_pipeline(&[("review", true)]);
        let store = make_store();
        let registry = make_registry(Arc::clone(&store));

        let config = make_config(task_id.clone(), "review", pipeline);
        let handle = registry.spawn_task(config);

        // Pending → InProgress
        handle
            .send_event(agent_assigned_envelope(task_id.clone(), session_id.clone()))
            .await
            .unwrap();

        // InProgress + AgentCompleted with requires_approval=true → AwaitingApproval
        let new_state = handle
            .send_event(agent_completed_envelope(task_id.clone(), session_id.clone()))
            .await
            .unwrap();

        assert!(
            matches!(new_state, TaskState::AwaitingApproval { .. }),
            "expected AwaitingApproval, got {new_state:?}"
        );
    }

    #[tokio::test]
    async fn actor_rejects_invalid_transition() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let pipeline = make_pipeline(&[("work", false)]);
        let store = make_store();
        let registry = make_registry(Arc::clone(&store));

        let config = make_config(task_id.clone(), "work", pipeline);
        let handle = registry.spawn_task(config);

        // Pending → AgentCompleted is invalid (must be InProgress first).
        let result = handle
            .send_event(agent_completed_envelope(task_id.clone(), session_id.clone()))
            .await;

        assert!(
            matches!(result, Err(ActorError::Transition { .. })),
            "expected Transition error, got {result:?}"
        );

        // Event must NOT be persisted when transition fails.
        let stored = store.get_events_for_task(&task_id).await.unwrap();
        assert_eq!(stored.len(), 0, "no events should be stored on failure");
    }

    #[tokio::test]
    async fn registry_spawns_and_retrieves_actors() {
        let task_id = TaskId::new();
        let pipeline = make_pipeline(&[("work", false)]);
        let store = make_store();
        let registry = make_registry(Arc::clone(&store));

        assert!(registry.get(&task_id).is_none());

        let config = make_config(task_id.clone(), "work", pipeline);
        registry.spawn_task(config);

        assert!(registry.get(&task_id).is_some());
    }

    #[tokio::test]
    async fn actor_shuts_down_cleanly() {
        let task_id = TaskId::new();
        let pipeline = make_pipeline(&[("work", false)]);
        let store = make_store();
        let registry = make_registry(Arc::clone(&store));

        let config = make_config(task_id.clone(), "work", pipeline);
        let handle = registry.spawn_task(config);

        // Shutdown should succeed without error.
        handle.shutdown().await.unwrap();

        // Subsequent commands should fail because the channel is closed.
        // Give the actor a moment to actually stop.
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let result = handle.get_state().await;
        assert!(
            matches!(result, Err(ActorError::ChannelClosed(_))),
            "expected ChannelClosed after shutdown, got {result:?}"
        );
    }

    #[tokio::test]
    async fn registry_shutdown_task_removes_actor() {
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

    #[tokio::test]
    async fn registry_shutdown_all_clears_registry() {
        let pipeline = make_pipeline(&[("work", false)]);
        let store = make_store();
        let registry = make_registry(Arc::clone(&store));

        for _ in 0..3 {
            let task_id = TaskId::new();
            let config = make_config(task_id, "work", Arc::clone(&pipeline));
            registry.spawn_task(config);
        }

        registry.shutdown_all().await;
        assert_eq!(registry.actors.len(), 0);
    }

    #[tokio::test]
    async fn state_watch_broadcasts_updates() {
        let task_id = TaskId::new();
        let session_id = SessionId::new();
        let pipeline = make_pipeline(&[("work", false)]);
        let store = make_store();
        let registry = make_registry(Arc::clone(&store));

        let config = make_config(task_id.clone(), "work", Arc::clone(&pipeline));
        let handle = registry.spawn_task(config);
        let mut state_rx = handle.state_rx.clone();

        // Apply AgentAssigned — should update the watch channel.
        handle
            .send_event(agent_assigned_envelope(task_id.clone(), session_id.clone()))
            .await
            .unwrap();

        // The watch channel should have a new value.
        state_rx.changed().await.unwrap();
        let update = state_rx.borrow_and_update().clone();
        assert_eq!(update.new_state, TaskState::InProgress);
        assert_eq!(update.task_id, task_id);
    }
}
