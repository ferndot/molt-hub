//! Axum HTTP handlers for the pipeline stages API.
//!
//! Routes:
//!   GET /api/pipeline/stages — return all pipeline stages as JSON

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// A single pipeline stage as returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StageResponse {
    pub id: String,
    pub label: String,
    pub wip_limit: Option<u32>,
}

/// Top-level response for GET /api/pipeline/stages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StagesResponse {
    pub stages: Vec<StageResponse>,
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// State shared across pipeline handlers.
///
/// Holds the list of configured stages. In a full implementation this would
/// be loaded from a YAML config file or database; for now it provides
/// sensible defaults that match the board's hardcoded stages.
pub struct PipelineState {
    pub stages: Vec<StageResponse>,
}

impl PipelineState {
    /// Create state with the default pipeline stages.
    pub fn default_stages() -> Self {
        Self {
            stages: vec![
                StageResponse {
                    id: "backlog".into(),
                    label: "Backlog".into(),
                    wip_limit: None,
                },
                StageResponse {
                    id: "in-progress".into(),
                    label: "In Progress".into(),
                    wip_limit: None,
                },
                StageResponse {
                    id: "code-review".into(),
                    label: "Code Review".into(),
                    wip_limit: None,
                },
                StageResponse {
                    id: "testing".into(),
                    label: "Testing".into(),
                    wip_limit: None,
                },
                StageResponse {
                    id: "deployed".into(),
                    label: "Deployed".into(),
                    wip_limit: None,
                },
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Handler: GET /api/pipeline/stages
// ---------------------------------------------------------------------------

/// Return all pipeline stages as JSON.
///
/// Response shape: `{ "stages": [{ "id": "...", "label": "...", "wip_limit": null }, ...] }`
#[instrument(skip_all)]
pub async fn get_stages(State(state): State<Arc<PipelineState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(StagesResponse {
            stages: state.stages.clone(),
        }),
    )
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

use axum::{routing::get, Router};

/// Build the `/api/pipeline` sub-router.
pub fn pipeline_router(state: Arc<PipelineState>) -> Router {
    Router::new()
        .route("/stages", get(get_stages))
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
    use tower::util::ServiceExt;

    fn test_app() -> impl tower::Service<
        Request<Body>,
        Response = Response<Body>,
        Error = std::convert::Infallible,
        Future: Send,
    > + Clone {
        let state = Arc::new(PipelineState::default_stages());
        Router::new()
            .nest("/api/pipeline", pipeline_router(state))
            .into_service::<Body>()
    }

    async fn json_body(resp: Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn get_stages_returns_ok() {
        let app = test_app();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/pipeline/stages")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_stages_returns_expected_shape() {
        let app = test_app();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/pipeline/stages")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = json_body(resp).await;

        // Verify top-level shape
        assert!(body["stages"].is_array(), "expected stages array");
        let stages = body["stages"].as_array().unwrap();
        assert_eq!(stages.len(), 5);

        // Verify first stage
        assert_eq!(stages[0]["id"], "backlog");
        assert_eq!(stages[0]["label"], "Backlog");
        assert!(stages[0]["wip_limit"].is_null());
    }

    #[tokio::test]
    async fn get_stages_all_stages_have_required_fields() {
        let app = test_app();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/pipeline/stages")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = json_body(resp).await;
        let stages = body["stages"].as_array().unwrap();

        for stage in stages {
            assert!(stage["id"].is_string(), "stage missing id");
            assert!(stage["label"].is_string(), "stage missing label");
            assert!(
                stage.get("wip_limit").is_some(),
                "stage missing wip_limit field"
            );
        }
    }

    #[tokio::test]
    async fn get_stages_custom_state() {
        let state = Arc::new(PipelineState {
            stages: vec![
                StageResponse {
                    id: "todo".into(),
                    label: "To Do".into(),
                    wip_limit: Some(5),
                },
                StageResponse {
                    id: "done".into(),
                    label: "Done".into(),
                    wip_limit: None,
                },
            ],
        });
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(state))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/pipeline/stages")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = json_body(resp).await;
        let stages = body["stages"].as_array().unwrap();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0]["id"], "todo");
        assert_eq!(stages[0]["wip_limit"], 5);
        assert_eq!(stages[1]["id"], "done");
        assert!(stages[1]["wip_limit"].is_null());
    }

    #[tokio::test]
    async fn pipeline_state_default_stages_has_five_entries() {
        let state = PipelineState::default_stages();
        assert_eq!(state.stages.len(), 5);
        let ids: Vec<&str> = state.stages.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["backlog", "in-progress", "code-review", "testing", "deployed"]
        );
    }
}
