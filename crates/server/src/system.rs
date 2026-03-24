//! Local-system helpers for the web UI (browser talking to a same-machine server).

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

#[derive(Serialize)]
pub struct PickFolderResponse {
    /// Absolute path when the user chose a folder; omitted or null when cancelled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Set when the dialog could not run (e.g. task join error).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `POST /api/system/pick-repo-folder` — opens a native folder picker on the host running the server.
///
/// Intended for the browser UI when it cannot use Tauri’s dialog. The chosen path is valid for
/// server-side Git and agent operations on that machine.
pub async fn pick_repo_folder() -> impl IntoResponse {
    let task_result = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Choose Git repository folder")
            .pick_folder()
    })
    .await;

    match task_result {
        Ok(Some(path)) => (
            StatusCode::OK,
            Json(PickFolderResponse {
                path: Some(path.display().to_string()),
                error: None,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::OK,
            Json(PickFolderResponse {
                path: None,
                error: None,
            }),
        )
            .into_response(),
        Err(join_err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(PickFolderResponse {
                path: None,
                error: Some(format!("folder picker task failed: {join_err}")),
            }),
        )
            .into_response(),
    }
}
