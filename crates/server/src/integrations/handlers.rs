//! Axum HTTP handlers for the Jira integration endpoints.
//!
//! Routes (under `/api/integrations/jira` when the event store is available):
//!   GET  /search   — search / preview issues
//!   POST /import   — import selected issues
//!   GET  /projects — list projects
//!
//! OAuth routes (`/auth`, `/oauth/callback`, `/status`, `/disconnect`) share
//! [`JiraOAuthState`] with these handlers via [`JiraAppState`] so import/search
//! use the same tokens as the OAuth flow.

use std::sync::Arc;

use axum::{
    extract::{Extension, FromRef, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use molt_hub_core::events::SqliteEventStore;
use molt_hub_core::model::SessionId;

use crate::credentials::{credential_scope_for_integration, CredentialScope};
use crate::ws::ConnectionManager;

use super::import_service::ImportService;
use super::jira_client::{JiraClient, JiraError};
use super::jira_oauth_handlers::{
    jira_auth, jira_disconnect, jira_oauth_callback, jira_status, JiraOAuthState, JiraOAuthStateRef,
};

// ---------------------------------------------------------------------------
// App state: OAuth + event store (import/search)
// ---------------------------------------------------------------------------

/// State for Jira OAuth and REST integration routes mounted together.
#[derive(Clone)]
pub struct JiraAppState {
    pub oauth: Arc<JiraOAuthState>,
    pub store: Arc<SqliteEventStore>,
}

impl FromRef<JiraAppState> for JiraOAuthStateRef {
    fn from_ref(state: &JiraAppState) -> Self {
        JiraOAuthStateRef(state.oauth.clone())
    }
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Query parameters for the search endpoint.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub jql: String,
    /// Optional cloud ID override; defaults to the first accessible site.
    pub cloud_id: Option<String>,
    #[serde(default, rename = "projectId")]
    pub project_id: Option<String>,
}

/// Body for the import endpoint.
#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub issue_keys: Vec<String>,
    /// Optional cloud ID override; defaults to the first accessible site.
    pub cloud_id: Option<String>,
    #[serde(default, rename = "projectId")]
    pub project_id: Option<String>,
    /// Pipeline stage id on the active board (e.g. first column). Defaults to `backlog`.
    #[serde(default, rename = "initialStage")]
    pub initial_stage: Option<String>,
}

/// Response body for a successful import.
#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub imported: Vec<String>,
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/jira/search
// ---------------------------------------------------------------------------

/// Search Jira issues (preview without importing).
#[instrument(skip_all, fields(jql = %query.jql))]
pub async fn search_issues(
    State(state): State<JiraAppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let scope = credential_scope_for_integration(query.project_id.as_deref());
    let client = match build_client(&state.oauth, query.cloud_id.as_deref(), &scope).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    match client.search_issues(&query.jql, 50).await {
        Ok(issues) => Json(issues).into_response(),
        Err(e) => {
            let (status, msg) = jira_error_to_http(&e);
            (status, Json(serde_json::json!({ "error": msg }))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Handler: POST /api/integrations/jira/import
// ---------------------------------------------------------------------------

/// Import the specified issues into Molt Hub.
#[instrument(skip_all, fields(count = body.issue_keys.len()))]
pub async fn import_issues(
    State(state): State<JiraAppState>,
    Extension(manager): Extension<Arc<ConnectionManager>>,
    Json(body): Json<ImportRequest>,
) -> impl IntoResponse {
    let scope = credential_scope_for_integration(body.project_id.as_deref());
    let client = match build_client(&state.oauth, body.cloud_id.as_deref(), &scope).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let session_id = SessionId::new();
    let svc = ImportService::new(client, Arc::clone(&state.store), session_id);

    let stage = body
        .initial_stage
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("backlog");
    let project_id = body
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("default");
    let broadcast = Some((manager.as_ref(), project_id));

    let mut imported = Vec::with_capacity(body.issue_keys.len());
    for key in &body.issue_keys {
        match svc.import_issue(key, stage, broadcast).await {
            Ok(task_id) => imported.push(task_id.to_string()),
            Err(e) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({ "error": format!("Failed to import {key}: {e}") })),
                )
                    .into_response();
            }
        }
    }

    Json(ImportResponse { imported }).into_response()
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/jira/projects
// ---------------------------------------------------------------------------

/// List all Jira projects visible to the authenticated user.
#[instrument(skip_all)]
pub async fn list_projects(
    State(state): State<JiraAppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let cloud_id = params.get("cloud_id").map(|s| s.as_str());
    let scope = credential_scope_for_integration(
        params
            .get("projectId")
            .or_else(|| params.get("project_id"))
            .map(|s| s.as_str()),
    );
    let client = match build_client(&state.oauth, cloud_id, &scope).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    match client.list_projects().await {
        Ok(projects) => Json(projects).into_response(),
        Err(e) => {
            let (status, msg) = jira_error_to_http(&e);
            (status, Json(serde_json::json!({ "error": msg }))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Router builders
// ---------------------------------------------------------------------------

/// OAuth + Jira REST routes sharing [`JiraAppState`].
///
/// Mount at `/api/integrations/jira` when the SQLite event store is available.
pub fn jira_integrations_router(state: JiraAppState) -> Router {
    Router::new()
        .route("/auth", get(jira_auth))
        .route("/oauth/callback", get(jira_oauth_callback))
        .route("/status", get(jira_status))
        .route("/disconnect", post(jira_disconnect))
        .route("/search", get(search_issues))
        .route("/import", post(import_issues))
        .route("/projects", get(list_projects))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a [`JiraClient`] from the stored OAuth tokens.
///
/// Selects `cloud_id` if provided, otherwise the cloud ID from the OAuth session.
/// Returns an error response if not authenticated or cloud ID is unknown.
async fn build_client(
    oauth: &JiraOAuthState,
    cloud_id: Option<&str>,
    scope: &CredentialScope,
) -> Result<JiraClient, axum::response::Response> {
    oauth.ensure_tokens_loaded(scope).await;
    let map = oauth.stored_tokens.lock().await;
    let tokens = map.get(scope).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "Not authenticated. Please complete OAuth authorization."
            })),
        )
            .into_response()
    })?;

    let selected_cloud_id = if let Some(id) = cloud_id {
        id.to_owned()
    } else {
        tokens.cloud_id.clone().ok_or_else(|| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({
                    "error": "No Atlassian cloud ID stored. Please disconnect and re-authorize Jira."
                })),
            )
                .into_response()
        })?
    };

    Ok(JiraClient::from_oauth(
        &selected_cloud_id,
        &tokens.access_token,
    ))
}

