//! Stage enter/exit hooks wired through TaskActor + HookExecutor.

use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde_json::json;

use molt_hub_core::config::{
    HookDefinition, HookKind, HookTrigger, PipelineConfig, StageDefinition,
};
use molt_hub_core::events::store::{EventStore, EventStoreError};
use molt_hub_core::events::types::{DomainEvent, EventEnvelope};
use molt_hub_core::model::{AgentId, EventId, SessionId, TaskId, TaskState};

use molt_hub_server::actors::{ActorError, TaskActorConfig, TaskRegistry};
use molt_hub_server::hooks::HookExecutor;

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

fn stage(name: &str, requires_approval: bool, hooks: Vec<HookDefinition>) -> StageDefinition {
    StageDefinition {
        name: name.to_string(),
        label: None,
        instructions: None,
        instructions_template: None,
        requires_approval,
        approvers: vec![],
        timeout_seconds: None,
        terminal: false,
        hooks,
        transition_rules: vec![],
        color: None,
        order: 0,
        wip_limit: None,
    }
}

fn assign_event(task_id: &TaskId, session_id: &SessionId) -> EventEnvelope {
    EventEnvelope {
        id: EventId::new(),
        task_id: Some(task_id.clone()),
        project_id: "default".to_owned(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
        caused_by: None,
        payload: DomainEvent::AgentAssigned {
            agent_id: AgentId::new(),
            agent_name: "agent".into(),
        },
    }
}

#[tokio::test]
async fn task_stage_changed_runs_exit_then_enter_hooks() {
    let dir = tempfile::tempdir().unwrap();
    let order_path = dir.path().join("order.txt");
    let p = order_path.to_str().unwrap();

    let pipeline = Arc::new(PipelineConfig {
        name: "pipe-a".into(),
        description: None,
        version: 1,
        stages: vec![
            stage(
                "plan",
                false,
                vec![HookDefinition {
                    kind: HookKind::Shell,
                    on: HookTrigger::Exit,
                    config: json!({ "command": format!("printf '1' >> {p}") }),
                }],
            ),
            stage(
                "implement",
                false,
                vec![HookDefinition {
                    kind: HookKind::Shell,
                    on: HookTrigger::Enter,
                    config: json!({ "command": format!("printf '2' >> {p}") }),
                }],
            ),
        ],
        integrations: vec![],
        columns: vec![],
    });

    let store = Arc::new(MemoryEventStore::default());
    let hooks = Arc::new(HookExecutor::new());
    let registry = TaskRegistry::new(Arc::clone(&store)).with_hook_executor(hooks);

    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let handle = registry.spawn_task(TaskActorConfig {
        project_id: "default".into(),
        task_id: task_id.clone(),
        session_id: session_id.clone(),
        initial_stage: "plan".into(),
        pipeline_config: Arc::clone(&pipeline),
    });

    handle
        .send_event(assign_event(&task_id, &session_id))
        .await
        .unwrap();

    handle
        .send_event(EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id.clone()),
            project_id: "default".to_owned(),
            session_id: session_id.clone(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::TaskStageChanged {
                from_stage: "plan".into(),
                to_stage: "implement".into(),
                new_state: TaskState::InProgress,
            },
        })
        .await
        .unwrap();

    let content = std::fs::read_to_string(&order_path).unwrap();
    assert_eq!(content, "12", "exit then enter hooks should run in order");
}

#[tokio::test]
async fn aborting_enter_hook_reverts_stage_and_skips_persist() {
    let pipeline = Arc::new(PipelineConfig {
        name: "pipe-b".into(),
        description: None,
        version: 1,
        stages: vec![
            stage("plan", false, vec![]),
            stage(
                "implement",
                false,
                vec![HookDefinition {
                    kind: HookKind::Shell,
                    on: HookTrigger::Enter,
                    config: json!({
                        "command": "exit 1",
                        "failure_policy": "abort"
                    }),
                }],
            ),
        ],
        integrations: vec![],
        columns: vec![],
    });

    let store = Arc::new(MemoryEventStore::default());
    let hooks = Arc::new(HookExecutor::new());
    let registry = TaskRegistry::new(Arc::clone(&store)).with_hook_executor(hooks);

    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let handle = registry.spawn_task(TaskActorConfig {
        project_id: "default".into(),
        task_id: task_id.clone(),
        session_id: session_id.clone(),
        initial_stage: "plan".into(),
        pipeline_config: Arc::clone(&pipeline),
    });

    handle
        .send_event(assign_event(&task_id, &session_id))
        .await
        .unwrap();

    let result = handle
        .send_event(EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id.clone()),
            project_id: "default".to_owned(),
            session_id: session_id.clone(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::TaskStageChanged {
                from_stage: "plan".into(),
                to_stage: "implement".into(),
                new_state: TaskState::InProgress,
            },
        })
        .await;

    assert!(
        matches!(result, Err(ActorError::Hook { .. })),
        "expected Hook error, got {result:?}"
    );

    let stored = store.get_events_for_task(&task_id).await.unwrap();
    assert_eq!(
        stored.len(),
        1,
        "stage-change event must not persist on hook abort"
    );

    // Watch channel / get_state: still InProgress on plan
    let state = handle.get_state().await.unwrap();
    assert_eq!(state, TaskState::InProgress);
    let snap = handle.state_rx.borrow().clone();
    assert_eq!(snap.current_stage, "plan");
}

#[tokio::test]
async fn agent_completed_success_runs_exit_hook_on_stage() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("done.txt");
    let p = marker.to_str().unwrap();

    let pipeline = Arc::new(PipelineConfig {
        name: "pipe-c".into(),
        description: None,
        version: 1,
        stages: vec![stage(
            "work",
            false,
            vec![HookDefinition {
                kind: HookKind::Shell,
                on: HookTrigger::Exit,
                config: json!({ "command": format!("printf done > {p}") }),
            }],
        )],
        integrations: vec![],
        columns: vec![],
    });

    let store = Arc::new(MemoryEventStore::default());
    let hooks = Arc::new(HookExecutor::new());
    let registry = TaskRegistry::new(Arc::clone(&store)).with_hook_executor(hooks);

    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let handle = registry.spawn_task(TaskActorConfig {
        project_id: "default".into(),
        task_id: task_id.clone(),
        session_id: session_id.clone(),
        initial_stage: "work".into(),
        pipeline_config: Arc::clone(&pipeline),
    });

    handle
        .send_event(assign_event(&task_id, &session_id))
        .await
        .unwrap();
    handle
        .send_event(EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id.clone()),
            project_id: "default".to_owned(),
            session_id: session_id.clone(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::AgentCompleted {
                agent_id: AgentId::new(),
                summary: None,
            },
        })
        .await
        .unwrap();

    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "done");
}
