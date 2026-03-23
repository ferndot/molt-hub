//! Axum HTTP handlers for the pipeline stages API.
//!
//! Routes:
//!   GET    /api/pipeline/stages      — return all pipeline stages as JSON
//!   PUT    /api/pipeline/stages      — replace all stages
//!   POST   /api/pipeline/stages      — add a single new stage
//!   DELETE /api/pipeline/stages/:id  — remove a stage by ID

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, instrument, warn};

// ---------------------------------------------------------------------------
// Response / request types
// ---------------------------------------------------------------------------

/// A single pipeline stage as returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StageResponse {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub wip_limit: Option<u32>,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub terminal: bool,
}

/// Top-level response for GET /api/pipeline/stages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StagesResponse {
    pub stages: Vec<StageResponse>,
}

/// YAML-serialisable pipeline config file format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineYaml {
    pub stages: Vec<StageResponse>,
}

/// Generic error response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ---------------------------------------------------------------------------
// PipelineConfigStore — in-memory + YAML persistence
// ---------------------------------------------------------------------------

/// Manages pipeline stage configuration with optional YAML file persistence.
pub struct PipelineConfigStore {
    stages: RwLock<Vec<StageResponse>>,
    config_path: Option<PathBuf>,
}

impl PipelineConfigStore {
    /// Create a store backed by a YAML file.
    ///
    /// If the file exists it is loaded; otherwise the default stages are
    /// written to disk.
    pub fn from_file(path: PathBuf) -> Self {
        let stages = match std::fs::read_to_string(&path) {
            Ok(contents) => {
                match serde_yaml::from_str::<PipelineYaml>(&contents) {
                    Ok(yaml) => yaml.stages,
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "failed to parse pipeline YAML, using defaults");
                        let defaults = Self::default_stages_vec();
                        // Attempt to overwrite the broken file with defaults.
                        let _ = Self::write_yaml(&path, &defaults);
                        defaults
                    }
                }
            }
            Err(_) => {
                // File doesn't exist — create it with defaults.
                let defaults = Self::default_stages_vec();
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = Self::write_yaml(&path, &defaults);
                defaults
            }
        };

        Self {
            stages: RwLock::new(stages),
            config_path: Some(path),
        }
    }

    /// Create a store with the default stages, no file backing (for tests).
    pub fn in_memory() -> Self {
        Self {
            stages: RwLock::new(Self::default_stages_vec()),
            config_path: None,
        }
    }

    /// Create a store from an explicit list, no file backing (for tests).
    pub fn in_memory_with(stages: Vec<StageResponse>) -> Self {
        Self {
            stages: RwLock::new(stages),
            config_path: None,
        }
    }

    /// Resolve the default config file location: `~/.config/molt-hub/pipeline.yaml`.
    pub fn default_config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("molt-hub").join("pipeline.yaml"))
    }

    /// Load from the default config path, or fall back to in-memory defaults.
    pub fn load_default() -> Self {
        match Self::default_config_path() {
            Some(path) => Self::from_file(path),
            None => Self::in_memory(),
        }
    }

    // -- reads ---------------------------------------------------------------

    pub async fn get_stages(&self) -> Vec<StageResponse> {
        self.stages.read().await.clone()
    }

    // -- writes --------------------------------------------------------------

    /// Replace all stages atomically.
    pub async fn set_stages(&self, stages: Vec<StageResponse>) -> Result<(), String> {
        if stages.is_empty() {
            return Err("pipeline must have at least one stage".into());
        }
        // Check for duplicate IDs.
        let mut seen = std::collections::HashSet::new();
        for s in &stages {
            if !seen.insert(&s.id) {
                return Err(format!("duplicate stage id: {}", s.id));
            }
        }
        let mut guard = self.stages.write().await;
        *guard = stages;
        self.persist(&guard)?;
        Ok(())
    }

    /// Add a single stage at the end.
    pub async fn add_stage(&self, stage: StageResponse) -> Result<(), String> {
        let mut guard = self.stages.write().await;
        if guard.iter().any(|s| s.id == stage.id) {
            return Err(format!("stage with id '{}' already exists", stage.id));
        }
        guard.push(stage);
        self.persist(&guard)?;
        Ok(())
    }

    /// Remove a stage by ID.
    pub async fn remove_stage(&self, id: &str) -> Result<(), String> {
        let mut guard = self.stages.write().await;
        let before = guard.len();
        guard.retain(|s| s.id != id);
        if guard.len() == before {
            return Err(format!("stage with id '{}' not found", id));
        }
        if guard.is_empty() {
            return Err("cannot remove last stage".into());
        }
        self.persist(&guard)?;
        Ok(())
    }

    // -- persistence ---------------------------------------------------------

    fn persist(&self, stages: &[StageResponse]) -> Result<(), String> {
        if let Some(ref path) = self.config_path {
            Self::write_yaml(path, stages)
        } else {
            Ok(())
        }
    }

    fn write_yaml(path: &std::path::Path, stages: &[StageResponse]) -> Result<(), String> {
        let yaml = PipelineYaml {
            stages: stages.to_vec(),
        };
        let content =
            serde_yaml::to_string(&yaml).map_err(|e| format!("yaml serialize: {e}"))?;
        std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))
    }

    fn default_stages_vec() -> Vec<StageResponse> {
        vec![
            StageResponse {
                id: "backlog".into(),
                label: "Backlog".into(),
                wip_limit: None,
                requires_approval: false,
                timeout_seconds: None,
                terminal: false,
            },
            StageResponse {
                id: "in-progress".into(),
                label: "In Progress".into(),
                wip_limit: Some(5),
                requires_approval: false,
                timeout_seconds: None,
                terminal: false,
            },
            StageResponse {
                id: "code-review".into(),
                label: "Code Review".into(),
                wip_limit: None,
                requires_approval: true,
                timeout_seconds: None,
                terminal: false,
            },
            StageResponse {
                id: "testing".into(),
                label: "Testing".into(),
                wip_limit: None,
                requires_approval: false,
                timeout_seconds: None,
                terminal: false,
            },
            StageResponse {
                id: "deployed".into(),
                label: "Deployed".into(),
                wip_limit: None,
                requires_approval: false,
                timeout_seconds: None,
                terminal: true,
            },
        ]
    }
}

