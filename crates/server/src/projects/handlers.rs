//! Axum HTTP handlers for the projects API.
//!
//! Routes:
//!   GET    /api/projects       — list all projects
//!   POST   /api/projects       — create a project
//!   GET    /api/projects/:id   — get a single project
//!   PATCH  /api/projects/:id   — update project name
//!   DELETE /api/projects/:id   — archive (soft-delete) a project

use crate::pipeline::handlers::{PipelineConfigStore, PipelineState, StagePatch, StagesResponse};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::Utc;
use molt_hub_core::model::ProjectId;
use molt_hub_core::project::{Project, ProjectStatus, ProjectValidationError};
use molt_hub_harness::supervisor::Supervisor;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, instrument, warn};

use super::runtime::{
    ensure_project_runtime, BoardSummary, MultiBoardPipelineStore, ProjectRuntime,
    ProjectRuntimeRegistry,
};

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Body for POST /api/projects.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub repo_path: String,
}

/// Body for PATCH /api/projects/:id.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
}

/// Single project response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectResponse {
    pub id: String,
    pub name: String,
    pub repo_path: String,
    pub status: ProjectStatus,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&Project> for ProjectResponse {
    fn from(p: &Project) -> Self {
        Self {
            id: p.id.to_string(),
            name: p.name.clone(),
            repo_path: p.repo_path.display().to_string(),
            status: p.status.clone(),
            created_at: p.created_at.to_rfc3339(),
            updated_at: p.updated_at.to_rfc3339(),
        }
    }
}

/// List response.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectsListResponse {
    pub projects: Vec<ProjectResponse>,
}

/// Generic error body.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ---------------------------------------------------------------------------
// YAML store
// ---------------------------------------------------------------------------

/// YAML-serialisable wrapper for the projects file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ProjectsYaml {
    projects: Vec<Project>,
}

/// In-memory + optional YAML-file persistence for projects.
pub struct ProjectConfigStore {
    projects: RwLock<Vec<Project>>,
    config_path: Option<PathBuf>,
}

