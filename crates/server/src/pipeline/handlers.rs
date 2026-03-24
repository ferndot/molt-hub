//! Axum HTTP handlers for the pipeline stages API.
//!
//! Persistence and responses align with [`molt_hub_core::config::PipelineConfig`]: `pipeline.yaml`
//! stores the full config (stages with hooks, `columns`, etc.). Legacy files that contain only
//! a top-level `stages` list are still loaded and migrated in memory.
//!
//! Routes:
//!   GET    /api/pipeline/stages      — return stages and column definitions as JSON
//!   PUT    /api/pipeline/stages      — replace stages (optional `columns` replaces board columns)
//!   POST   /api/pipeline/stages      — add a single new stage
//!   PATCH  /api/pipeline/stages/:id  — partially update a single stage
//!   DELETE /api/pipeline/stages/:id  — remove a stage by ID

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use molt_hub_core::config::{
    ColumnConfig, HookDefinition, PipelineConfig, StageDefinition, TransitionDefinition,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, instrument, warn};

// ---------------------------------------------------------------------------
// Response / request types
// ---------------------------------------------------------------------------

/// A single pipeline stage as returned by the API (aligned with [`StageDefinition`], using `id`/`label`).
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
    /// Hex color for the column header (e.g. "#6366f1").
    #[serde(default)]
    pub color: Option<String>,
    /// Display order (0-indexed).
    #[serde(default)]
    pub order: u32,
    #[serde(default)]
    pub hooks: Vec<HookDefinition>,
    #[serde(default)]
    pub transition_rules: Vec<TransitionDefinition>,
    pub instructions: Option<String>,
    pub instructions_template: Option<String>,
    #[serde(default)]
    pub approvers: Vec<String>,
}

pub(crate) fn stage_to_response(s: &StageDefinition) -> StageResponse {
    StageResponse {
        id: s.name.clone(),
        label: s.label.clone().unwrap_or_else(|| s.name.clone()),
        wip_limit: s.wip_limit,
        requires_approval: s.requires_approval,
        timeout_seconds: s.timeout_seconds,
        terminal: s.terminal,
        color: s.color.clone(),
        order: s.order,
        hooks: s.hooks.clone(),
        transition_rules: s.transition_rules.clone(),
        instructions: s.instructions.clone(),
        instructions_template: s.instructions_template.clone(),
        approvers: s.approvers.clone(),
    }
}

pub(crate) fn stage_from_response(s: StageResponse) -> Result<StageDefinition, String> {
    let id = s.id.trim();
    if id.is_empty() {
        return Err("stage id must be non-empty".into());
    }
    let label = if s.label.trim().is_empty() || s.label == id {
        None
    } else {
        Some(s.label)
    };
    Ok(StageDefinition {
        name: id.to_string(),
        label,
        instructions: s.instructions,
        instructions_template: s.instructions_template,
        requires_approval: s.requires_approval,
        approvers: s.approvers,
        timeout_seconds: s.timeout_seconds,
        terminal: s.terminal,
        hooks: s.hooks,
        transition_rules: s.transition_rules,
        color: s.color,
        order: s.order,
        wip_limit: s.wip_limit,
    })
}

fn validate_pipeline_config(cfg: &PipelineConfig) -> Result<(), String> {
    let errs = cfg.validate();
    if errs.is_empty() {
        return Ok(());
    }
    Err(errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; "))
}

/// Partial update payload for PATCH /api/pipeline/stages/:id.
///
/// All fields are optional; only supplied fields are applied.
/// For nullable fields (`wip_limit`, `timeout_seconds`, `color`), the outer
/// `Option` distinguishes "not supplied" (`None`) from "explicitly set to null"
/// (`Some(None)`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StagePatch {
    pub label: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub wip_limit: Option<Option<u32>>,
    pub requires_approval: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub timeout_seconds: Option<Option<u64>>,
    pub terminal: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub color: Option<Option<String>>,
    pub order: Option<u32>,
}

/// Deserialize a double-option: absent key → `None`, explicit `null` → `Some(None)`,
/// value present → `Some(Some(v))`.
fn deserialize_double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    // If serde calls this function the key was present in JSON.
    // `Option::deserialize` yields `None` for `null`, `Some(v)` for a value.
    Ok(Some(Option::deserialize(deserializer)?))
}

/// Top-level body for GET/PUT `/api/pipeline/stages`.
///
/// `columns`: on PUT, omit or `null` leaves existing column definitions unchanged;
/// a JSON array (including `[]`) replaces them. Always populated on GET.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StagesResponse {
    pub stages: Vec<StageResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<ColumnConfig>>,
}

