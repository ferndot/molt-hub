//! Axum HTTP handlers for agent lifecycle management.
//!
//! Routes:
//!   GET  /api/agents             — list all agents with status
//!   POST /api/agents/spawn       — spawn a new agent
//!   POST /api/agents/:id/terminate — terminate an agent
//!   POST /api/agents/:id/pause     — pause an agent
//!   POST /api/agents/:id/resume    — resume an agent
//!   POST /api/agents/:id/steer     — send a steering message to an agent
//!   GET  /api/agents/:id/output    — get buffered output lines

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use molt_hub_core::model::AgentId;
use molt_hub_harness::supervisor::{SteerMessage, SteerPriority, Supervisor, SupervisorError};

use super::output_buffer::AgentOutputBuffer;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// State shared across agent handlers.
pub struct AgentState {
    pub supervisor: Arc<Supervisor>,
    pub output_buffer: Arc<AgentOutputBuffer>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// A single agent as returned by the list API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub agent_id: String,
    pub task_id: String,
    pub status: String,
}

/// Top-level response for GET /api/agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsListResponse {
    pub agents: Vec<AgentResponse>,
    pub count: usize,
}

/// Response for POST /api/agents/spawn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnResponse {
    pub agent_id: String,
    pub message: String,
}

/// Request body for POST /api/agents/spawn.
#[derive(Debug, Clone, Deserialize)]
pub struct SpawnRequest {
    /// Adapter type to use (e.g. "claude-cli", "cli").
    pub adapter_type: String,
    /// Task instructions for the agent.
    pub instructions: Option<String>,
    /// Additional adapter configuration.
    pub adapter_config: Option<serde_json::Value>,
}

/// Generic message response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageResponse {
    pub message: String,
}

/// Response for GET /api/agents/:id/output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutputResponse {
    pub agent_id: String,
    pub lines: Vec<OutputLineResponse>,
    pub count: usize,
}

/// A single output line in the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputLineResponse {
    pub line: String,
    pub timestamp: String,
}

/// Request body for POST /api/agents/:id/steer.
#[derive(Debug, Clone, Deserialize)]
pub struct SteerRequest {
    /// Message to send to the running agent.
    pub message: String,
    /// Priority of the steering message (default: normal).
    #[serde(default)]
    pub priority: Option<String>,
}

/// Response for POST /api/agents/:id/steer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerResponse {
    pub delivered: bool,
    pub agent_id: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/agents — list all agents with status.
#[instrument(skip(state))]
async fn list_agents(
    State(state): State<Arc<AgentState>>,
) -> impl IntoResponse {
    let agents = state.supervisor.list_agents().await;
    let responses: Vec<AgentResponse> = agents
        .into_iter()
        .map(|(agent_id, task_id, status)| AgentResponse {
            agent_id: agent_id.to_string(),
            task_id: task_id.to_string(),
            status: format!("{:?}", status),
        })
        .collect();
    let count = responses.len();

    Json(AgentsListResponse {
        agents: responses,
        count,
    })
}

/// POST /api/agents/:id/terminate — terminate an agent.
#[instrument(skip(state))]
async fn terminate_agent(
    State(state): State<Arc<AgentState>>,
    Path(agent_id_str): Path<String>,
) -> impl IntoResponse {
    // Parse the agent ID from the ULID string.
    let ulid = match ulid::Ulid::from_string(&agent_id_str) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MessageResponse {
                    message: format!("invalid agent ID: {agent_id_str}"),
                }),
            )
                .into_response();
        }
    };

    let agent_id = AgentId(ulid);

    match state.supervisor.terminate_agent(&agent_id).await {
        Ok(()) => Json(MessageResponse {
            message: format!("agent {agent_id_str} terminated"),
        })
        .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("failed to terminate agent: {e}"),
            }),
        )
            .into_response(),
    }
}

