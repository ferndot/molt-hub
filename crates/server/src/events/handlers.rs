//! Axum HTTP handlers for the events and tasks API.
//!
//! Routes:
//!   GET    /api/events            — list events (query: task_id, since)
//!   GET    /api/events/:id        — get a single event by ID
//!   POST   /api/events            — append a new event
//!   GET    /api/tasks             — list tasks derived from events
//!   POST   /api/tasks/create      — create a manual task (`TaskCreated`)
//!   POST   /api/tasks/:id/move    — persisted kanban move + pipeline hooks
//!   POST   /api/tasks/:id/decision — human approve / reject / redirect (`HumanDecision`)

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use ulid::Ulid;

use molt_hub_core::events::types::{DomainEvent, EventEnvelope};
use molt_hub_core::events::{EventStore, EventStoreError, HumanDecisionKind, SqliteEventStore};
use molt_hub_core::machine::{replay_task_machine_from_events, TaskMachine};
use molt_hub_core::model::{EventId, Priority, SessionId, TaskId, TaskOutcome, TaskState};
use molt_hub_harness::supervisor::Supervisor;

use crate::actors::run_lifecycle_hooks_for_event;
use crate::hooks::HookExecutor;
use crate::projects::runtime::{ensure_project_runtime, ProjectRuntimeRegistry};
use crate::ws::ConnectionManager;
use crate::ws_broadcast::{broadcast_board_update_full, BoardUpdate};

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// State shared by event/task handlers — wraps the event store.
pub struct EventStoreState {
    pub store: Arc<SqliteEventStore>,
}

// ---------------------------------------------------------------------------
// Query / response types
// ---------------------------------------------------------------------------

/// Query parameters for `GET /api/events`.
#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    /// Filter events by task ID (ULID string).
    pub task_id: Option<String>,
    /// Filter events since this ISO 8601 timestamp.
    pub since: Option<String>,
}

/// A lightweight task summary derived from events.
#[derive(Debug, Clone, Serialize)]
pub struct TaskSummary {
    pub task_id: String,
    pub title: Option<String>,
    pub event_count: usize,
    pub last_event_at: Option<String>,
}

/// Generic error response body.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Body for `POST /api/tasks/create`.
#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "initialStage")]
    pub initial_stage: Option<String>,
    #[serde(default, rename = "projectId")]
    pub project_id: Option<String>,
    #[serde(default, rename = "boardId")]
    pub board_id: Option<String>,
}

/// Body for `POST /api/tasks/:id/move` — persisted stage change + pipeline hooks.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveTaskBody {
    pub to_stage: String,
    pub board_id: String,
}

/// Body for `POST /api/tasks/:id/decision` — human approve / reject / redirect while awaiting approval.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanDecisionBody {
    pub board_id: String,
    /// `"approved"`, `"rejected"`, or `"redirected"`.
    pub kind: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, rename = "toStage")]
    pub to_stage: Option<String>,
    #[serde(default, rename = "decidedBy")]
    pub decided_by: Option<String>,
}

fn board_status_for_state(s: &TaskState) -> &'static str {
    match s {
        TaskState::Pending => "waiting",
        TaskState::InProgress => "running",
        TaskState::Blocked { .. } => "blocked",
        TaskState::AwaitingApproval { .. } => "waiting",
        TaskState::Completed { .. } => "complete",
        TaskState::Failed { .. } => "blocked",
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/events — list events with optional task_id or since filter.
#[instrument(skip_all)]
pub async fn list_events(
    State(state): State<Arc<EventStoreState>>,
    Query(params): Query<EventsQuery>,
) -> impl IntoResponse {
    // Filter by task_id if provided.
    if let Some(ref tid) = params.task_id {
        let ulid = match Ulid::from_str(tid) {
            Ok(u) => u,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("invalid task_id: {e}") })),
                )
                    .into_response();
            }
        };
        let task_id = TaskId(ulid);
        match state.store.get_events_for_task(&task_id).await {
            Ok(events) => (
                StatusCode::OK,
                Json(serde_json::json!({ "events": events })),
            )
                .into_response(),
            Err(e) => error_response(e),
        }
    }
    // Filter by since if provided.
    else if let Some(ref since_str) = params.since {
        let since = match since_str.parse::<DateTime<Utc>>() {
            Ok(dt) => dt,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("invalid since timestamp: {e}") })),
                )
                    .into_response();
            }
        };
        match state.store.get_events_since(since).await {
            Ok(events) => (
                StatusCode::OK,
                Json(serde_json::json!({ "events": events })),
            )
                .into_response(),
            Err(e) => error_response(e),
        }
    }
    // No filter: return events since epoch (all events).
    else {
        let since = DateTime::<Utc>::MIN_UTC;
        match state.store.get_events_since(since).await {
            Ok(events) => (
                StatusCode::OK,
                Json(serde_json::json!({ "events": events })),
            )
                .into_response(),
            Err(e) => error_response(e),
        }
    }
}

/// GET /api/events/:id — get a single event by its ULID.
#[instrument(skip_all)]
pub async fn get_event(
    State(state): State<Arc<EventStoreState>>,
    Path(id_str): Path<String>,
) -> impl IntoResponse {
    let ulid = match Ulid::from_str(&id_str) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid event id: {e}") })),
            )
                .into_response();
        }
    };
    let event_id = EventId(ulid);
    match state.store.get_event_by_id(&event_id).await {
        Ok(Some(envelope)) => (StatusCode::OK, Json(serde_json::json!(envelope))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "event not found" })),
        )
            .into_response(),
        Err(e) => error_response(e),
    }
}

