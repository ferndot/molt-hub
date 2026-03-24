//! Axum HTTP handlers for the events and tasks API.
//!
//! Routes:
//!   GET    /api/events            — list events (query: task_id, since)
//!   GET    /api/events/:id        — get a single event by ID
//!   POST   /api/events            — append a new event
//!   GET    /api/tasks             — list tasks derived from events
//!   POST   /api/tasks/create      — create a manual task (`TaskCreated`)

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
use molt_hub_core::events::{EventStore, EventStoreError, SqliteEventStore};
use molt_hub_core::model::{EventId, Priority, SessionId, TaskId};

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

/// Build the `/api/tasks` sub-router.
pub fn tasks_router(state: Arc<EventStoreState>) -> Router {
    Router::new()
        .route("/", get(list_tasks))
        .route("/create", post(create_task))
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