/// Legacy `pipeline.yaml` shape: only `stages` (pre–full [`PipelineConfig`]).
#[derive(Debug, Deserialize)]
struct LegacyPipelineYamlV1 {
    stages: Vec<StageResponse>,
}

/// Generic error response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ---------------------------------------------------------------------------
// PipelineConfigStore — in-memory + YAML persistence
// ---------------------------------------------------------------------------

/// Manages full [`PipelineConfig`] (stages, hooks, columns) with optional YAML persistence.
pub struct PipelineConfigStore {
    config: RwLock<PipelineConfig>,
    config_path: Option<PathBuf>,
}

fn load_pipeline_yaml(contents: &str) -> Result<PipelineConfig, String> {
    if let Ok(cfg) = serde_yaml::from_str::<PipelineConfig>(contents) {
        validate_pipeline_config(&cfg)?;
        return Ok(cfg);
    }
    if let Ok(legacy) = serde_yaml::from_str::<LegacyPipelineYamlV1>(contents) {
        let stages: Result<Vec<StageDefinition>, String> =
            legacy.stages.into_iter().map(stage_from_response).collect();
        let stages = stages?;
        let mut cfg = PipelineConfig::board_defaults();
        cfg.stages = stages;
        validate_pipeline_config(&cfg)?;
        return Ok(cfg);
    }
    Err("unrecognized pipeline YAML shape".into())
}