/// POST /api/events — append a new event envelope.
#[instrument(skip_all)]
pub async fn append_event(
    State(state): State<Arc<EventStoreState>>,
    Json(envelope): Json<EventEnvelope>,
) -> impl IntoResponse {
    let task_id_str = envelope
        .task_id
        .as_ref()
        .map(|t| t.0.to_string())
        .unwrap_or_default();
    info!(event_id = %envelope.id, task_id = %task_id_str, "appending event via API");
    match state.store.append(envelope).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "status": "ok" })),
        )
            .into_response(),
        Err(e) => error_response(e),
    }
}

/// GET /api/tasks — derive a task list from all events.
///
/// Groups events by task_id and extracts the title from the most recent
/// `TaskCreated` event for each task.
#[instrument(skip_all)]
pub async fn list_tasks(State(state): State<Arc<EventStoreState>>) -> impl IntoResponse {
    let since = DateTime::<Utc>::MIN_UTC;
    match state.store.get_events_since(since).await {
        Ok(events) => {
            let mut tasks: HashMap<String, TaskSummary> = HashMap::new();

            for ev in &events {
                let tid = ev
                    .task_id
                    .as_ref()
                    .map(|t| t.0.to_string())
                    .unwrap_or_else(|| ev.project_id.clone());
                let entry = tasks.entry(tid.clone()).or_insert_with(|| TaskSummary {
                    task_id: tid,
                    title: None,
                    event_count: 0,
                    last_event_at: None,
                });
                entry.event_count += 1;
                entry.last_event_at = Some(ev.timestamp.to_rfc3339());

                // Extract title from TaskCreated events.
                if let DomainEvent::TaskCreated { ref title, .. } = ev.payload {
                    entry.title = Some(title.clone());
                }
            }

            let mut task_list: Vec<TaskSummary> = tasks.into_values().collect();
            task_list.sort_by(|a, b| b.last_event_at.cmp(&a.last_event_at));

            (
                StatusCode::OK,
                Json(serde_json::json!({ "tasks": task_list })),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// POST /api/tasks/create — append `TaskCreated` and broadcast board update.
#[instrument(skip_all)]
pub async fn create_task(
    State(state): State<Arc<EventStoreState>>,
    Extension(manager): Extension<Arc<ConnectionManager>>,
    Json(body): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    let title = body.title.trim();
    if title.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "title is required" })),
        )
            .into_response();
    }

    let stage = body
        .initial_stage
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("backlog");
    let stage_owned = stage.to_owned();

    let description = body
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_owned();

    let project_topic = body
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("default");

    let task_id = TaskId::new();
    let envelope = EventEnvelope {
        id: EventId::new(),
        task_id: Some(task_id.clone()),
        project_id: "default".to_owned(),
        session_id: SessionId::new(),
        timestamp: Utc::now(),
        caused_by: None,
        payload: DomainEvent::TaskCreated {
            title: title.to_owned(),
            description,
            initial_stage: stage_owned.clone(),
            priority: Priority::P2,
            board_id: body.board_id.clone(),
        },
    };

    match state.store.append(envelope).await {
        Ok(()) => {
            broadcast_board_update_full(
                manager.as_ref(),
                project_topic,
                &BoardUpdate {
                    task_id: task_id.to_string(),
                    stage: stage_owned,
                    status: "waiting".to_owned(),
                    priority: None,
                    name: Some(title.to_owned()),
                    agent_name: None,
                    summary: None,
                    board_id: body.board_id.clone(),
                },
            );
            (
                StatusCode::CREATED,
                Json(serde_json::json!({ "taskId": task_id.to_string() })),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// POST /api/tasks/:id/move — validate transition, run enter/exit hooks, persist `TaskStageChanged`.
#[instrument(skip_all)]
pub async fn move_task_stage(
    State(state): State<Arc<EventStoreState>>,
    Extension(manager): Extension<Arc<ConnectionManager>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    Extension(hook_executor): Extension<Arc<HookExecutor>>,
    Path(task_id_str): Path<String>,
    Json(body): Json<MoveTaskBody>,
) -> impl IntoResponse {
    let task_id_ulid = match Ulid::from_str(task_id_str.trim()) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid task id: {e}") })),
            )
                .into_response();
        }
    };
    let task_id = TaskId(task_id_ulid);

    let to_stage = body.to_stage.trim().to_owned();
    let board_id = body.board_id.trim();
    if to_stage.is_empty() || board_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "toStage and boardId are required" })),
        )
            .into_response();
    }

    let rt = ensure_project_runtime("default", &registry, &supervisor).await;
    let board_store = match rt.boards.get_store(board_id).await {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("board '{board_id}' not found") })),
            )
                .into_response();
        }
    };

    let pipeline = board_store.snapshot_config().await;
    if !pipeline.stages.iter().any(|s| s.name == to_stage) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("unknown stage '{}'", to_stage) })),
        )
            .into_response();
    }

    let events = match state.store.get_events_for_task(&task_id).await {
        Ok(e) => e,
        Err(e) => return error_response(e),
    };

    let machine = match replay_task_machine_from_events(&events, &pipeline) {
        Ok(m) => m,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };

    if machine.is_terminal() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "task is in a terminal state" })),
        )
            .into_response();
    }

    let from_stage = machine.current_stage.clone();
    if from_stage == to_stage {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "task is already in this stage" })),
        )
            .into_response();
    }

    let requires_approval = pipeline
        .stages
        .iter()
        .find(|s| s.name == from_stage)
        .map(|s| s.requires_approval)
        .unwrap_or(false);

    let mut probe = machine.clone();
    let new_state = match probe.apply_with_approval_flag(
        &DomainEvent::TaskStageChanged {
            from_stage: from_stage.clone(),
            to_stage: to_stage.clone(),
            new_state: TaskState::Pending,
        },
        requires_approval,
    ) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let payload = DomainEvent::TaskStageChanged {
        from_stage: from_stage.clone(),
        to_stage: to_stage.clone(),
        new_state: new_state.clone(),
    };

    let session_id = events
        .last()
        .map(|e| e.session_id.clone())
        .unwrap_or_else(SessionId::new);
    let project_id = events
        .last()
        .map(|e| e.project_id.clone())
        .unwrap_or_else(|| "default".to_owned());

    let envelope = EventEnvelope {
        id: EventId::new(),
        task_id: Some(task_id.clone()),
        project_id: project_id.clone(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
        caused_by: None,
        payload: payload.clone(),
    };

    let task_title = events.iter().rev().find_map(|e| {
        if let DomainEvent::TaskCreated { title, .. } = &e.payload {
            Some(title.clone())
        } else {
            None
        }
    });
    let (hook_task_title, hook_task_description, hook_priority) =
        events.iter().rev().find_map(|e| {
            if let DomainEvent::TaskCreated { title, description, priority, .. } = &e.payload {
                let p = match priority {
                    molt_hub_core::model::Priority::P0 => "p0",
                    molt_hub_core::model::Priority::P1 => "p1",
                    molt_hub_core::model::Priority::P2 => "p2",
                    molt_hub_core::model::Priority::P3 => "p3",
                };
                Some((title.clone(), description.clone(), p.to_string()))
            } else {
                None
            }
        }).unwrap_or_else(|| ("".to_string(), "".to_string(), "p2".to_string()));

    let ws_project = events
        .last()
        .map(|e| e.project_id.as_str())
        .unwrap_or("default");

    if let Err(source) = run_lifecycle_hooks_for_event(
        hook_executor.as_ref(),
        &pipeline,
        &task_id,
        &session_id,
        &envelope,
        &from_stage,
        &to_stage,
        &new_state,
        &hook_task_title,
        &hook_task_description,
        &hook_priority,
        Some(&manager),
        ws_project,
    )
    .await
    {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": source.to_string() })),
        )
            .into_response();
    }

    if let Err(e) = state.store.append(envelope).await {
        return error_response(e);
    }

    broadcast_board_update_full(
        manager.as_ref(),
        ws_project,
        &BoardUpdate {
            task_id: task_id.to_string(),
            stage: to_stage.clone(),
            status: board_status_for_state(&new_state).to_owned(),
            priority: None,
            name: task_title,
            agent_name: None,
            summary: None,
            board_id: Some(body.board_id.clone()),
        },
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "taskId": task_id.to_string(),
            "stage": to_stage,
            "status": board_status_for_state(&new_state),
        })),
    )
        .into_response()
}