fn jira_error_to_http(e: &JiraError) -> (StatusCode, String) {
    match e {
        JiraError::AuthError { .. } => (StatusCode::UNAUTHORIZED, e.to_string()),
        JiraError::NotFound { .. } => (StatusCode::NOT_FOUND, e.to_string()),
        JiraError::HttpError(_) => (StatusCode::BAD_GATEWAY, e.to_string()),
        JiraError::ParseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        JiraError::ApiError { status, message } => {
            let code = StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY);
            let http = if code.is_client_error() {
                code
            } else {
                StatusCode::BAD_GATEWAY
            };
            (http, message.clone())
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{CredentialScope, MemoryStore};

    use super::super::jira_oauth_handlers::JiraStoredTokens;
    use super::super::oauth::JiraOAuthService;

    #[test]
    fn jira_error_to_http_maps_auth_error() {
        let err = JiraError::AuthError { status: 401 };
        let (status, _) = jira_error_to_http(&err);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn jira_error_to_http_maps_not_found() {
        let err = JiraError::NotFound {
            key: "PROJ-1".into(),
        };
        let (status, _) = jira_error_to_http(&err);
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn jira_error_to_http_maps_parse_error() {
        let err = JiraError::ParseError("bad json".into());
        let (status, _) = jira_error_to_http(&err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn jira_error_to_http_maps_api_error_client_status() {
        let err = JiraError::ApiError {
            status: 400,
            message: "Invalid JQL".into(),
        };
        let (status, msg) = jira_error_to_http(&err);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(msg, "Invalid JQL");
    }

    #[tokio::test]
    async fn build_client_returns_error_when_no_tokens() {
        let svc = JiraOAuthService::new("https://example.com/cb");
        let store = Arc::new(MemoryStore::new());
        let oauth = JiraOAuthState::new(svc, store);

        let result = build_client(&oauth, None, &CredentialScope::Global).await;
        assert!(result.is_err(), "should fail when no tokens are stored");
    }

    #[tokio::test]
    async fn build_client_returns_error_when_no_cloud_id() {
        let svc = JiraOAuthService::new("https://example.com/cb");
        let store = Arc::new(MemoryStore::new());
        let oauth = JiraOAuthState::new(svc, store);

        {
            let mut map = oauth.stored_tokens.lock().await;
            map.insert(
                CredentialScope::Global,
                JiraStoredTokens {
                    access_token: "tok".into(),
                    refresh_token: None,
                    expires_in: 3600,
                    scope: "read:jira-work".into(),
                    cloud_id: None,
                    site_url: None,
                    site_name: None,
                },
            );
        }

        let result = build_client(&oauth, None, &CredentialScope::Global).await;
        assert!(result.is_err(), "should fail when no cloud_id is stored");
    }

    #[tokio::test]
    async fn build_client_succeeds_with_tokens_and_cloud_id() {
        let svc = JiraOAuthService::new("https://example.com/cb");
        let store = Arc::new(MemoryStore::new());
        let oauth = JiraOAuthState::new(svc, store);

        {
            let mut map = oauth.stored_tokens.lock().await;
            map.insert(
                CredentialScope::Global,
                JiraStoredTokens {
                    access_token: "my-token".into(),
                    refresh_token: Some("ref".into()),
                    expires_in: 3600,
                    scope: "read:jira-work".into(),
                    cloud_id: Some("cloud-abc".into()),
                    site_url: Some("https://my-org.atlassian.net".into()),
                    site_name: Some("my-org".into()),
                },
            );
        }

        let client = build_client(&oauth, None, &CredentialScope::Global)
            .await
            .expect("should succeed");
        assert!(client.base_url.contains("cloud-abc"));
        assert_eq!(client.access_token, "my-token");
    }

    #[tokio::test]
    async fn build_client_uses_provided_cloud_id() {
        let svc = JiraOAuthService::new("https://example.com/cb");
        let store = Arc::new(MemoryStore::new());
        let oauth = JiraOAuthState::new(svc, store);

        {
            let mut map = oauth.stored_tokens.lock().await;
            map.insert(
                CredentialScope::Global,
                JiraStoredTokens {
                    access_token: "tok".into(),
                    refresh_token: None,
                    expires_in: 3600,
                    scope: "read:jira-work".into(),
                    cloud_id: Some("default-cloud".into()),
                    site_url: None,
                    site_name: None,
                },
            );
        }

        let client = build_client(&oauth, Some("override-cloud"), &CredentialScope::Global)
            .await
            .expect("should succeed");
        assert!(client.base_url.contains("override-cloud"));
    }
}