impl PipelineConfigStore {
    /// Create a store backed by a YAML file.
    ///
    /// If the file exists it is loaded; otherwise the default config is
    /// written to disk.
    pub fn from_file(path: PathBuf) -> Self {
        let config = match std::fs::read_to_string(&path) {
            Ok(contents) => match load_pipeline_yaml(&contents) {
                Ok(cfg) => cfg,
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "failed to parse pipeline YAML, using defaults");
                    let defaults = PipelineConfig::board_defaults();
                    let _ = Self::write_yaml(&path, &defaults);
                    defaults
                }
            },
            Err(_) => {
                let defaults = PipelineConfig::board_defaults();
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = Self::write_yaml(&path, &defaults);
                defaults
            }
        };

        Self {
            config: RwLock::new(config),
            config_path: Some(path),
        }
    }

    /// Create a store with the default pipeline, no file backing (for tests).
    pub fn in_memory() -> Self {
        Self {
            config: RwLock::new(PipelineConfig::board_defaults()),
            config_path: None,
        }
    }

    /// Create a store from explicit API stages, no file backing (for tests).
    pub fn in_memory_with(stages: Vec<StageResponse>) -> Self {
        let stages: Vec<StageDefinition> = stages
            .into_iter()
            .map(|s| stage_from_response(s).expect("valid test stage"))
            .collect();
        let cfg = PipelineConfig {
            name: "default".into(),
            description: None,
            version: 1,
            stages,
            integrations: vec![],
            columns: vec![],
        };
        Self {
            config: RwLock::new(cfg),
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
        self.config
            .read()
            .await
            .stages
            .iter()
            .map(stage_to_response)
            .collect()
    }

    /// Full stages response including column definitions (for GET/PUT bodies).
    pub async fn get_stages_response(&self) -> StagesResponse {
        let cfg = self.config.read().await;
        StagesResponse {
            stages: cfg.stages.iter().map(stage_to_response).collect(),
            columns: Some(cfg.columns.clone()),
        }
    }

    /// Clone the full in-memory pipeline config (for seeding another store).
    pub async fn snapshot_config(&self) -> PipelineConfig {
        self.config.read().await.clone()
    }

    /// In-memory store from an explicit [`PipelineConfig`] (no file backing).
    pub fn from_pipeline_config(cfg: PipelineConfig) -> Self {
        Self {
            config: RwLock::new(cfg),
            config_path: None,
        }
    }

    /// Display name from config (`PipelineConfig::name`).
    pub async fn pipeline_display_name(&self) -> String {
        self.config.read().await.name.clone()
    }

    /// Set `PipelineConfig::name` (board title in the UI).
    pub async fn set_display_name(&self, name: String) {
        self.config.write().await.name = name;
    }

    // -- writes --------------------------------------------------------------

    /// Replace stages (and optionally columns) from an API body.
    pub async fn set_stages_response(&self, body: StagesResponse) -> Result<(), String> {
        if body.stages.is_empty() {
            return Err("pipeline must have at least one stage".into());
        }
        let mut seen = std::collections::HashSet::new();
        for s in &body.stages {
            if !seen.insert(s.id.as_str()) {
                return Err(format!("duplicate stage id: {}", s.id));
            }
        }
        let new_stages: Result<Vec<StageDefinition>, String> =
            body.stages.into_iter().map(stage_from_response).collect();
        let new_stages = new_stages?;

        let mut guard = self.config.write().await;
        guard.stages = new_stages;
        if let Some(cols) = body.columns {
            guard.columns = cols;
        }
        validate_pipeline_config(&guard)?;
        self.persist(&guard)?;
        Ok(())
    }

    /// Add a single stage at the end.
    pub async fn add_stage(&self, stage: StageResponse) -> Result<(), String> {
        let def = stage_from_response(stage)?;
        let mut guard = self.config.write().await;
        if guard.stages.iter().any(|s| s.name == def.name) {
            return Err(format!("stage with id '{}' already exists", def.name));
        }
        guard.stages.push(def);
        validate_pipeline_config(&guard)?;
        self.persist(&guard)?;
        Ok(())
    }

    /// Remove a stage by ID.
    pub async fn remove_stage(&self, id: &str) -> Result<(), String> {
        let mut guard = self.config.write().await;
        let before = guard.stages.len();
        guard.stages.retain(|s| s.name != id);
        if guard.stages.len() == before {
            return Err(format!("stage with id '{}' not found", id));
        }
        if guard.stages.is_empty() {
            return Err("cannot remove last stage".into());
        }
        validate_pipeline_config(&guard)?;
        self.persist(&guard)?;
        Ok(())
    }

    /// Partially update a single stage by ID, applying only supplied fields.
    pub async fn patch_stage(&self, id: &str, patch: StagePatch) -> Result<StageResponse, String> {
        let mut guard = self.config.write().await;
        let stage = guard
            .stages
            .iter_mut()
            .find(|s| s.name == id)
            .ok_or_else(|| format!("stage with id '{}' not found", id))?;

        if let Some(label) = patch.label {
            stage.label = if label.trim().is_empty() || label == id {
                None
            } else {
                Some(label)
            };
        }
        if let Some(wip_limit) = patch.wip_limit {
            stage.wip_limit = wip_limit;
        }
        if let Some(requires_approval) = patch.requires_approval {
            stage.requires_approval = requires_approval;
        }
        if let Some(timeout_seconds) = patch.timeout_seconds {
            stage.timeout_seconds = timeout_seconds;
        }
        if let Some(terminal) = patch.terminal {
            stage.terminal = terminal;
        }
        if let Some(color) = patch.color {
            stage.color = color;
        }
        if let Some(order) = patch.order {
            stage.order = order;
        }

        let updated = stage_to_response(stage);
        validate_pipeline_config(&guard)?;
        self.persist(&guard)?;
        Ok(updated)
    }

    // -- persistence ---------------------------------------------------------

    fn persist(&self, cfg: &PipelineConfig) -> Result<(), String> {
        if let Some(ref path) = self.config_path {
            Self::write_yaml(path, cfg)
        } else {
            Ok(())
        }
    }

    fn write_yaml(path: &std::path::Path, cfg: &PipelineConfig) -> Result<(), String> {
        let content = serde_yaml::to_string(cfg).map_err(|e| format!("yaml serialize: {e}"))?;
        std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))
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
    let body = state.get_stages_response().await;
    (StatusCode::OK, Json(body))
}

/// PUT /api/pipeline/stages — replace all stages
#[instrument(skip_all)]
pub async fn put_stages(
    State(state): State<Arc<PipelineConfigStore>>,
    Json(body): Json<StagesResponse>,
) -> impl IntoResponse {
    match state.set_stages_response(body).await {
        Ok(()) => {
            let body = state.get_stages_response().await;
            info!(count = body.stages.len(), "pipeline stages updated");
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response(),
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
            let body = state.get_stages_response().await;
            info!(count = body.stages.len(), "pipeline stage added");
            (StatusCode::CREATED, Json(body)).into_response()
        }
        Err(e) => (StatusCode::CONFLICT, Json(ErrorResponse { error: e })).into_response(),
    }
}