impl ProjectConfigStore {
    /// Create a store backed by a YAML file.
    pub fn from_file(path: PathBuf) -> Self {
        let projects = match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_yaml::from_str::<ProjectsYaml>(&contents) {
                Ok(yaml) => yaml.projects,
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "failed to parse projects YAML, starting empty");
                    Vec::new()
                }
            },
            Err(_) => {
                // File doesn't exist yet — start empty.
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                Vec::new()
            }
        };

        Self {
            projects: RwLock::new(projects),
            config_path: Some(path),
        }
    }

    /// In-memory store (for tests).
    pub fn in_memory() -> Self {
        Self {
            projects: RwLock::new(Vec::new()),
            config_path: None,
        }
    }

    /// In-memory store with initial data (for tests).
    pub fn in_memory_with(projects: Vec<Project>) -> Self {
        Self {
            projects: RwLock::new(projects),
            config_path: None,
        }
    }

    /// Resolve the default config file location: `~/.config/molt-hub/projects.yaml`.
    pub fn default_config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("molt-hub").join("projects.yaml"))
    }

    /// Load from the default config path, or fall back to in-memory.
    pub fn load_default() -> Self {
        match Self::default_config_path() {
            Some(path) => Self::from_file(path),
            None => Self::in_memory(),
        }
    }

    // -- reads ---------------------------------------------------------------

    pub async fn list(&self) -> Vec<Project> {
        self.projects
            .read()
            .await
            .iter()
            .filter(|p| p.status == ProjectStatus::Active)
            .cloned()
            .collect()
    }

    pub async fn list_all(&self) -> Vec<Project> {
        self.projects.read().await.clone()
    }

    pub async fn get(&self, id: &str) -> Option<Project> {
        self.projects
            .read()
            .await
            .iter()
            .find(|p| p.id.to_string() == id)
            .cloned()
    }

    // -- writes --------------------------------------------------------------

    pub async fn create(&self, name: String, repo_path: PathBuf) -> Result<Project, String> {
        let now = Utc::now();
        let project = Project {
            id: ProjectId::new(),
            name: name.clone(),
            repo_path,
            description: None,
            status: ProjectStatus::Active,
            created_at: now,
            updated_at: now,
        };

        project
            .validate(true)
            .map_err(|e: ProjectValidationError| e.to_string())?;

        let mut guard = self.projects.write().await;

        // Check for duplicate name among active projects.
        if guard
            .iter()
            .any(|p| p.name == name && p.status == ProjectStatus::Active)
        {
            return Err(format!("project with name '{}' already exists", name));
        }

        guard.push(project.clone());
        self.persist(&guard)?;
        Ok(project)
    }

    pub async fn update(&self, id: &str, name: String) -> Result<Project, String> {
        let mut guard = self.projects.write().await;

        let project = guard
            .iter_mut()
            .find(|p| p.id.to_string() == id)
            .ok_or_else(|| format!("project '{}' not found", id))?;

        if project.status == ProjectStatus::Archived {
            return Err("cannot update an archived project".into());
        }

        if name.trim().is_empty() {
            return Err(ProjectValidationError::EmptyName.to_string());
        }

        // Check for duplicate name among other active projects.
        let has_dup = guard
            .iter()
            .any(|p| p.id.to_string() != id && p.name == name && p.status == ProjectStatus::Active);
        if has_dup {
            return Err(format!("project with name '{}' already exists", name));
        }

        let project = guard.iter_mut().find(|p| p.id.to_string() == id).unwrap();
        project.name = name;
        project.updated_at = Utc::now();

        let updated = project.clone();
        self.persist(&guard)?;
        Ok(updated)
    }

    pub async fn archive(&self, id: &str) -> Result<Project, String> {
        let mut guard = self.projects.write().await;

        let project = guard
            .iter_mut()
            .find(|p| p.id.to_string() == id)
            .ok_or_else(|| format!("project '{}' not found", id))?;

        if project.status == ProjectStatus::Archived {
            return Err("project is already archived".into());
        }

        project.status = ProjectStatus::Archived;
        project.updated_at = Utc::now();

        let archived = project.clone();
        self.persist(&guard)?;
        Ok(archived)
    }

    // -- persistence ---------------------------------------------------------

    fn persist(&self, projects: &[Project]) -> Result<(), String> {
        if let Some(ref path) = self.config_path {
            Self::write_yaml(path, projects)
        } else {
            Ok(())
        }
    }

    fn write_yaml(path: &std::path::Path, projects: &[Project]) -> Result<(), String> {
        let yaml = ProjectsYaml {
            projects: projects.to_vec(),
        };
        let content = serde_yaml::to_string(&yaml).map_err(|e| format!("yaml serialize: {e}"))?;
        std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))
    }
}

// ---------------------------------------------------------------------------
// Shared state type
// ---------------------------------------------------------------------------

pub type ProjectState = ProjectConfigStore;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/projects
#[instrument(skip_all)]
pub async fn list_projects(State(state): State<Arc<ProjectConfigStore>>) -> impl IntoResponse {
    let projects = state.list().await;
    let items: Vec<ProjectResponse> = projects.iter().map(ProjectResponse::from).collect();
    (
        StatusCode::OK,
        Json(ProjectsListResponse { projects: items }),
    )
}

/// POST /api/projects
#[instrument(skip_all)]
pub async fn create_project(
    State(state): State<Arc<ProjectConfigStore>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    Json(body): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    match state
        .create(body.name, PathBuf::from(&body.repo_path))
        .await
    {
        Ok(project) => {
            let pid = project.id.to_string();
            let boards_store = registry.boards_store();
            let runtime = Arc::new(ProjectRuntime {
                project_id: pid.clone(),
                supervisor: Arc::clone(&supervisor),
                boards: Arc::new(
                    MultiBoardPipelineStore::load_or_empty(
                        &pid,
                        registry.new_board_template(),
                        boards_store,
                    )
                    .await,
                ),
            });
            registry.insert(pid, runtime).await;
            info!(id = %project.id, name = %project.name, "project created");
            (StatusCode::CREATED, Json(ProjectResponse::from(&project))).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response(),
    }
}

/// GET /api/projects/:id
#[instrument(skip_all)]
pub async fn get_project(
    State(state): State<Arc<ProjectConfigStore>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.get(&id).await {
        Some(project) => (StatusCode::OK, Json(ProjectResponse::from(&project))).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("project '{}' not found", id),
            }),
        )
            .into_response(),
    }
}