/// POST /api/tasks/:id/decision — append `HumanDecision` when task is awaiting approval.
#[instrument(skip_all)]
pub async fn submit_human_decision(
    State(state): State<Arc<EventStoreState>>,
    Extension(manager): Extension<Arc<ConnectionManager>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    Extension(hook_executor): Extension<Arc<HookExecutor>>,
    Path(task_id_str): Path<String>,
    Json(body): Json<HumanDecisionBody>,
) -> impl IntoResponse {
    let task_id_ulid = match Ulid::from_str(task_id_str.trim()) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid task id: {e}") })),
            )
                .into_response();
        }
    };
    let task_id = TaskId(task_id_ulid);

    let board_id = body.board_id.trim();
    if board_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "boardId is required" })),
        )
            .into_response();
    }

    let rt = ensure_project_runtime("default", &registry, &supervisor).await;
    let board_store = match rt.boards.get_store(board_id).await {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("board '{board_id}' not found") })),
            )
                .into_response();
        }
    };

    let pipeline = board_store.snapshot_config().await;

    let events = match state.store.get_events_for_task(&task_id).await {
        Ok(e) => e,
        Err(e) => return error_response(e),
    };

    let machine = match replay_task_machine_from_events(&events, &pipeline) {
        Ok(m) => m,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };

    if !matches!(machine.state, TaskState::AwaitingApproval { .. }) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "task is not awaiting human approval",
            })),
        )
            .into_response();
    }

    let stage_before = machine.current_stage.clone();
    let kind = body.kind.trim().to_ascii_lowercase();
    let decision_kind = match kind.as_str() {
        "approved" => HumanDecisionKind::Approved,
        "rejected" => HumanDecisionKind::Rejected {
            reason: body
                .reason
                .clone()
                .unwrap_or_default()
                .trim()
                .to_owned(),
        },
        "redirected" => {
            let to = body
                .to_stage
                .as_ref()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            let Some(to_stage) = to else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "toStage is required for redirected" })),
                )
                    .into_response();
            };
            if !pipeline.stages.iter().any(|s| s.name == to_stage) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("unknown stage '{to_stage}'") })),
                )
                    .into_response();
            }
            let reason = body
                .reason
                .clone()
                .unwrap_or_default()
                .trim()
                .to_owned();
            HumanDecisionKind::Redirected { to_stage, reason }
        }
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "kind must be approved, rejected, or redirected",
                })),
            )
                .into_response();
        }
    };

    let decided_by = body
        .decided_by
        .as_ref()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "user".to_owned());

    let payload = DomainEvent::HumanDecision {
        decided_by,
        decision: decision_kind.clone(),
        note: None,
    };

    let mut probe = machine.clone();
    let new_state = match probe.apply_with_approval_flag(&payload, false) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let session_id = events
        .last()
        .map(|e| e.session_id.clone())
        .unwrap_or_else(SessionId::new);
    let project_id = events
        .last()
        .map(|e| e.project_id.clone())
        .unwrap_or_else(|| "default".to_owned());

    let envelope = EventEnvelope {
        id: EventId::new(),
        task_id: Some(task_id.clone()),
        project_id: project_id.clone(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
        caused_by: None,
        payload: payload.clone(),
    };

    let stage_after = probe.current_stage.clone();

    let task_title = events.iter().rev().find_map(|e| {
        if let DomainEvent::TaskCreated { title, .. } = &e.payload {
            Some(title.clone())
        } else {
            None
        }
    });
    let (hook_task_title2, hook_task_description2, hook_priority2) =
        events.iter().rev().find_map(|e| {
            if let DomainEvent::TaskCreated { title, description, priority, .. } = &e.payload {
                let p = match priority {
                    molt_hub_core::model::Priority::P0 => "p0",
                    molt_hub_core::model::Priority::P1 => "p1",
                    molt_hub_core::model::Priority::P2 => "p2",
                    molt_hub_core::model::Priority::P3 => "p3",
                };
                Some((title.clone(), description.clone(), p.to_string()))
            } else {
                None
            }
        }).unwrap_or_else(|| ("".to_string(), "".to_string(), "p2".to_string()));

    let ws_project = events
        .last()
        .map(|e| e.project_id.as_str())
        .unwrap_or("default");

    if let Err(source) = run_lifecycle_hooks_for_event(
        hook_executor.as_ref(),
        &pipeline,
        &task_id,
        &session_id,
        &envelope,
        &stage_before,
        &stage_after,
        &new_state,
        &hook_task_title2,
        &hook_task_description2,
        &hook_priority2,
        Some(&manager),
        ws_project,
    )
    .await
    {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": source.to_string() })),
        )
            .into_response();
    }

    if let Err(e) = state.store.append(envelope).await {
        return error_response(e);
    }

    broadcast_board_update_full(
        manager.as_ref(),
        ws_project,
        &BoardUpdate {
            task_id: task_id.to_string(),
            stage: stage_after,
            status: board_status_for_state(&new_state).to_owned(),
            priority: None,
            name: task_title,
            agent_name: None,
            summary: None,
            board_id: Some(body.board_id.clone()),
        },
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "taskId": task_id.to_string(),
            "status": board_status_for_state(&new_state),
        })),
    )
        .into_response()
}

