//! Axum HTTP handlers for the flat boards API (no project namespace).
//!
//! Routes:
//!   GET    /api/boards                        — list boards
//!   POST   /api/boards                        — create a board
//!   DELETE /api/boards/:bid                   — delete a board
//!   GET    /api/boards/template               — default stages template
//!   GET    /api/boards/:bid/stages            — get board stages
//!   PUT    /api/boards/:bid/stages            — replace board stages
//!   PATCH  /api/boards/:bid/stages/:sid       — patch a single stage
//!
//! All handlers operate on the hardcoded `"default"` project runtime.

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, patch},
    Json, Router,
};
use molt_hub_harness::supervisor::Supervisor;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::pipeline::handlers::{PipelineConfigStore, PipelineState, StagePatch, StagesResponse};
use crate::projects::runtime::{ensure_project_runtime, BoardSummary, ProjectRuntimeRegistry};

const DEFAULT_PROJECT: &str = "default";

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardRequest {
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

#[derive(Debug, Clone, Serialize)]
struct ErrorResponse {
    error: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/boards/template
#[instrument(skip_all)]
pub async fn get_board_template(
    Extension(pipeline): Extension<Arc<PipelineState>>,
) -> impl IntoResponse {
    let body = pipeline.get_stages_response().await;
    (StatusCode::OK, Json(body)).into_response()
}

/// GET /api/boards
#[instrument(skip_all)]
pub async fn list_boards(
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
) -> impl IntoResponse {
    let rt = ensure_project_runtime(DEFAULT_PROJECT, &registry, &supervisor).await;
    let boards = rt.boards.list_summaries().await;
    (StatusCode::OK, Json(BoardsListBody { boards })).into_response()
}

/// POST /api/boards
#[instrument(skip_all)]
pub async fn post_board(
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    Json(body): Json<CreateBoardRequest>,
) -> impl IntoResponse {
    let rt = ensure_project_runtime(DEFAULT_PROJECT, &registry, &supervisor).await;
    match rt.boards.create_board(&body.name).await {
        Ok(board_id) => {
            let boards = rt.boards.list_summaries().await;
            (
                StatusCode::CREATED,
                Json(CreateBoardResponse { boards, board_id }),
            )
                .into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response(),
    }
}

/// DELETE /api/boards/:bid
#[instrument(skip_all)]
pub async fn delete_board(
    Path(board_id): Path<String>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
) -> impl IntoResponse {
    let rt = ensure_project_runtime(DEFAULT_PROJECT, &registry, &supervisor).await;
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
    board_id: &str,
    registry: &ProjectRuntimeRegistry,
    supervisor: &Arc<Supervisor>,
) -> Result<Arc<PipelineConfigStore>, (StatusCode, Json<ErrorResponse>)> {
    let rt = ensure_project_runtime(DEFAULT_PROJECT, registry, supervisor).await;
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

/// GET /api/boards/:bid/stages
#[instrument(skip_all)]
pub async fn get_board_stages(
    Path(board_id): Path<String>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
) -> impl IntoResponse {
    match board_pipeline_store(&board_id, &registry, &supervisor).await {
        Ok(store) => {
            let body = store.get_stages_response().await;
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// PUT /api/boards/:bid/stages
#[instrument(skip_all)]
pub async fn put_board_stages(
    Path(board_id): Path<String>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    Json(body): Json<StagesResponse>,
) -> impl IntoResponse {
    let store = match board_pipeline_store(&board_id, &registry, &supervisor).await {
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

/// PATCH /api/boards/:bid/stages/:sid
#[instrument(skip_all)]
pub async fn patch_board_stage(
    Path((board_id, stage_id)): Path<(String, String)>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
    Json(p): Json<StagePatch>,
) -> impl IntoResponse {
    let store = match board_pipeline_store(&board_id, &registry, &supervisor).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    match store.patch_stage(&stage_id, p).await {
        Ok(stage) => (StatusCode::OK, Json(stage)).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(ErrorResponse { error: e })).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

/// Build the `/api/boards` sub-router (for use with `nest_service`).
pub fn boards_router() -> Router {
    Router::new()
        .route("/template", get(get_board_template))
        .route("/", get(list_boards).post(post_board))
        .route("/:bid", delete(delete_board))
        .route("/:bid/stages", get(get_board_stages).put(put_board_stages))
        .route("/:bid/stages/:sid", patch(patch_board_stage))
}