/// PATCH /api/projects/:id
#[instrument(skip_all)]
pub async fn update_project(
    State(state): State<Arc<ProjectConfigStore>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectRequest>,
) -> impl IntoResponse {
    let name = match body.name {
        Some(n) => n,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "name field is required".into(),
                }),
            )
                .into_response()
        }
    };

    match state.update(&id, name).await {
        Ok(project) => {
            info!(id = %project.id, name = %project.name, "project updated");
            (StatusCode::OK, Json(ProjectResponse::from(&project))).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response(),
    }
}

/// DELETE /api/projects/:id
#[instrument(skip_all)]
pub async fn archive_project(
    State(state): State<Arc<ProjectConfigStore>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.archive(&id).await {
        Ok(project) => {
            info!(id = %project.id, "project archived");
            (StatusCode::OK, Json(ProjectResponse::from(&project))).into_response()
        }
        Err(e) => {
            let status = if e.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, Json(ErrorResponse { error: e })).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Boards + project-scoped pipeline stages
// ---------------------------------------------------------------------------

/// GET /api/projects/:pid/board-template
///
/// Default stages/columns for **new** boards (same JSON shape as GET …/boards/:bid/stages).
#[instrument(skip_all)]
pub async fn get_project_board_template(
    Path(project_id): Path<String>,
    State(projects): State<Arc<ProjectConfigStore>>,
    Extension(pipeline): Extension<Arc<PipelineState>>,
) -> impl IntoResponse {
    if !project_exists_or_default(&projects, &project_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("project '{project_id}' not found"),
            }),
        )
            .into_response();
    }
    let body = pipeline.get_stages_response().await;
    (StatusCode::OK, Json(body)).into_response()
}

async fn project_exists_or_default(projects: &ProjectConfigStore, project_id: &str) -> bool {
    project_id == "default" || projects.get(project_id).await.is_some()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardRequest {
    /// Display name for the board (server assigns a ULID as the stable id).
    pub name: String,
}

#[derive(Serialize)]
struct BoardsListBody {
    boards: Vec<BoardSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateBoardResponse {
    boards: Vec<BoardSummary>,
    board_id: String,
}

/// GET /api/projects/:pid/boards
#[instrument(skip_all)]
pub async fn list_project_boards(
    Path(project_id): Path<String>,
    State(projects): State<Arc<ProjectConfigStore>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
) -> impl IntoResponse {
    if !project_exists_or_default(&projects, &project_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("project '{}' not found", project_id),
            }),
        )
            .into_response();
    }
    let rt = ensure_project_runtime(&project_id, &registry, &supervisor).await;
    let boards = rt.boards.list_summaries().await;
    (StatusCode::OK, Json(BoardsListBody { boards })).into_response()
}

/// POST /api/projects/:pid/boards
#[instrument(skip_all)]
pub async fn post_project_board(
    Path(project_id): Path<String>,
    State(projects): State<Arc<ProjectConfigStore>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    Json(body): Json<CreateBoardRequest>,
) -> impl IntoResponse {
    if !project_exists_or_default(&projects, &project_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("project '{}' not found", project_id),
            }),
        )
            .into_response();
    }
    let rt = ensure_project_runtime(&project_id, &registry, &supervisor).await;
    match rt.boards.create_board(&body.name).await {
        Ok(board_id) => {
            let boards = rt.boards.list_summaries().await;
            (
                StatusCode::CREATED,
                Json(CreateBoardResponse {
                    boards,
                    board_id,
                }),
            )
                .into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response(),
    }
}

