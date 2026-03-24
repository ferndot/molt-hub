//! Axum HTTP handlers for the GitHub integration endpoints.
//!
//! Routes (under `/api/integrations/github`):
//!   GET  /repos   — list repos
//!   GET  /search  — search issues (GitHub `q` string)
//!   GET  /issues  — list/search issues (UI: owner, repo, state, labels)
//!   POST /import  — import selected issues into the event store (when available)
//!
//! Authentication uses the same OAuth tokens as `/auth` and `/oauth/callback`
//! via [`GithubOAuthState`] inside [`GithubAppState`].

use std::sync::Arc;

use axum::{
    extract::{FromRef, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use molt_hub_core::events::SqliteEventStore;
use molt_hub_core::model::SessionId;

use crate::credentials::CredentialScope;

use super::github_client::{GitHubClient, GitHubError, GitHubIssue};
use super::github_import_service::GithubImportService;
use super::github_oauth_handlers::{
    github_auth, github_disconnect, github_oauth_callback, github_status, GithubOAuthState,
    GithubOAuthStateRef,
};

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

/// Shared state for GitHub OAuth and REST handlers.
#[derive(Clone)]
pub struct GithubAppState {
    pub oauth: Arc<GithubOAuthState>,
    /// When `None`, the server started without SQLite; [`import_issues`] returns 503.
    pub store: Option<Arc<SqliteEventStore>>,
}

impl FromRef<GithubAppState> for GithubOAuthStateRef {
    fn from_ref(state: &GithubAppState) -> Self {
        GithubOAuthStateRef(state.oauth.clone())
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
    /// Optional monitored project ULID — selects per-project GitHub OAuth tokens.
    #[serde(default, rename = "projectId")]
    pub project_id: Option<String>,
}

/// Query parameters for `GET /issues` (Solid UI).
#[derive(Debug, Deserialize)]
pub struct IssuesQuery {
    pub owner: String,
    pub repo: String,
    /// `open`, `closed`, or `all` (default `open`).
    #[serde(default = "default_issue_state_filter")]
    pub state: String,
    /// Comma-separated label names.
    #[serde(default)]
    pub labels: String,
    #[serde(default, rename = "projectId")]
    pub project_id: Option<String>,
}

fn default_issue_state_filter() -> String {
    "open".to_owned()
}

/// Issue row shape expected by the web UI.
#[derive(Debug, Serialize)]
pub struct GitHubIssueUi {
    pub number: i64,
    pub title: String,
    pub state: String,
    pub labels: Vec<String>,
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
    #[serde(default, rename = "projectId")]
    pub project_id: Option<String>,
}

/// Response body for a successful import.
#[derive(Debug, Serialize)]
pub struct ImportResponse {
    /// Issues processed successfully (new imports plus idempotent skips).
    pub imported: usize,
    /// Human-readable message.
    pub message: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// List GitHub repositories visible to the authenticated user.
#[instrument(skip_all)]
pub async fn list_repos(
    State(state): State<GithubAppState>,
    Query(project): Query<super::integration_params::ProjectIdQuery>,
) -> impl IntoResponse {
    let scope = project.credential_scope();
    let client = match build_client(&state.oauth, &scope).await {
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

/// Search issues in a GitHub repository (raw `q` appended to `repo:owner/name`).
#[instrument(skip_all, fields(owner = %query.owner, repo = %query.repo))]
pub async fn search_issues(
    State(state): State<GithubAppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let scope = crate::credentials::credential_scope_for_integration(query.project_id.as_deref());
    let client = match build_client(&state.oauth, &scope).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    match client
        .search_issues(&query.owner, &query.repo, &query.q)
        .await
    {
        Ok(issues) => Json(issues).into_response(),
        Err(e) => {
            let (status, msg) = github_error_to_http(&e);
            (status, Json(serde_json::json!({ "error": msg }))).into_response()
        }
    }
}

/// Search issues using UI-friendly query parameters (`state`, `labels`).
#[instrument(skip_all, fields(owner = %query.owner, repo = %query.repo))]
pub async fn list_issues_ui(
    State(state): State<GithubAppState>,
    Query(query): Query<IssuesQuery>,
) -> impl IntoResponse {
    let scope = crate::credentials::credential_scope_for_integration(query.project_id.as_deref());
    let client = match build_client(&state.oauth, &scope).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let q = build_github_search_q(&query.state, &query.labels);
    match client.search_issues(&query.owner, &query.repo, &q).await {
        Ok(issues) => {
            let ui: Vec<GitHubIssueUi> = issues.into_iter().map(issue_to_ui).collect();
            Json(ui).into_response()
        }
        Err(e) => {
            let (status, msg) = github_error_to_http(&e);
            (status, Json(serde_json::json!({ "error": msg }))).into_response()
        }
    }
}

/// Import selected GitHub issues into the event store.
#[instrument(skip_all, fields(count = body.issues.len()))]
pub async fn import_issues(
    State(state): State<GithubAppState>,
    Json(body): Json<ImportRequest>,
) -> impl IntoResponse {
    let scope = crate::credentials::credential_scope_for_integration(body.project_id.as_deref());
    let store = match state.store.as_ref() {
        Some(s) => Arc::clone(s),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Event store is not available; cannot import issues."
                })),
            )
                .into_response();
        }
    };

    let client = match build_client(&state.oauth, &scope).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    if body.issues.is_empty() {
        return Json(ImportResponse {
            imported: 0,
            message: "No issues selected.".to_owned(),
        })
        .into_response();
    }

    let session_id = SessionId::new();
    let svc = GithubImportService::new(client, store, session_id);

    match svc
        .import_issues(&body.owner, &body.repo, &body.issues)
        .await
    {
        Ok((new_count, skipped)) => {
            let total = new_count + skipped;
            let message = if skipped == 0 {
                format!(
                    "Imported {new_count} issue(s) from {}/{}.",
                    body.owner, body.repo
                )
            } else if new_count == 0 {
                format!(
                    "All {skipped} issue(s) from {}/{} were already imported.",
                    body.owner, body.repo
                )
            } else {
                format!(
                    "Imported {new_count} new issue(s) from {}/{}; {skipped} already in hub.",
                    body.owner, body.repo
                )
            };
            Json(ImportResponse {
                imported: total,
                message,
            })
            .into_response()
        }
        Err(e) => {
            let (status, msg) = github_import_error_to_http(&e);
            (status, Json(serde_json::json!({ "error": msg }))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// GitHub OAuth + REST integration routes sharing [`GithubAppState`].
pub fn github_integrations_router(state: GithubAppState) -> Router {
    Router::new()
        .route("/auth", get(github_auth))
        .route("/oauth/callback", get(github_oauth_callback))
        .route("/status", get(github_status))
        .route("/disconnect", post(github_disconnect))
        .route("/repos", get(list_repos))
        .route("/search", get(search_issues))
        .route("/issues", get(list_issues_ui))
        .route("/import", post(import_issues))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn build_github_search_q(state_filter: &str, labels_csv: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    match state_filter {
        "closed" => parts.push("is:closed".to_owned()),
        "all" => {}
        _ => parts.push("is:open".to_owned()),
    }
    for label in labels_csv
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("label:{label}"));
    }
    parts.join(" ")
}

fn issue_to_ui(issue: GitHubIssue) -> GitHubIssueUi {
    GitHubIssueUi {
        number: issue.number,
        title: issue.title,
        state: issue.state,
        labels: issue.labels.into_iter().map(|l| l.name).collect(),
    }
}

async fn build_client(
    oauth: &GithubOAuthState,
    scope: &CredentialScope,
) -> Result<GitHubClient, axum::response::Response> {
    oauth.ensure_tokens_loaded(scope).await;
    let map = oauth.stored_tokens.lock().await;
    let tokens = map.get(scope).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "Not authenticated. Please complete GitHub OAuth authorization."
            })),
        )
            .into_response()
    })?;

    Ok(GitHubClient::new(tokens.access_token.clone()))
}