/// GET /api/tasks/triage — derive tasks needing human attention from all stored events.
#[instrument(skip_all)]
pub async fn list_triage_tasks(State(state): State<Arc<EventStoreState>>) -> impl IntoResponse {
    use std::collections::HashMap;

    let since = DateTime::<Utc>::MIN_UTC;
    let events = match state.store.get_events_since(since).await {
        Ok(e) => e,
        Err(e) => return error_response(e),
    };

    #[derive(Default)]
    struct Proj {
        id: String,
        title: String,
        stage: String,
        priority: String,
        agent_name: Option<String>,
        summary: String,
        status: String,
        created_at: String,
        had_agent_completed: bool,
    }

    let mut tasks: HashMap<String, Proj> = HashMap::new();

    for envelope in &events {
        let Some(ref tid) = envelope.task_id else {
            continue;
        };
        let id = tid.0.to_string();
        let proj = tasks.entry(id.clone()).or_insert_with(|| Proj {
            id: id.clone(),
            status: "waiting".to_owned(),
            ..Default::default()
        });

        match &envelope.payload {
            DomainEvent::TaskCreated {
                title,
                priority,
                initial_stage,
                ..
            } => {
                proj.title = title.clone();
                proj.stage = initial_stage.clone();
                proj.priority = format!("{priority:?}").to_ascii_lowercase();
                proj.status = "waiting".to_owned();
                proj.created_at = envelope.timestamp.to_rfc3339();
            }
            DomainEvent::TaskStageChanged { to_stage, .. } => {
                proj.stage = to_stage.clone();
                if proj.status == "blocked" {
                    proj.status = if proj.agent_name.is_some() {
                        "running".to_owned()
                    } else {
                        "waiting".to_owned()
                    };
                } else {
                    proj.status = "waiting".to_owned();
                }
            }
            DomainEvent::TaskPriorityChanged { to, .. } => {
                proj.priority = format!("{to:?}").to_ascii_lowercase();
            }
            DomainEvent::AgentAssigned { agent_name, .. } => {
                proj.agent_name = Some(agent_name.clone());
                proj.status = "running".to_owned();
            }
            DomainEvent::AgentOutput { .. } => {}
            DomainEvent::AgentCompleted { summary, .. } => {
                proj.had_agent_completed = true;
                if let Some(s) = summary {
                    proj.summary = s.clone();
                }
                proj.agent_name = None;
                if proj.stage == "deployment" {
                    proj.status = "complete".to_owned();
                } else {
                    proj.status = "waiting".to_owned();
                }
            }
            DomainEvent::TaskBlocked { .. } => {
                proj.status = "blocked".to_owned();
            }
            DomainEvent::TaskUnblocked { .. } => {
                proj.status = if proj.agent_name.is_some() {
                    "running".to_owned()
                } else {
                    "waiting".to_owned()
                };
            }
            DomainEvent::TaskCompleted { .. } => {
                proj.status = "complete".to_owned();
            }
            _ => {}
        }
    }

    let items: Vec<serde_json::Value> = tasks
        .into_values()
        .filter(|p| !p.title.is_empty())
        .filter(|p| {
            // Include blocked tasks (type = "info")
            if p.status == "blocked" {
                return true;
            }
            // Include tasks in testing/review/planning with status "waiting" that had AgentCompleted (type = "decision")
            if p.status == "waiting"
                && (p.stage == "testing" || p.stage == "review" || p.stage == "planning")
                && p.had_agent_completed
            {
                return true;
            }
            false
        })
        .map(|p| {
            let item_type = if p.status == "blocked" { "info" } else { "decision" };
            serde_json::json!({
                "id": p.id,
                "task_id": p.id,
                "task_name": p.title,
                "agent_name": p.agent_name.unwrap_or_default(),
                "stage": p.stage,
                "priority": p.priority,
                "type": item_type,
                "created_at": p.created_at,
                "summary": p.summary,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({ "items": items })),
    )
        .into_response()
}

/// GET /api/tasks/:id — derive full task detail from events.
#[instrument(skip_all)]
pub async fn get_task_detail(
    State(state): State<Arc<EventStoreState>>,
    Path(task_id_str): Path<String>,
) -> impl IntoResponse {
    let ulid = match Ulid::from_str(task_id_str.trim()) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid task id: {e}") })),
            )
                .into_response();
        }
    };
    let task_id = TaskId(ulid);

    let events = match state.store.get_events_for_task(&task_id).await {
        Ok(e) => e,
        Err(e) => return error_response(e),
    };

    if events.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "task not found" })),
        )
            .into_response();
    }

    let mut title = String::new();
    let mut description = String::new();
    let mut current_stage = String::new();
    let mut priority = String::new();
    let mut assigned_agent: Option<String> = None;
    let mut agent_name: Option<String> = None;
    let mut state_type = "pending".to_owned();
    let mut created_at = String::new();
    let mut updated_at = String::new();

    for envelope in &events {
        updated_at = envelope.timestamp.to_rfc3339();
        match &envelope.payload {
            DomainEvent::TaskCreated {
                title: t,
                description: d,
                initial_stage,
                priority: p,
                ..
            } => {
                title = t.clone();
                description = d.clone();
                current_stage = initial_stage.clone();
                priority = format!("{p:?}").to_ascii_lowercase();
                created_at = envelope.timestamp.to_rfc3339();
                state_type = "pending".to_owned();
            }
            DomainEvent::TaskStageChanged { to_stage, .. } => {
                current_stage = to_stage.clone();
                state_type = "in_progress".to_owned();
            }
            DomainEvent::TaskPriorityChanged { to, .. } => {
                priority = format!("{to:?}").to_ascii_lowercase();
            }
            DomainEvent::AgentAssigned {
                agent_id,
                agent_name: name,
            } => {
                assigned_agent = Some(agent_id.0.to_string());
                agent_name = Some(name.clone());
                state_type = "in_progress".to_owned();
            }
            DomainEvent::AgentCompleted { .. } => {
                assigned_agent = None;
                agent_name = None;
                state_type = "awaiting_approval".to_owned();
            }
            DomainEvent::TaskBlocked { .. } => {
                state_type = "blocked".to_owned();
            }
            DomainEvent::TaskUnblocked { .. } => {
                state_type = if assigned_agent.is_some() {
                    "in_progress".to_owned()
                } else {
                    "pending".to_owned()
                };
            }
            DomainEvent::HumanDecision { .. } => {
                state_type = "in_progress".to_owned();
            }
            DomainEvent::TaskCompleted { outcome } => {
                state_type = match outcome {
                    TaskOutcome::Success => "completed",
                    TaskOutcome::Rejected { .. } => "failed",
                    TaskOutcome::Abandoned { .. } => "completed",
                }
                .to_owned();
            }
            _ => {}
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": task_id.0.to_string(),
            "title": title,
            "description": description,
            "current_stage": current_stage,
            "priority": priority,
            "assigned_agent": assigned_agent,
            "agent_name": agent_name,
            "state_type": state_type,
            "created_at": created_at,
            "updated_at": updated_at,
        })),
    )
        .into_response()
}

