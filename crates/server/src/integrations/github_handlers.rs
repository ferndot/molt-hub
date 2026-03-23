//! Axum HTTP handlers for the GitHub integration endpoints.
//!
//! Routes:
//!   GET  /api/integrations/github/repos   — list repos
//!   GET  /api/integrations/github/search  — search issues
//!   POST /api/integrations/github/import  — import selected issues (stub)
//!
//! Authentication is via a GitHub personal access token stored in app state.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use super::github_client::{GitHubClient, GitHubError};

// ---------------------------------------------------------------------------
// Shared app state
// ---------------------------------------------------------------------------

/// State injected into GitHub integration handlers.
pub struct GitHubState {
    /// The GitHub token used to authenticate API requests.
    /// `None` means no token has been configured yet.
    pub token: tokio::sync::RwLock<Option<String>>,
}

impl GitHubState {
    /// Create a new state with no token.
    pub fn new() -> Self {
        Self {
            token: tokio::sync::RwLock::new(None),
        }
    }

    /// Create a new state with a pre-configured token.
    pub fn with_token(token: String) -> Self {
        Self {
            token: tokio::sync::RwLock::new(Some(token)),
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Query parameters for the search endpoint.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// Repository owner (user or org).
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// Free-text search query.
    #[serde(default)]
    pub q: String,
}

/// Body for the import endpoint.
#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    /// Repository owner.
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// Issue numbers to import.
    pub issues: Vec<i64>,
}

/// Response body for a successful import.
#[derive(Debug, Serialize)]
pub struct ImportResponse {
    /// Number of issues imported.
    pub imported: usize,
    /// Human-readable message.
    pub message: String,
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/github/repos
// ---------------------------------------------------------------------------

/// List GitHub repositories visible to the authenticated user.
#[instrument(skip_all)]
pub async fn list_repos(
    State(state): State<Arc<GitHubState>>,
) -> impl IntoResponse {
    let client = match build_client(&state).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    match client.list_repos().await {
        Ok(repos) => Json(repos).into_response(),
        Err(e) => {
            let (status, msg) = github_error_to_http(&e);
            (status, Json(serde_json::json!({ "error": msg }))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/github/search
// ---------------------------------------------------------------------------

/// Search issues in a GitHub repository.
#[instrument(skip_all, fields(owner = %query.owner, repo = %query.repo))]
pub async fn search_issues(
    State(state): State<Arc<GitHubState>>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let client = match build_client(&state).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    match client.search_issues(&query.owner, &query.repo, &query.q).await {
        Ok(issues) => Json(issues).into_response(),
        Err(e) => {
            let (status, msg) = github_error_to_http(&e);
            (status, Json(serde_json::json!({ "error": msg }))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Handler: POST /api/integrations/github/import
// ---------------------------------------------------------------------------

/// Import selected GitHub issues (stub — returns success without persisting).
#[instrument(skip_all, fields(count = body.issues.len()))]
pub async fn import_issues(
    State(state): State<Arc<GitHubState>>,
    Json(body): Json<ImportRequest>,
) -> impl IntoResponse {
    // Verify we have a token (even though we don't actually call the API yet).
    let _client = match build_client(&state).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    // Stub: acknowledge the request without actually importing.
    let count = body.issues.len();
    Json(ImportResponse {
        imported: count,
        message: format!(
            "Stub: {count} issue(s) from {}/{} queued for import",
            body.owner, body.repo
        ),
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

use axum::{routing::{get, post}, Router};

/// Build the `/api/integrations/github` sub-router.
pub fn github_router(state: Arc<GitHubState>) -> Router {
    Router::new()
        .route("/repos", get(list_repos))
        .route("/search", get(search_issues))
        .route("/import", post(import_issues))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a [`GitHubClient`] from the stored token.
async fn build_client(
    state: &GitHubState,
) -> Result<GitHubClient, axum::response::Response> {
    let guard = state.token.read().await;
    let token = guard.as_ref().ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "GitHub token not configured. Set a personal access token first."
            })),
        )
            .into_response()
    })?;

    Ok(GitHubClient::new(token.clone()))
}

fn github_error_to_http(e: &GitHubError) -> (StatusCode, String) {
    match e {
        GitHubError::AuthError { .. } => (StatusCode::UNAUTHORIZED, e.to_string()),
        GitHubError::HttpError(_) => (StatusCode::BAD_GATEWAY, e.to_string()),
        GitHubError::ParseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_error_to_http_maps_auth_error() {
        let err = GitHubError::AuthError { status: 401 };
        let (status, _) = github_error_to_http(&err);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn github_error_to_http_maps_parse_error() {
        let err = GitHubError::ParseError("bad json".into());
        let (status, _) = github_error_to_http(&err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn github_error_to_http_maps_http_error() {
        // We can't easily create a reqwest::Error, so test the other branches.
        let err = GitHubError::AuthError { status: 403 };
        let (status, msg) = github_error_to_http(&err);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(msg.contains("403"));
    }

    #[tokio::test]
    async fn build_client_returns_error_when_no_token() {
        let state = GitHubState::new();
        let result = build_client(&state).await;
        assert!(result.is_err(), "should fail when no token is set");
    }

    #[tokio::test]
    async fn build_client_succeeds_with_token() {
        let state = GitHubState::with_token("ghp_test123".into());
        let result = build_client(&state).await;
        assert!(result.is_ok(), "should succeed when token is set");
    }

    #[test]
    fn github_state_new_has_no_token() {
        let state = GitHubState::new();
        let token = state.token.try_read().unwrap();
        assert!(token.is_none());
    }

    #[test]
    fn github_state_with_token_stores_token() {
        let state = GitHubState::with_token("tok".into());
        let token = state.token.try_read().unwrap();
        assert_eq!(token.as_deref(), Some("tok"));
    }

    #[test]
    fn import_response_serializes() {
        let resp = ImportResponse {
            imported: 3,
            message: "Stub: 3 issue(s) imported".into(),
        };
        let json = serde_json::to_value(&resp).expect("serialize");
        assert_eq!(json["imported"], 3);
    }
}