/// PATCH /api/pipeline/stages/:id — partially update a single stage
#[instrument(skip_all)]
pub async fn patch_stage(
    State(state): State<Arc<PipelineConfigStore>>,
    Path(stage_id): Path<String>,
    Json(patch): Json<StagePatch>,
) -> impl IntoResponse {
    match state.patch_stage(&stage_id, patch).await {
        Ok(stage) => {
            info!(stage_id, "pipeline stage patched");
            (StatusCode::OK, Json(stage)).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, Json(ErrorResponse { error: e })).into_response(),
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
            let body = state.get_stages_response().await;
            info!(stage_id, "pipeline stage removed");
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => (StatusCode::GONE, Json(ErrorResponse { error: e })).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

use axum::{routing::get, Router};

/// Build the `/api/pipeline` sub-router.
pub fn pipeline_router(state: Arc<PipelineConfigStore>) -> Router {
    Router::new()
        .route("/stages", get(get_stages).put(put_stages).post(post_stage))
        .route(
            "/stages/:id",
            axum::routing::delete(delete_stage).patch(patch_stage),
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

    fn sample_stage(
        id: &str,
        label: &str,
        wip_limit: Option<u32>,
        requires_approval: bool,
        timeout_seconds: Option<u64>,
        terminal: bool,
        color: Option<String>,
        order: u32,
    ) -> StageResponse {
        StageResponse {
            id: id.into(),
            label: label.into(),
            wip_limit,
            requires_approval,
            timeout_seconds,
            terminal,
            color,
            order,
            hooks: vec![],
            transition_rules: vec![],
            instructions: None,
            instructions_template: None,
            approvers: vec![],
        }
    }

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
    async fn get_stages_include_display_fields() {
        let app = test_app();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/pipeline/stages")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = json_body(resp).await;
        let stages = body["stages"].as_array().unwrap();

        // Backlog defaults
        assert_eq!(stages[0]["color"], "#94a3b8");
        assert_eq!(stages[0]["order"], 0);

        // In Progress defaults
        assert_eq!(stages[1]["color"], "#6366f1");
        assert_eq!(stages[1]["order"], 1);

        // Deployed defaults
        assert_eq!(stages[4]["color"], "#22c55e");
        assert_eq!(stages[4]["order"], 4);
    }

    #[tokio::test]
    async fn get_stages_custom_state() {
        let state = Arc::new(PipelineConfigStore::in_memory_with(vec![
            sample_stage(
                "todo",
                "To Do",
                Some(5),
                false,
                None,
                false,
                Some("#ff0000".into()),
                0,
            ),
            sample_stage("done", "Done", None, false, None, true, None, 1),
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
        assert_eq!(stages[0]["color"], "#ff0000");
        assert_eq!(stages[1]["id"], "done");
        assert!(stages[1]["wip_limit"].is_null());
        assert!(stages[1]["color"].is_null());
    }

    #[tokio::test]
    async fn pipeline_state_default_stages_has_five_entries() {
        let state = PipelineConfigStore::in_memory();
        let stages = state.get_stages().await;
        assert_eq!(stages.len(), 5);
        let ids: Vec<&str> = stages.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "backlog",
                "in-progress",
                "code-review",
                "testing",
                "deployed"
            ]
        );
    }

    #[tokio::test]
    async fn default_stages_have_colors_and_orders() {
        let state = PipelineConfigStore::in_memory();
        let stages = state.get_stages().await;

        let expected = vec![
            ("backlog", "#94a3b8", 0u32),
            ("in-progress", "#6366f1", 1),
            ("code-review", "#f59e0b", 2),
            ("testing", "#10b981", 3),
            ("deployed", "#22c55e", 4),
        ];
        for (stage, (id, color, order)) in stages.iter().zip(expected.iter()) {
            assert_eq!(stage.id, *id);
            assert_eq!(stage.color.as_deref(), Some(*color));
            assert_eq!(stage.order, *order);
        }
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
    async fn put_stages_preserves_display_fields() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(Arc::clone(&state)))
            .into_service::<Body>();

        let new_stages = serde_json::json!({
            "stages": [
                {"id": "x", "label": "X", "color": "#abcdef", "order": 7}
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
        assert_eq!(stages[0].color.as_deref(), Some("#abcdef"));
        assert_eq!(stages[0].order, 7);
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
    async fn post_stage_with_display_fields() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(Arc::clone(&state)))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/pipeline/stages")
            .header("content-type", "application/json")
            .body(Body::from(
                r##"{"id":"qa","label":"QA","color":"#e11d48","order":5}"##,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let stages = state.get_stages().await;
        let qa = stages.iter().find(|s| s.id == "qa").unwrap();
        assert_eq!(qa.color.as_deref(), Some("#e11d48"));
        assert_eq!(qa.order, 5);
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
            .body(Body::from(r#"{"id":"backlog","label":"Dupe"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    // -- PATCH tests ---------------------------------------------------------

    #[tokio::test]
    async fn patch_stage_updates_color() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(Arc::clone(&state)))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::PATCH)
            .uri("/api/pipeline/stages/backlog")
            .header("content-type", "application/json")
            .body(Body::from(r##"{"color":"#ff5500"}"##))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        assert_eq!(body["id"], "backlog");
        assert_eq!(body["color"], "#ff5500");

        // Verify persisted
        let stages = state.get_stages().await;
        let backlog = stages.iter().find(|s| s.id == "backlog").unwrap();
        assert_eq!(backlog.color.as_deref(), Some("#ff5500"));
    }

    #[tokio::test]
    async fn patch_stage_updates_order() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(Arc::clone(&state)))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::PATCH)
            .uri("/api/pipeline/stages/in-progress")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"order":99}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let stages = state.get_stages().await;
        let stage = stages.iter().find(|s| s.id == "in-progress").unwrap();
        assert_eq!(stage.order, 99);
    }

    #[tokio::test]
    async fn patch_stage_updates_label() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(Arc::clone(&state)))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::PATCH)
            .uri("/api/pipeline/stages/backlog")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"label":"TODO"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let stages = state.get_stages().await;
        let backlog = stages.iter().find(|s| s.id == "backlog").unwrap();
        assert_eq!(backlog.label, "TODO");
    }

    #[tokio::test]
    async fn patch_stage_updates_multiple_fields() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(Arc::clone(&state)))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::PATCH)
            .uri("/api/pipeline/stages/testing")
            .header("content-type", "application/json")
            .body(Body::from(
                r##"{"label":"QA Testing","color":"#dc2626","order":10,"wip_limit":3}"##,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let stages = state.get_stages().await;
        let stage = stages.iter().find(|s| s.id == "testing").unwrap();
        assert_eq!(stage.label, "QA Testing");
        assert_eq!(stage.color.as_deref(), Some("#dc2626"));
        assert_eq!(stage.order, 10);
        assert_eq!(stage.wip_limit, Some(3));
    }

    #[tokio::test]
    async fn patch_stage_clears_color_with_null() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(Arc::clone(&state)))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::PATCH)
            .uri("/api/pipeline/stages/backlog")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"color":null}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let stages = state.get_stages().await;
        let backlog = stages.iter().find(|s| s.id == "backlog").unwrap();
        assert_eq!(backlog.color, None);
    }

    #[tokio::test]
    async fn patch_stage_not_found() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(state))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::PATCH)
            .uri("/api/pipeline/stages/nonexistent")
            .header("content-type", "application/json")
            .body(Body::from(r##"{"color":"#000000"}"##))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn patch_stage_empty_body_is_noop() {
        let state = Arc::new(PipelineConfigStore::in_memory());
        let original = state.get_stages().await;
        let app = Router::new()
            .nest("/api/pipeline", pipeline_router(Arc::clone(&state)))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::PATCH)
            .uri("/api/pipeline/stages/backlog")
            .header("content-type", "application/json")
            .body(Body::from(r#"{}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let stages = state.get_stages().await;
        let backlog = stages.iter().find(|s| s.id == "backlog").unwrap();
        let orig_backlog = original.iter().find(|s| s.id == "backlog").unwrap();
        assert_eq!(backlog, orig_backlog);
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
            .add_stage(sample_stage(
                "extra",
                "Extra",
                Some(10),
                true,
                Some(3600),
                false,
                Some("#8b5cf6".into()),
                5,
            ))
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
        assert_eq!(extra.color.as_deref(), Some("#8b5cf6"));
        assert_eq!(extra.order, 5);
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
            .set_stages_response(StagesResponse {
                stages: vec![sample_stage(
                    "only",
                    "Only Stage",
                    None,
                    false,
                    None,
                    true,
                    Some("#000000".into()),
                    0,
                )],
                columns: None,
            })
            .await
            .unwrap();

        let store2 = PipelineConfigStore::from_file(path);
        let stages = store2.get_stages().await;
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].id, "only");
        assert_eq!(stages[0].color.as_deref(), Some("#000000"));
    }

    #[tokio::test]
    async fn yaml_roundtrip_patch_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pipeline.yaml");

        let store = PipelineConfigStore::from_file(path.clone());
        store
            .patch_stage(
                "backlog",
                StagePatch {
                    color: Some(Some("#ffffff".into())),
                    order: Some(42),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let store2 = PipelineConfigStore::from_file(path);
        let stages = store2.get_stages().await;
        let backlog = stages.iter().find(|s| s.id == "backlog").unwrap();
        assert_eq!(backlog.color.as_deref(), Some("#ffffff"));
        assert_eq!(backlog.order, 42);
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
