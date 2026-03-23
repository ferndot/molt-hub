//! Axum HTTP handlers for the Jira integration endpoints.
//!
//! Routes:
//!   GET  /api/integrations/jira/search   — search / preview issues
//!   POST /api/integrations/jira/import   — import selected issues
//!   GET  /api/integrations/jira/projects — list projects
//!
//! Authentication is handled via OAuth 2.0 — see `oauth_handlers.rs`.
//! Credentials are read from the shared [`OAuthState`] rather than from
//! request bodies.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use molt_hub_core::events::store::EventStore;
use molt_hub_core::model::SessionId;

use super::import_service::ImportService;
use super::jira_client::{JiraClient, JiraError};
use super::oauth_handlers::OAuthState;

// ---------------------------------------------------------------------------
// Shared app state for integration routes
// ---------------------------------------------------------------------------

/// State injected into integration handlers.
pub struct IntegrationState<S: EventStore + 'static> {
    pub store: Arc<S>,
    pub oauth: Arc<OAuthState>,
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
}

/// Body for the import endpoint.
#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub issue_keys: Vec<String>,
    /// Optional cloud ID override; defaults to the first accessible site.
    pub cloud_id: Option<String>,
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
pub async fn search_issues<S: EventStore + 'static>(
    State(state): State<Arc<IntegrationState<S>>>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let client = match build_client(&state.oauth, query.cloud_id.as_deref()).await {
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
pub async fn import_issues<S: EventStore + 'static>(
    State(state): State<Arc<IntegrationState<S>>>,
    Json(body): Json<ImportRequest>,
) -> impl IntoResponse {
    let client = match build_client(&state.oauth, body.cloud_id.as_deref()).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let session_id = SessionId::new();
    let svc = ImportService::new(client, Arc::clone(&state.store), session_id);

    let mut imported = Vec::with_capacity(body.issue_keys.len());
    for key in &body.issue_keys {
        match svc.import_issue(key).await {
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
pub async fn list_projects<S: EventStore + 'static>(
    State(state): State<Arc<IntegrationState<S>>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let cloud_id = params.get("cloud_id").map(|s| s.as_str());
    let client = match build_client(&state.oauth, cloud_id).await {
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
// Router builder
// ---------------------------------------------------------------------------

use axum::{routing::{get, post}, Router};

/// Build the `/api/integrations/jira` sub-router.
///
/// `state` is shared across all handlers that need store and OAuth access.
pub fn jira_router<S: EventStore + Clone + 'static>(state: Arc<IntegrationState<S>>) -> Router {
    Router::new()
        .route("/search", get(search_issues::<S>))
        .route("/import", post(import_issues::<S>))
        .route("/projects", get(list_projects::<S>))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a [`JiraClient`] from the stored OAuth tokens.
///
/// Selects `cloud_id` if provided, otherwise falls back to the first stored site.
/// Returns an error response if no tokens are stored.
async fn build_client(
    oauth: &OAuthState,
    cloud_id: Option<&str>,
) -> Result<JiraClient, axum::response::Response> {
    let stored = oauth.stored_tokens.lock().await;
    let tokens = stored.as_ref().ok_or_else(|| {
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
        tokens
            .sites
            .first()
            .map(|s| s.id.clone())
            .ok_or_else(|| {
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({
                        "error": "No Atlassian sites found. Please re-authorize."
                    })),
                )
                    .into_response()
            })?
    };

    Ok(JiraClient::from_oauth(&selected_cloud_id, &tokens.access_token))
}

fn jira_error_to_http(e: &JiraError) -> (StatusCode, String) {
    match e {
        JiraError::AuthError { .. } => (StatusCode::UNAUTHORIZED, e.to_string()),
        JiraError::NotFound { .. } => (StatusCode::NOT_FOUND, e.to_string()),
        JiraError::HttpError(_) => (StatusCode::BAD_GATEWAY, e.to_string()),
        JiraError::ParseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn build_client_returns_error_when_no_tokens() {
        use super::super::oauth::JiraOAuthService;
        let svc = JiraOAuthService::new("https://example.com/cb");
        let oauth = OAuthState::new(svc);

        let result = build_client(&oauth, None).await;
        assert!(result.is_err(), "should fail when no tokens are stored");
    }

    #[tokio::test]
    async fn build_client_returns_error_when_no_sites() {
        use super::super::oauth::JiraOAuthService;
        use super::super::oauth_handlers::StoredTokens;

        let svc = JiraOAuthService::new("https://example.com/cb");
        let oauth = OAuthState::new(svc);

        {
            let mut stored = oauth.stored_tokens.lock().await;
            *stored = Some(StoredTokens {
                access_token: "tok".into(),
                refresh_token: None,
                expires_in: 3600,
                scope: "read:jira-work".into(),
                sites: vec![], // no sites
            });
        }

        let result = build_client(&oauth, None).await;
        assert!(result.is_err(), "should fail when no sites are stored");
    }

    #[tokio::test]
    async fn build_client_succeeds_with_tokens_and_site() {
        use super::super::oauth::{CloudResource, JiraOAuthService};
        use super::super::oauth_handlers::StoredTokens;

        let svc = JiraOAuthService::new("https://example.com/cb");
        let oauth = OAuthState::new(svc);

        {
            let mut stored = oauth.stored_tokens.lock().await;
            *stored = Some(StoredTokens {
                access_token: "my-token".into(),
                refresh_token: Some("ref".into()),
                expires_in: 3600,
                scope: "read:jira-work".into(),
                sites: vec![CloudResource {
                    id: "cloud-abc".into(),
                    name: "my-org".into(),
                    url: "https://my-org.atlassian.net".into(),
                }],
            });
        }

        let client = build_client(&oauth, None).await.expect("should succeed");
        // Verify the client was built with correct cloud ID.
        assert!(client.base_url.contains("cloud-abc"));
        assert_eq!(client.access_token, "my-token");
    }

    #[tokio::test]
    async fn build_client_uses_provided_cloud_id() {
        use super::super::oauth::{CloudResource, JiraOAuthService};
        use super::super::oauth_handlers::StoredTokens;

        let svc = JiraOAuthService::new("https://example.com/cb");
        let oauth = OAuthState::new(svc);

        {
            let mut stored = oauth.stored_tokens.lock().await;
            *stored = Some(StoredTokens {
                access_token: "tok".into(),
                refresh_token: None,
                expires_in: 3600,
                scope: "read:jira-work".into(),
                sites: vec![CloudResource {
                    id: "default-cloud".into(),
                    name: "default".into(),
                    url: "https://default.atlassian.net".into(),
                }],
            });
        }

        let client = build_client(&oauth, Some("override-cloud")).await.expect("should succeed");
        assert!(client.base_url.contains("override-cloud"));
    }
}