/// DELETE /api/projects/:pid/boards/:bid
#[instrument(skip_all)]
pub async fn delete_project_board(
    Path((project_id, board_id)): Path<(String, String)>,
    State(projects): State<Arc<ProjectConfigStore>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
) -> impl IntoResponse {
    if !project_exists_or_default(&projects, &project_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("project '{}' not found", project_id),
            }),
        )
            .into_response();
    }
    let rt = ensure_project_runtime(&project_id, &registry, &supervisor).await;
    match rt.boards.delete_board(&board_id).await {
        Ok(()) => {
            let boards = rt.boards.list_summaries().await;
            (StatusCode::OK, Json(BoardsListBody { boards })).into_response()
        }
        Err(e) => {
            let status = if e.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, Json(ErrorResponse { error: e })).into_response()
        }
    }
}

async fn board_pipeline_store(
    project_id: &str,
    board_id: &str,
    projects: &ProjectConfigStore,
    registry: &ProjectRuntimeRegistry,
    supervisor: &Arc<Supervisor>,
) -> Result<Arc<PipelineConfigStore>, (StatusCode, Json<ErrorResponse>)> {
    if !project_exists_or_default(projects, project_id).await {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("project '{project_id}' not found"),
            }),
        ));
    }
    let rt = ensure_project_runtime(project_id, registry, supervisor).await;
    let Some(store) = rt.boards.get_store(board_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("board '{board_id}' not found"),
            }),
        ));
    };
    Ok(store)
}