fn github_error_to_http(e: &GitHubError) -> (StatusCode, String) {
    match e {
        GitHubError::AuthError { .. } => (StatusCode::UNAUTHORIZED, e.to_string()),
        GitHubError::NotFound { .. } => (StatusCode::NOT_FOUND, e.to_string()),
        GitHubError::HttpError(_) => (StatusCode::BAD_GATEWAY, e.to_string()),
        GitHubError::ParseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn github_import_error_to_http(
    e: &super::github_import_service::GithubImportError,
) -> (StatusCode, String) {
    match e {
        super::github_import_service::GithubImportError::GitHub(ge) => github_error_to_http(ge),
        super::github_import_service::GithubImportError::EventStore(es) => {
            (StatusCode::INTERNAL_SERVER_ERROR, es.to_string())
        }
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
        let err = GitHubError::AuthError { status: 403 };
        let (status, msg) = github_error_to_http(&err);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(msg.contains("403"));
    }

    #[test]
    fn github_error_to_http_maps_not_found() {
        let err = GitHubError::NotFound {
            repo: "o/r".into(),
            number: 99,
        };
        let (status, _) = github_error_to_http(&err);
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn build_client_returns_error_when_no_token() {
        use crate::credentials::MemoryStore;
        use crate::integrations::github_oauth::GithubOAuthService;

        let svc = GithubOAuthService::with_credentials(
            "https://example.com/gh-cb",
            "cid".into(),
            Some("sec".into()),
        );
        let store = Arc::new(MemoryStore::new());
        let oauth = GithubOAuthState::new(svc, store);

        let result = build_client(&oauth, &CredentialScope::Global).await;
        assert!(result.is_err(), "should fail when no token is set");
    }

    #[tokio::test]
    async fn build_client_succeeds_with_token() {
        use crate::credentials::{CredentialScope, MemoryStore};
        use crate::integrations::github_oauth::GithubOAuthService;
        use crate::integrations::github_oauth_handlers::GithubStoredTokens;

        let svc = GithubOAuthService::with_credentials(
            "https://example.com/gh-cb",
            "cid".into(),
            Some("sec".into()),
        );
        let store = Arc::new(MemoryStore::new());
        let oauth = GithubOAuthState::new(svc, store);

        {
            let mut map = oauth.stored_tokens.lock().await;
            map.insert(
                CredentialScope::Global,
                GithubStoredTokens {
                    access_token: "ghp_test123".into(),
                    refresh_token: None,
                    expires_in: Some(3600),
                    scope: "repo".into(),
                    login: None,
                },
            );
        }

        let result = build_client(&oauth, &CredentialScope::Global).await;
        assert!(result.is_ok(), "should succeed when token is set");
    }

    #[test]
    fn build_github_search_q_open_and_labels() {
        let q = build_github_search_q("open", "bug, ui");
        assert!(q.contains("is:open"));
        assert!(q.contains("label:bug"));
        assert!(q.contains("label:ui"));
    }

    #[test]
    fn build_github_search_q_all_skips_is() {
        let q = build_github_search_q("all", "");
        assert!(!q.contains("is:"));
    }

    #[test]
    fn import_response_serializes() {
        let resp = ImportResponse {
            imported: 3,
            message: "Imported 3 issue(s) from o/r.".into(),
        };
        let json = serde_json::to_value(&resp).expect("serialize");
        assert_eq!(json["imported"], 3);
    }
}