// ---------------------------------------------------------------------------
// Backward compat: PipelineState type alias
// ---------------------------------------------------------------------------

/// Legacy alias kept so `serve.rs` can use the same construction pattern.
pub type PipelineState = PipelineConfigStore;

impl PipelineState {
    /// Create state with the default pipeline stages (in-memory, no file).
    pub fn default_stages() -> Self {
        Self::in_memory()
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/pipeline/stages
#[instrument(skip_all)]
pub async fn get_stages(State(state): State<Arc<PipelineConfigStore>>) -> impl IntoResponse {
    let stages = state.get_stages().await;
    (StatusCode::OK, Json(StagesResponse { stages }))
}

/// PUT /api/pipeline/stages — replace all stages
#[instrument(skip_all)]
pub async fn put_stages(
    State(state): State<Arc<PipelineConfigStore>>,
    Json(body): Json<StagesResponse>,
) -> impl IntoResponse {
    match state.set_stages(body.stages).await {
        Ok(()) => {
            let stages = state.get_stages().await;
            info!(count = stages.len(), "pipeline stages updated");
            (StatusCode::OK, Json(StagesResponse { stages })).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: e }),
        )
            .into_response(),
    }
}

/// POST /api/pipeline/stages — add a single stage
#[instrument(skip_all)]
pub async fn post_stage(
    State(state): State<Arc<PipelineConfigStore>>,
    Json(stage): Json<StageResponse>,
) -> impl IntoResponse {
    match state.add_stage(stage).await {
        Ok(()) => {
            let stages = state.get_stages().await;
            info!(count = stages.len(), "pipeline stage added");
            (StatusCode::CREATED, Json(StagesResponse { stages })).into_response()
        }
        Err(e) => (
            StatusCode::CONFLICT,
            Json(ErrorResponse { error: e }),
        )
            .into_response(),
    }
}