/// GET /api/tasks/:id/events — return a formatted activity timeline for a task.
#[instrument(skip_all)]
pub async fn get_task_events(
    State(state): State<Arc<EventStoreState>>,
    Path(task_id_str): Path<String>,
) -> impl IntoResponse {
    let ulid = match Ulid::from_str(task_id_str.trim()) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid task id: {e}") })),
            )
                .into_response();
        }
    };
    let task_id = TaskId(ulid);

    let events = match state.store.get_events_for_task(&task_id).await {
        Ok(e) => e,
        Err(e) => return error_response(e),
    };

    let formatted: Vec<serde_json::Value> = events
        .iter()
        .map(|envelope| {
            let (event_type, actor, description) = match &envelope.payload {
                DomainEvent::TaskCreated {
                    initial_stage,
                    priority,
                    ..
                } => (
                    "task_created",
                    "system".to_owned(),
                    format!(
                        "Task created in {} with {} priority",
                        initial_stage,
                        format!("{priority:?}").to_ascii_lowercase()
                    ),
                ),
                DomainEvent::TaskStageChanged {
                    from_stage,
                    to_stage,
                    ..
                } => (
                    "task_stage_changed",
                    "system".to_owned(),
                    format!("Moved from {} to {}", from_stage, to_stage),
                ),
                DomainEvent::AgentAssigned { agent_name, .. } => (
                    "agent_assigned",
                    agent_name.clone(),
                    format!("Agent {} assigned", agent_name),
                ),
                DomainEvent::AgentOutput { agent_id, output } => {
                    let truncated = if output.len() > 80 {
                        format!("{}…", &output[..80])
                    } else {
                        output.clone()
                    };
                    (
                        "agent_output",
                        agent_id.0.to_string(),
                        format!("Agent output: {}", truncated),
                    )
                }
                DomainEvent::AgentCompleted {
                    agent_id, summary, ..
                } => (
                    "agent_completed",
                    agent_id.0.to_string(),
                    format!(
                        "Agent completed: {}",
                        summary.as_deref().unwrap_or("work complete")
                    ),
                ),
                DomainEvent::TaskBlocked { reason } => (
                    "task_blocked",
                    "system".to_owned(),
                    format!("Blocked: {}", reason),
                ),
                DomainEvent::TaskUnblocked { resolution } => (
                    "task_unblocked",
                    "system".to_owned(),
                    format!(
                        "Unblocked: {}",
                        resolution.as_deref().unwrap_or("resolved")
                    ),
                ),
                DomainEvent::HumanDecision {
                    decided_by,
                    decision,
                    ..
                } => {
                    let kind_str = match decision {
                        HumanDecisionKind::Approved => "approved",
                        HumanDecisionKind::Rejected { .. } => "rejected",
                        HumanDecisionKind::Redirected { .. } => {
                            "redirected"
                        }
                    };
                    (
                        "human_decision",
                        "human".to_owned(),
                        format!("{}: {}", decided_by, kind_str),
                    )
                }
                DomainEvent::TaskPriorityChanged { from, to } => (
                    "task_priority_changed",
                    "system".to_owned(),
                    format!(
                        "Priority changed from {} to {}",
                        format!("{from:?}").to_ascii_lowercase(),
                        format!("{to:?}").to_ascii_lowercase()
                    ),
                ),
                DomainEvent::TaskCompleted { .. } => (
                    "task_completed",
                    "system".to_owned(),
                    "Task completed".to_owned(),
                ),
                other => {
                    let type_str = format!("{:?}", other)
                        .split('{')
                        .next()
                        .unwrap_or("unknown")
                        .trim()
                        .to_ascii_lowercase();
                    (
                        "unknown",
                        "system".to_owned(),
                        type_str,
                    )
                }
            };

            serde_json::json!({
                "id": envelope.id.0.to_string(),
                "timestamp": envelope.timestamp.to_rfc3339(),
                "event_type": event_type,
                "actor": actor,
                "description": description,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({ "events": formatted })),
    )
        .into_response()
}

/// DELETE /api/tasks/:id — physically remove all events for a task.
#[instrument(skip_all)]
pub async fn delete_task(
    State(state): State<Arc<EventStoreState>>,
    Extension(manager): Extension<Arc<ConnectionManager>>,
    Path(task_id_str): Path<String>,
) -> impl IntoResponse {
    let task_id_ulid = match Ulid::from_str(task_id_str.trim()) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid task id: {e}") })),
            )
                .into_response();
        }
    };
    let task_id = TaskId(task_id_ulid);

    // Get the project_id before deleting so we can broadcast removal.
    let events = state.store.get_events_for_task(&task_id).await;
    let project_id = events
        .ok()
        .and_then(|ev| ev.last().map(|e| e.project_id.clone()))
        .unwrap_or_else(|| "default".to_owned());

    match state.store.delete_task(&task_id).await {
        Ok(()) => {
            // Broadcast a board update with status "deleted" so clients remove the card.
            broadcast_board_update_full(
                manager.as_ref(),
                &project_id,
                &BoardUpdate {
                    task_id: task_id.to_string(),
                    stage: String::new(),
                    status: "deleted".to_owned(),
                    priority: None,
                    name: None,
                    agent_name: None,
                    summary: None,
                    board_id: None,
                },
            );
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// GET /api/tasks/board — derive current board-task state from all stored events.
///
/// Returns a flat list of tasks with the fields the board UI needs: id, name,
/// stage, status, priority, agent_name, and summary.  The status is derived
/// from a simple event scan (no pipeline config required):
///
/// - `blocked`  — last status-relevant event was `TaskBlocked`
/// - `running`  — last status-relevant event was `AgentAssigned`
/// - `complete` — `AgentCompleted` while in the terminal `deployed` stage
/// - `waiting`  — everything else (pending, code-review, testing, etc.)
#[instrument(skip_all)]
pub async fn list_board_tasks(
    State(state): State<Arc<EventStoreState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    use std::collections::HashMap;

    let since = DateTime::<Utc>::MIN_UTC;
    let events = match state.store.get_events_since(since).await {
        Ok(e) => e,
        Err(e) => return error_response(e),
    };

    #[derive(Default)]
    struct Proj {
        id: String,
        title: String,
        stage: String,
        priority: String,
        agent_name: Option<String>,
        summary: String,
        /// Derived from the last status-affecting event.
        status: String,
        board_id: Option<String>,
    }

    let mut tasks: HashMap<String, Proj> = HashMap::new();

    for envelope in &events {
        let Some(ref tid) = envelope.task_id else {
            continue;
        };
        let id = tid.0.to_string();
        let proj = tasks.entry(id.clone()).or_insert_with(|| Proj {
            id: id.clone(),
            status: "waiting".to_owned(),
            ..Default::default()
        });

        match &envelope.payload {
            DomainEvent::TaskCreated {
                title,
                priority,
                initial_stage,
                board_id,
                ..
            } => {
                proj.title = title.clone();
                proj.stage = initial_stage.clone();
                proj.priority = format!("{priority:?}").to_ascii_lowercase();
                proj.status = "waiting".to_owned();
                proj.board_id = board_id.clone();
            }
            DomainEvent::TaskStageChanged { to_stage, .. } => {
                proj.stage = to_stage.clone();
                if proj.status == "blocked" {
                    // Unblock on stage change
                    proj.status = if proj.agent_name.is_some() {
                        "running".to_owned()
                    } else {
                        "waiting".to_owned()
                    };
                } else {
                    // Moving stages always resets to waiting
                    proj.status = "waiting".to_owned();
                }
            }
            DomainEvent::TaskPriorityChanged { to, .. } => {
                proj.priority = format!("{to:?}").to_ascii_lowercase();
            }
            DomainEvent::AgentAssigned { agent_name, .. } => {
                proj.agent_name = Some(agent_name.clone());
                proj.status = "running".to_owned();
            }
            DomainEvent::AgentOutput { .. } => {
                // Keep status as-is; agent is still running
            }
            DomainEvent::AgentCompleted { summary, .. } => {
                proj.agent_name = None;
                if let Some(s) = summary {
                    proj.summary = s.clone();
                }
                // Terminal stage → complete; otherwise waiting for next action
                if proj.stage == "deployment" {
                    proj.status = "complete".to_owned();
                } else {
                    proj.status = "waiting".to_owned();
                }
            }
            DomainEvent::TaskBlocked { .. } => {
                proj.status = "blocked".to_owned();
            }
            DomainEvent::TaskUnblocked { .. } => {
                proj.status = if proj.agent_name.is_some() {
                    "running".to_owned()
                } else {
                    "waiting".to_owned()
                };
            }
            DomainEvent::TaskCompleted { .. } => {
                proj.status = "complete".to_owned();
            }
            _ => {}
        }
    }

    let filter_board_id = params.get("boardId").map(|s| s.as_str());
    let mut task_list: Vec<serde_json::Value> = tasks
        .into_values()
        .filter(|p| !p.title.is_empty())
        .filter(|p| {
            // If no filter, include all tasks (backwards compat).
            // If filter provided, include tasks matching that board OR tasks with no board.
            match filter_board_id {
                None => true,
                Some(bid) => p.board_id.as_deref().map_or(false, |t| t == bid),
            }
        })
        .map(|p| {
            serde_json::json!({
                "task_id":    p.id,
                "name":       p.title,
                "stage":      p.stage,
                "status":     p.status,
                "priority":   p.priority,
                "agent_name": p.agent_name,
                "summary":    p.summary,
                "board_id":   p.board_id,
            })
        })
        .collect();

    // Sort newest-first by task_id (ULIDs are lexicographically time-ordered).
    task_list.sort_by(|a, b| {
        b["task_id"]
            .as_str()
            .cmp(&a["task_id"].as_str())
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({ "tasks": task_list })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn error_response(err: EventStoreError) -> axum::response::Response {
    warn!(error = %err, "event store error");
    let (status, msg) = match &err {
        EventStoreError::EventNotFound(_) => (StatusCode::NOT_FOUND, err.to_string()),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

// ---------------------------------------------------------------------------
// Router builders
// ---------------------------------------------------------------------------

/// Build the `/api/events` sub-router.
pub fn events_router(state: Arc<EventStoreState>) -> Router {
    Router::new()
        .route("/", get(list_events).post(append_event))
        .route("/:id", get(get_event))
        .with_state(state)
}

/// GET /api/tasks/:id — fetch a single task's detail by replaying its events.
#[instrument(skip(state))]
pub async fn get_task(
    State(state): State<Arc<EventStoreState>>,
    Path(task_id_str): Path<String>,
) -> impl IntoResponse {
    let task_id_ulid = match Ulid::from_str(task_id_str.trim()) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid task id: {e}") })),
            )
                .into_response();
        }
    };
    let task_id = TaskId(task_id_ulid);

    let events = match state.store.get_events_for_task(&task_id).await {
        Ok(e) => e,
        Err(e) => return error_response(e),
    };

    if events.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "task not found" })),
        )
            .into_response();
    }

    // Derive task detail from events.
    let mut title = String::new();
    let mut description = String::new();
    let mut priority = Priority::P2;
    let mut current_stage = String::new();
    let mut assigned_agent: Option<String> = None;
    let mut agent_name: Option<String> = None;
    let mut created_at = events[0].timestamp;
    let mut updated_at = events[0].timestamp;

    for ev in &events {
        updated_at = ev.timestamp;
        match &ev.payload {
            DomainEvent::TaskCreated {
                title: t,
                description: d,
                initial_stage,
                priority: p,
                ..
            } => {
                title = t.clone();
                description = d.clone();
                priority = p.clone();
                current_stage = initial_stage.clone();
                created_at = ev.timestamp;
            }
            DomainEvent::TaskStageChanged { to_stage, .. } => {
                current_stage = to_stage.clone();
            }
            DomainEvent::AgentAssigned {
                agent_id,
                agent_name: name,
            } => {
                assigned_agent = Some(agent_id.0.to_string());
                agent_name = Some(name.clone());
            }
            DomainEvent::AgentCompleted { .. } => {
                agent_name = None;
            }
            _ => {}
        }
    }

    let state_type = {
        let mut m = TaskMachine::new(String::new());
        for ev in &events {
            if let DomainEvent::TaskCreated { initial_stage, .. } = &ev.payload {
                m = TaskMachine::new(initial_stage.clone());
            } else {
                let _ = m.apply(&ev.payload);
            }
        }
        board_status_for_state(&m.state).to_string()
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": task_id.0.to_string(),
            "title": title,
            "description": description,
            "current_stage": current_stage,
            "priority": format!("{priority:?}").to_lowercase(),
            "assigned_agent": assigned_agent,
            "agent_name": agent_name,
            "state_type": state_type,
            "created_at": created_at.to_rfc3339(),
            "updated_at": updated_at.to_rfc3339(),
        })),
    )
        .into_response()
}