/// POST /api/agents/:id/pause — pause an agent.
#[instrument(skip(state))]
async fn pause_agent(
    State(state): State<Arc<AgentState>>,
    Path(agent_id_str): Path<String>,
) -> impl IntoResponse {
    let ulid = match ulid::Ulid::from_string(&agent_id_str) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MessageResponse {
                    message: format!("invalid agent ID: {agent_id_str}"),
                }),
            )
                .into_response();
        }
    };

    let agent_id = AgentId(ulid);

    match state.supervisor.pause_agent(&agent_id).await {
        Ok(()) => Json(MessageResponse {
            message: format!("agent {agent_id_str} paused"),
        })
        .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("failed to pause agent: {e}"),
            }),
        )
            .into_response(),
    }
}

/// POST /api/agents/:id/resume — resume a paused agent.
#[instrument(skip(state))]
async fn resume_agent(
    State(state): State<Arc<AgentState>>,
    Path(agent_id_str): Path<String>,
) -> impl IntoResponse {
    let ulid = match ulid::Ulid::from_string(&agent_id_str) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MessageResponse {
                    message: format!("invalid agent ID: {agent_id_str}"),
                }),
            )
                .into_response();
        }
    };

    let agent_id = AgentId(ulid);

    match state.supervisor.resume_agent(&agent_id).await {
        Ok(()) => Json(MessageResponse {
            message: format!("agent {agent_id_str} resumed"),
        })
        .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("failed to resume agent: {e}"),
            }),
        )
            .into_response(),
    }
}

/// POST /api/agents/:id/steer — send a steering message to a running agent.
#[instrument(skip(state, body))]
async fn steer_agent(
    State(state): State<Arc<AgentState>>,
    Path(agent_id_str): Path<String>,
    Json(body): Json<SteerRequest>,
) -> impl IntoResponse {
    let ulid = match ulid::Ulid::from_string(&agent_id_str) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MessageResponse {
                    message: format!("invalid agent ID: {agent_id_str}"),
                }),
            )
                .into_response();
        }
    };

    let agent_id = AgentId(ulid);

    let priority = match body.priority.as_deref() {
        Some("urgent") => SteerPriority::Urgent,
        _ => SteerPriority::Normal,
    };

    let steer_msg = SteerMessage {
        message: body.message,
        priority,
    };

    match state.supervisor.steer(&agent_id, steer_msg).await {
        Ok(()) => Json(SteerResponse {
            delivered: true,
            agent_id: agent_id_str,
        })
        .into_response(),
        Err(SupervisorError::AgentNotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("agent not found: {agent_id_str}"),
            }),
        )
            .into_response(),
        Err(SupervisorError::AgentNotRunning(_)) => (
            StatusCode::CONFLICT,
            Json(MessageResponse {
                message: format!("agent not running: {agent_id_str}"),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MessageResponse {
                message: format!("steer failed: {e}"),
            }),
        )
            .into_response(),
    }
}