/// DELETE /api/pipeline/stages/:id — remove a stage
#[instrument(skip_all)]
pub async fn delete_stage(
    State(state): State<Arc<PipelineConfigStore>>,
    Path(stage_id): Path<String>,
) -> impl IntoResponse {
    match state.remove_stage(&stage_id).await {
        Ok(()) => {
            let stages = state.get_stages().await;
            info!(stage_id, "pipeline stage removed");
            (StatusCode::OK, Json(StagesResponse { stages })).into_response()
        }
        Err(e) => (
            StatusCode::GONE,
            Json(ErrorResponse { error: e }),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

use axum::{routing::get, Router};

/// Build the `/api/pipeline` sub-router.
pub fn pipeline_router(state: Arc<PipelineConfigStore>) -> Router {
    Router::new()
        .route(
            "/stages",
            get(get_stages).put(put_stages).post(post_stage),
        )
        .route(
            "/stages/:id",
            axum::routing::delete(delete_stage),
        )
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
        let state = Arc::new(PipelineConfigStore::in_memory());
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

    // -- GET tests -----------------------------------------------------------

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

        assert!(body["stages"].is_array(), "expected stages array");
        let stages = body["stages"].as_array().unwrap();
        assert_eq!(stages.len(), 5);
        assert_eq!(stages[0]["id"], "backlog");
        assert_eq!(stages[0]["label"], "Backlog");
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
        let state = Arc::new(PipelineConfigStore::in_memory_with(vec![
            StageResponse {
                id: "todo".into(),
                label: "To Do".into(),
                wip_limit: Some(5),
                requires_approval: false,
                timeout_seconds: None,
                terminal: false,
            },
            StageResponse {
                id: "done".into(),
                label: "Done".into(),
                wip_limit: None,
                requires_approval: false,
                timeout_seconds: None,
                terminal: true,
            },
        ]));
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
        let state = PipelineConfigStore::in_memory();
        let stages = state.get_stages().await;
        assert_eq!(stages.len(), 5);
        let ids: Vec<&str> = stages.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["backlog", "in-progress", "code-review", "testing", "deployed"]
        );
    }

    // -- PUT tests -----------------------------------------------------------

    #[tokio::test]
    async fn put_stages_replaces_all() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(Arc::clone(&state)))
            .into_service::<Body>();

        let new_stages = serde_json::json!({
            "stages": [
                {"id": "a", "label": "A", "wip_limit": null, "requires_approval": false, "timeout_seconds": null, "terminal": false},
                {"id": "b", "label": "B", "wip_limit": 3, "requires_approval": false, "timeout_seconds": null, "terminal": true}
            ]
        });

        let req = Request::builder()
            .method(Method::PUT)
            .uri("/api/pipeline/stages")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&new_stages).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let stages = state.get_stages().await;
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].id, "a");
        assert_eq!(stages[1].id, "b");
    }

    #[tokio::test]
    async fn put_stages_rejects_empty() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(state))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::PUT)
            .uri("/api/pipeline/stages")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"stages":[]}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn put_stages_rejects_duplicates() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(state))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::PUT)
            .uri("/api/pipeline/stages")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"stages":[{"id":"a","label":"A"},{"id":"a","label":"A2"}]}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // -- POST tests ----------------------------------------------------------

    #[tokio::test]
    async fn post_stage_adds_one() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(Arc::clone(&state)))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/pipeline/stages")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"id":"qa","label":"QA","wip_limit":2,"requires_approval":false,"timeout_seconds":null,"terminal":false}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let stages = state.get_stages().await;
        assert_eq!(stages.len(), 6);
        assert_eq!(stages.last().unwrap().id, "qa");
    }

    #[tokio::test]
    async fn post_stage_rejects_duplicate_id() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(state))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/pipeline/stages")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"id":"backlog","label":"Dupe"}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    // -- DELETE tests --------------------------------------------------------

    #[tokio::test]
    async fn delete_stage_removes_one() {
        // Test the store directly, then verify via HTTP.
        let store = Arc::new(PipelineConfigStore::in_memory());
        assert_eq!(store.get_stages().await.len(), 5);

        store.remove_stage("testing").await.unwrap();
        let stages = store.get_stages().await;
        assert_eq!(stages.len(), 4);
        assert!(!stages.iter().any(|s| s.id == "testing"));
    }

    #[tokio::test]
    async fn delete_stage_via_http() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = pipeline_router(Arc::clone(&state));

        let req = Request::builder()
            .method(Method::DELETE)
            .uri("/stages/testing")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let stages = state.get_stages().await;
        assert_eq!(stages.len(), 4);
        assert!(!stages.iter().any(|s| s.id == "testing"));
    }

    #[tokio::test]
    async fn delete_stage_not_found() {
        let store = Arc::new(PipelineConfigStore::in_memory());
        let err = store.remove_stage("nonexistent").await;
        assert!(err.is_err());
    }

    // -- YAML persistence tests ----------------------------------------------

    #[tokio::test]
    async fn yaml_roundtrip_write_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pipeline.yaml");

        // Create store — should write defaults.
        let store = PipelineConfigStore::from_file(path.clone());
        assert_eq!(store.get_stages().await.len(), 5);

        // Add a stage.
        store
            .add_stage(StageResponse {
                id: "extra".into(),
                label: "Extra".into(),
                wip_limit: Some(10),
                requires_approval: true,
                timeout_seconds: Some(3600),
                terminal: false,
            })
            .await
            .unwrap();

        // Reload from disk.
        let store2 = PipelineConfigStore::from_file(path);
        let stages = store2.get_stages().await;
        assert_eq!(stages.len(), 6);

        let extra = stages.iter().find(|s| s.id == "extra").unwrap();
        assert_eq!(extra.label, "Extra");
        assert_eq!(extra.wip_limit, Some(10));
        assert!(extra.requires_approval);
        assert_eq!(extra.timeout_seconds, Some(3600));
    }

    #[tokio::test]
    async fn yaml_roundtrip_remove_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pipeline.yaml");

        let store = PipelineConfigStore::from_file(path.clone());
        store.remove_stage("testing").await.unwrap();

        let store2 = PipelineConfigStore::from_file(path);
        let stages = store2.get_stages().await;
        assert_eq!(stages.len(), 4);
        assert!(!stages.iter().any(|s| s.id == "testing"));
    }

    #[tokio::test]
    async fn yaml_roundtrip_replace_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pipeline.yaml");

        let store = PipelineConfigStore::from_file(path.clone());
        store
            .set_stages(vec![
                StageResponse {
                    id: "only".into(),
                    label: "Only Stage".into(),
                    wip_limit: None,
                    requires_approval: false,
                    timeout_seconds: None,
                    terminal: true,
                },
            ])
            .await
            .unwrap();

        let store2 = PipelineConfigStore::from_file(path);
        let stages = store2.get_stages().await;
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].id, "only");
    }

    #[tokio::test]
    async fn from_file_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("pipeline.yaml");

        let store = PipelineConfigStore::from_file(path.clone());
        assert_eq!(store.get_stages().await.len(), 5);
        assert!(path.exists());
    }

    #[tokio::test]
    async fn from_file_handles_corrupt_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pipeline.yaml");
        std::fs::write(&path, "this is not valid yaml: [[[").unwrap();

        let store = PipelineConfigStore::from_file(path);
        // Should fall back to defaults.
        assert_eq!(store.get_stages().await.len(), 5);
    }
}