/// GET /api/projects/:pid/boards/:bid/stages
#[instrument(skip_all)]
pub async fn get_project_board_stages(
    Path((project_id, board_id)): Path<(String, String)>,
    State(projects): State<Arc<ProjectConfigStore>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
) -> impl IntoResponse {
    match board_pipeline_store(&project_id, &board_id, &projects, &registry, &supervisor).await {
        Ok(store) => {
            let body = store.get_stages_response().await;
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// PUT /api/projects/:pid/boards/:bid/stages
#[instrument(skip_all)]
pub async fn put_project_board_stages(
    Path((project_id, board_id)): Path<(String, String)>,
    State(projects): State<Arc<ProjectConfigStore>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    Json(body): Json<StagesResponse>,
) -> impl IntoResponse {
    let store =
        match board_pipeline_store(&project_id, &board_id, &projects, &registry, &supervisor).await
        {
            Ok(s) => s,
            Err(e) => return e.into_response(),
        };
    match store.set_stages_response(body).await {
        Ok(()) => {
            let body = store.get_stages_response().await;
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response(),
    }
}

/// PATCH /api/projects/:pid/boards/:bid/stages/:sid
#[instrument(skip_all)]
pub async fn patch_project_board_stage(
    Path((project_id, board_id, stage_id)): Path<(String, String, String)>,
    State(projects): State<Arc<ProjectConfigStore>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    Json(patch): Json<StagePatch>,
) -> impl IntoResponse {
    let store =
        match board_pipeline_store(&project_id, &board_id, &projects, &registry, &supervisor).await
        {
            Ok(s) => s,
            Err(e) => return e.into_response(),
        };
    match store.patch_stage(&stage_id, patch).await {
        Ok(stage) => (StatusCode::OK, Json(stage)).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(ErrorResponse { error: e })).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

/// Build the `/api/projects` sub-router (for use with `nest_service`).
///
/// This includes both the base project CRUD routes and the project-scoped
/// agent/board routes. The project-scoped handlers use
/// `Extension<Arc<ProjectRuntimeRegistry>>` (not `State`) so they are
/// compatible with the `Arc<ProjectConfigStore>` state of this router.
pub fn project_router(state: Arc<ProjectConfigStore>) -> Router {
    Router::new()
        .route("/", get(list_projects).post(create_project))
        .route(
            "/:id",
            get(get_project)
                .patch(update_project)
                .delete(archive_project),
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
        extract::Extension,
        http::{Method, Request, Response},
        Router,
    };
    use molt_hub_harness::adapter::AgentEvent;
    use molt_hub_harness::supervisor::{Supervisor, SupervisorConfig};
    use tokio::sync::broadcast;
    use tower::util::ServiceExt;

    fn test_extensions() -> (
        Extension<Arc<ProjectRuntimeRegistry>>,
        Extension<Arc<Supervisor>>,
    ) {
        use molt_hub_core::config::PipelineConfig;
        let registry = Arc::new(ProjectRuntimeRegistry::new(PipelineConfig::board_defaults(), None));
        let (tx, _rx) = broadcast::channel::<AgentEvent>(4);
        let supervisor = Arc::new(Supervisor::new(SupervisorConfig::default(), tx));
        (Extension(registry), Extension(supervisor))
    }

    fn test_app() -> impl tower::Service<
        Request<Body>,
        Response = Response<Body>,
        Error = std::convert::Infallible,
        Future: Send,
    > + Clone {
        let state = Arc::new(ProjectConfigStore::in_memory());
        let (ext_reg, ext_sup) = test_extensions();
        Router::new()
            .nest("/api/projects", project_router(state))
            .layer(ext_reg)
            .layer(ext_sup)
            .into_service::<Body>()
    }

    fn test_app_with_state(
        state: Arc<ProjectConfigStore>,
    ) -> impl tower::Service<
        Request<Body>,
        Response = Response<Body>,
        Error = std::convert::Infallible,
        Future: Send,
    > + Clone {
        let (ext_reg, ext_sup) = test_extensions();
        Router::new()
            .nest("/api/projects", project_router(state))
            .layer(ext_reg)
            .layer(ext_sup)
            .into_service::<Body>()
    }

    async fn json_body(resp: Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn create_body(name: &str, repo_path: &str) -> Body {
        Body::from(
            serde_json::to_string(&serde_json::json!({
                "name": name,
                "repo_path": repo_path,
            }))
            .unwrap(),
        )
    }

    // -- GET /api/projects ---------------------------------------------------

    #[tokio::test]
    async fn list_projects_empty() {
        let app = test_app();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/projects")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        assert_eq!(body["projects"].as_array().unwrap().len(), 0);
    }

    // -- POST /api/projects --------------------------------------------------

    #[tokio::test]
    async fn create_project_success() {
        let state = Arc::new(ProjectConfigStore::in_memory());
        let app = test_app_with_state(Arc::clone(&state));

        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/projects")
            .header("content-type", "application/json")
            .body(create_body("my-app", "/tmp"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = json_body(resp).await;
        assert_eq!(body["name"], "my-app");
        assert_eq!(body["status"], "active");

        // Verify persisted.
        assert_eq!(state.list().await.len(), 1);
    }

    #[tokio::test]
    async fn create_project_empty_name_rejected() {
        let app = test_app();
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/projects")
            .header("content-type", "application/json")
            .body(create_body("", "/tmp"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_project_nonexistent_path_rejected() {
        let app = test_app();
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/projects")
            .header("content-type", "application/json")
            .body(create_body("my-app", "/nonexistent/path/xyz999"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_project_duplicate_name_rejected() {
        let state = Arc::new(ProjectConfigStore::in_memory());

        // Create first.
        state
            .create("dup".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();

        let app = test_app_with_state(state);
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/projects")
            .header("content-type", "application/json")
            .body(create_body("dup", "/tmp"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = json_body(resp).await;
        assert!(body["error"].as_str().unwrap().contains("already exists"));
    }

    // -- GET /api/projects/:id -----------------------------------------------

    #[tokio::test]
    async fn get_project_found() {
        let state = Arc::new(ProjectConfigStore::in_memory());
        let project = state
            .create("test-proj".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();

        let app = test_app_with_state(state);
        let req = Request::builder()
            .method(Method::GET)
            .uri(&format!("/api/projects/{}", project.id))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        assert_eq!(body["name"], "test-proj");
    }

    #[tokio::test]
    async fn get_project_not_found() {
        let app = test_app();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/projects/nonexistent-id")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // -- PATCH /api/projects/:id ---------------------------------------------

    #[tokio::test]
    async fn update_project_name() {
        let state = Arc::new(ProjectConfigStore::in_memory());
        let project = state
            .create("old-name".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();

        let app = test_app_with_state(state);
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(&format!("/api/projects/{}", project.id))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name": "new-name"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        assert_eq!(body["name"], "new-name");
    }

    #[tokio::test]
    async fn update_project_empty_name_rejected() {
        let state = Arc::new(ProjectConfigStore::in_memory());
        let project = state
            .create("proj".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();

        let app = test_app_with_state(state);
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(&format!("/api/projects/{}", project.id))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name": ""}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn update_project_duplicate_name_rejected() {
        let state = Arc::new(ProjectConfigStore::in_memory());
        state
            .create("proj-a".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        let proj_b = state
            .create("proj-b".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();

        let app = test_app_with_state(state);
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(&format!("/api/projects/{}", proj_b.id))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name": "proj-a"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // -- DELETE /api/projects/:id --------------------------------------------

    #[tokio::test]
    async fn archive_project_success() {
        let state = Arc::new(ProjectConfigStore::in_memory());
        let project = state
            .create("doomed".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();

        let app = test_app_with_state(Arc::clone(&state));
        let req = Request::builder()
            .method(Method::DELETE)
            .uri(&format!("/api/projects/{}", project.id))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        assert_eq!(body["status"], "archived");

        // Active list should be empty.
        assert_eq!(state.list().await.len(), 0);
        // But it should still exist in all.
        assert_eq!(state.list_all().await.len(), 1);
    }

    #[tokio::test]
    async fn archive_project_not_found() {
        let app = test_app();
        let req = Request::builder()
            .method(Method::DELETE)
            .uri("/api/projects/nonexistent-id")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn archive_project_already_archived() {
        let state = Arc::new(ProjectConfigStore::in_memory());
        let project = state
            .create("arch".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        state.archive(&project.id.to_string()).await.unwrap();

        let app = test_app_with_state(state);
        let req = Request::builder()
            .method(Method::DELETE)
            .uri(&format!("/api/projects/{}", project.id))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // -- YAML persistence ----------------------------------------------------

    #[tokio::test]
    async fn yaml_roundtrip_create_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.yaml");

        let store = ProjectConfigStore::from_file(path.clone());
        store
            .create("roundtrip".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();

        // Reload from disk.
        let store2 = ProjectConfigStore::from_file(path);
        let projects = store2.list().await;
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "roundtrip");
    }

    #[tokio::test]
    async fn yaml_roundtrip_archive_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.yaml");

        let store = ProjectConfigStore::from_file(path.clone());
        let project = store
            .create("archivable".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        store.archive(&project.id.to_string()).await.unwrap();

        let store2 = ProjectConfigStore::from_file(path);
        assert_eq!(store2.list().await.len(), 0);
        assert_eq!(store2.list_all().await.len(), 1);
        assert_eq!(store2.list_all().await[0].status, ProjectStatus::Archived);
    }

    #[tokio::test]
    async fn from_file_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("projects.yaml");
        let _store = ProjectConfigStore::from_file(path.clone());
        // No panic = success. The directory is created even if file doesn't
        // exist yet (no projects to write).
    }

    // -- Store-level unit tests ----------------------------------------------

    #[tokio::test]
    async fn store_list_excludes_archived() {
        let store = ProjectConfigStore::in_memory();
        let p = store
            .create("a".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        store
            .create("b".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        store.archive(&p.id.to_string()).await.unwrap();

        assert_eq!(store.list().await.len(), 1);
        assert_eq!(store.list().await[0].name, "b");
    }

    #[tokio::test]
    async fn store_update_archived_rejected() {
        let store = ProjectConfigStore::in_memory();
        let p = store
            .create("x".into(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        store.archive(&p.id.to_string()).await.unwrap();

        let err = store.update(&p.id.to_string(), "y".into()).await;
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("archived"));
    }
}