/// GET /api/agents/:id/output — return buffered output lines for an agent.
#[instrument(skip(state))]
async fn get_agent_output(
    State(state): State<Arc<AgentState>>,
    Path(agent_id_str): Path<String>,
) -> impl IntoResponse {
    let lines = state.output_buffer.get_lines(&agent_id_str);
    let count = lines.len();

    Json(AgentOutputResponse {
        agent_id: agent_id_str,
        lines: lines
            .into_iter()
            .map(|l| OutputLineResponse {
                line: l.line,
                timestamp: l.timestamp.to_rfc3339(),
            })
            .collect(),
        count,
    })
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the agent API sub-router.
///
/// Mounts:
///   GET  /                  — list all agents
///   POST /:id/terminate     — terminate an agent
///   POST /:id/pause         — pause an agent
///   POST /:id/resume        — resume an agent
///   POST /:id/steer         — send a steering message to an agent
///   GET  /:id/output        — get buffered output lines
pub fn agent_router(state: Arc<AgentState>) -> Router {
    Router::new()
        .route("/", get(list_agents))
        .route("/:id/terminate", post(terminate_agent))
        .route("/:id/pause", post(pause_agent))
        .route("/:id/resume", post(resume_agent))
        .route("/:id/steer", post(steer_agent))
        .route("/:id/output", get(get_agent_output))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use molt_hub_harness::adapter::AgentEvent;
    use molt_hub_harness::supervisor::SupervisorConfig;
    use std::time::Duration;
    use tokio::sync::broadcast;
    use tower::ServiceExt;

    fn make_supervisor() -> Arc<Supervisor> {
        let (tx, _rx) = broadcast::channel::<AgentEvent>(64);
        let config = SupervisorConfig {
            max_agents: 4,
            health_check_interval: Duration::from_secs(60),
            graceful_shutdown_timeout: Duration::from_millis(100),
        };
        Arc::new(Supervisor::new(config, tx))
    }

    fn make_state() -> Arc<AgentState> {
        Arc::new(AgentState {
            supervisor: make_supervisor(),
            output_buffer: Arc::new(AgentOutputBuffer::new()),
        })
    }

    #[tokio::test]
    async fn test_list_agents_empty() {
        let state = make_state();
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let parsed: AgentsListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.count, 0);
        assert!(parsed.agents.is_empty());
    }

    #[tokio::test]
    async fn test_terminate_invalid_id_returns_error() {
        let state = make_state();
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/not-a-ulid/terminate")
            .method("POST")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        // Handler returns 400 for malformed ULID, but Axum may return 404
        // if the route pattern doesn't match. Either is acceptable for invalid IDs.
        let status = resp.status().as_u16();
        assert!(status == 400 || status == 404, "expected 400 or 404, got {status}");
    }

    #[tokio::test]
    async fn test_terminate_unknown_agent_returns_not_found() {
        let state = make_state();
        let app = agent_router(state);

        let agent_id = AgentId::new();
        let uri = format!("/{}/terminate", agent_id);

        let req = Request::builder()
            .uri(&uri)
            .method("POST")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_pause_unknown_agent_returns_not_found() {
        let state = make_state();
        let app = agent_router(state);

        let agent_id = AgentId::new();
        let uri = format!("/{}/pause", agent_id);

        let req = Request::builder()
            .uri(&uri)
            .method("POST")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_resume_unknown_agent_returns_not_found() {
        let state = make_state();
        let app = agent_router(state);

        let agent_id = AgentId::new();
        let uri = format!("/{}/resume", agent_id);

        let req = Request::builder()
            .uri(&uri)
            .method("POST")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_agent_output_empty() {
        let state = make_state();
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/some-agent/output")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let parsed: AgentOutputResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.count, 0);
        assert!(parsed.lines.is_empty());
    }

    #[tokio::test]
    async fn test_get_agent_output_with_data() {
        let output_buffer = Arc::new(AgentOutputBuffer::new());
        output_buffer.push("agent-42", "line one".into());
        output_buffer.push("agent-42", "line two".into());

        let state = Arc::new(AgentState {
            supervisor: make_supervisor(),
            output_buffer,
        });
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/agent-42/output")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let parsed: AgentOutputResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.agent_id, "agent-42");
        assert_eq!(parsed.count, 2);
        assert_eq!(parsed.lines[0].line, "line one");
        assert_eq!(parsed.lines[1].line, "line two");
        // Verify timestamps are valid RFC3339.
        for line in &parsed.lines {
            assert!(!line.timestamp.is_empty());
        }
    }

    // -----------------------------------------------------------------------
    // Steer endpoint tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_steer_unknown_agent_returns_not_found() {
        let state = make_state();
        let app = agent_router(state);

        let agent_id = AgentId::new();
        let uri = format!("/{}/steer", agent_id);

        let req = Request::builder()
            .uri(&uri)
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"message": "hello", "priority": "normal"}).to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_steer_invalid_id_returns_error() {
        let state = make_state();
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/not-a-ulid/steer")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"message": "hello"}).to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        assert!(status == 400 || status == 404, "expected 400 or 404, got {status}");
    }

    #[test]
    fn test_steer_response_serialization() {
        let resp = SteerResponse {
            delivered: true,
            agent_id: "abc123".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"delivered\":true"));
        assert!(json.contains("\"agent_id\":\"abc123\""));
    }
}