/// Build the `/api/tasks` sub-router.
pub fn tasks_router(state: Arc<EventStoreState>) -> Router {
    Router::new()
        .route("/", get(list_tasks))
        .route("/board", get(list_board_tasks))
        .route("/triage", get(list_triage_tasks))
        .route("/create", post(create_task))
        .route("/:id", get(get_task_detail).delete(delete_task))
        .route("/:id/events", get(get_task_events))
        .route("/:id/move", post(move_task_stage))
        .route("/:id/decision", post(submit_human_decision))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request, Response},
    };
    use crate::ws::ConnectionManager;

    use molt_hub_core::events::SqliteEventStore;
    use molt_hub_core::model::{Priority, SessionId};
    use sqlx::sqlite::SqlitePoolOptions;
    use tower::util::ServiceExt;

    /// Create an in-memory SQLite event store for testing.
    async fn test_store() -> Arc<SqliteEventStore> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("failed to create in-memory pool");
        Arc::new(
            SqliteEventStore::new(pool)
                .await
                .expect("failed to init store"),
        )
    }

    fn test_state(store: Arc<SqliteEventStore>) -> Arc<EventStoreState> {
        Arc::new(EventStoreState { store })
    }

    fn test_app(
        state: Arc<EventStoreState>,
    ) -> impl tower::Service<
        Request<Body>,
        Response = Response<Body>,
        Error = std::convert::Infallible,
        Future: Send,
    > + Clone {
        let mgr = Arc::new(ConnectionManager::new());
        Router::new()
            .nest("/api/events", events_router(Arc::clone(&state)))
            .nest("/api/tasks", tasks_router(state))
            .layer(axum::Extension(mgr))
            .into_service::<Body>()
    }

    async fn json_body(resp: Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn make_envelope(task_id: TaskId, title: &str) -> EventEnvelope {
        EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id),
            project_id: "default".to_owned(),
            session_id: SessionId::new(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::TaskCreated {
                title: title.to_string(),
                description: "test description".to_string(),
                initial_stage: "backlog".to_string(),
                priority: Priority::P2,
                board_id: None,
            },
        }
    }

    // -- GET /api/events tests -----------------------------------------------

    #[tokio::test]
    async fn get_events_empty_returns_ok() {
        let store = test_store().await;
        let state = test_state(store);
        let app = test_app(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/events")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        assert!(body["events"].is_array());
        assert_eq!(body["events"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn get_events_with_task_id_filter() {
        let store = test_store().await;
        let task_id = TaskId::new();
        let other_task = TaskId::new();

        // Insert events for two different tasks.
        store
            .append(make_envelope(task_id.clone(), "Task A"))
            .await
            .unwrap();
        store
            .append(make_envelope(other_task, "Task B"))
            .await
            .unwrap();

        let state = test_state(store);
        let app = test_app(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri(&format!("/api/events?task_id={}", task_id.0))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        let events = body["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn get_events_invalid_task_id_returns_400() {
        let store = test_store().await;
        let state = test_state(store);
        let app = test_app(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/events?task_id=not-a-ulid")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_events_with_since_filter() {
        let store = test_store().await;
        let task_id = TaskId::new();
        store
            .append(make_envelope(task_id, "Old Task"))
            .await
            .unwrap();

        // Use a future timestamp — should return 0 events.
        // Use Zulu (UTC) format to avoid URL-encoding issues with '+'.
        let future = (Utc::now() + chrono::Duration::hours(1))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        let state = test_state(store);
        let app = test_app(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri(&format!("/api/events?since={}", future))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        assert_eq!(body["events"].as_array().unwrap().len(), 0);
    }

    // -- GET /api/events/:id tests -------------------------------------------

    #[tokio::test]
    async fn get_event_by_id_found() {
        let store = test_store().await;
        let task_id = TaskId::new();
        let envelope = make_envelope(task_id, "Test Task");
        let event_id = envelope.id.0.to_string();
        store.append(envelope).await.unwrap();

        let state = test_state(store);
        let app = test_app(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri(&format!("/api/events/{}", event_id))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_event_by_id_not_found() {
        let store = test_store().await;
        let state = test_state(store);
        let app = test_app(state);

        let fake_id = Ulid::new().to_string();
        let req = Request::builder()
            .method(Method::GET)
            .uri(&format!("/api/events/{}", fake_id))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_event_invalid_id_returns_400() {
        let store = test_store().await;
        let state = test_state(store);
        let app = test_app(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/events/not-a-ulid")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // -- POST /api/events tests ----------------------------------------------

    #[tokio::test]
    async fn post_event_appends_and_returns_created() {
        let store = test_store().await;
        let state = test_state(Arc::clone(&store));
        let app = test_app(state);

        let envelope = make_envelope(TaskId::new(), "Posted Task");
        let task_id = envelope.task_id.clone().unwrap();
        let body_json = serde_json::to_string(&envelope).unwrap();

        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/events")
            .header("content-type", "application/json")
            .body(Body::from(body_json))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Verify it was actually stored.
        let events = store.get_events_for_task(&task_id).await.unwrap();
        assert_eq!(events.len(), 1);
    }

    // -- GET /api/tasks tests ------------------------------------------------

    #[tokio::test]
    async fn get_tasks_empty_returns_ok() {
        let store = test_store().await;
        let state = test_state(store);
        let app = test_app(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/tasks")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        assert_eq!(body["tasks"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn get_tasks_groups_by_task_id() {
        let store = test_store().await;
        let task_a = TaskId::new();
        let task_b = TaskId::new();

        store
            .append(make_envelope(task_a.clone(), "Task A"))
            .await
            .unwrap();
        store
            .append(make_envelope(task_b.clone(), "Task B"))
            .await
            .unwrap();
        // Add a second event for task A.
        store
            .append(EventEnvelope {
                id: EventId::new(),
                task_id: Some(task_a.clone()),
                project_id: "default".to_owned(),
                session_id: SessionId::new(),
                timestamp: Utc::now(),
                caused_by: None,
                payload: DomainEvent::TaskBlocked {
                    reason: "blocked for test".to_string(),
                },
            })
            .await
            .unwrap();

        let state = test_state(store);
        let app = test_app(state);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/tasks")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        let tasks = body["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 2);

        // Find Task A and verify event count.
        let task_a_summary = tasks
            .iter()
            .find(|t| t["task_id"] == task_a.0.to_string())
            .unwrap();
        assert_eq!(task_a_summary["title"], "Task A");
        assert_eq!(task_a_summary["event_count"], 2);
    }
}
