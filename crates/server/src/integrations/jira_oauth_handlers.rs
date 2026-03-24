//! Axum HTTP handlers for the Jira (Atlassian) OAuth 2.0 (3LO + PKCE) flow.
//!
//! Routes (mounted at `/api/integrations/jira`):
//!   GET    /auth              — returns authorization URL + CSRF state
//!   GET    /oauth/callback    — exchanges code for tokens, fetches site info
//!   GET    /status            — returns `{ connected, site_url? }`
//!   POST   /disconnect        — clears stored tokens

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tracing::{instrument, warn};

use super::integration_params::ProjectIdQuery;
use super::oauth::{JiraOAuthService, OAuthError, DEFAULT_SCOPES};
use super::oauth_common::{oauth_success_html, random_oauth_state};
use crate::credentials::{CredentialScope, CredentialStore};

// ---------------------------------------------------------------------------
// Credential store keys
// ---------------------------------------------------------------------------

const KEY_ACCESS_TOKEN: &str = "jira/access_token";
const KEY_REFRESH_TOKEN: &str = "jira/refresh_token";
const KEY_SCOPE: &str = "jira/scope";
const KEY_SITE_URL: &str = "jira/site_url";
const KEY_SITE_NAME: &str = "jira/site_name";
const KEY_CLOUD_ID: &str = "jira/cloud_id";

// ---------------------------------------------------------------------------
// Shared OAuth state
// ---------------------------------------------------------------------------

/// Axum [`State`] wrapper for `FromRef<Arc<JiraAppState>>` (see `handlers::JiraAppState`).
#[derive(Clone)]
pub struct JiraOAuthStateRef(pub Arc<JiraOAuthState>);

impl Deref for JiraOAuthStateRef {
    type Target = JiraOAuthState;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// State injected into Jira OAuth handlers.
pub struct JiraOAuthState {
    pub service: JiraOAuthService,
    /// In-memory OAuth tokens keyed by [`CredentialScope`].
    pub stored_tokens: tokio::sync::Mutex<HashMap<CredentialScope, JiraStoredTokens>>,
    /// Pending PKCE verifiers + scope keyed by CSRF state.
    pub pkce_verifiers: DashMap<String, (String, CredentialScope)>,
    /// Persistent credential store (OS keychain or in-memory fallback).
    pub credential_store: Arc<dyn CredentialStore>,
}

/// Tokens stored after a successful Jira OAuth exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraStoredTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub scope: String,
    /// Atlassian cloud ID for REST URLs (`/ex/jira/{cloud_id}/...`).
    pub cloud_id: Option<String>,
    /// The URL of the first accessible Atlassian cloud site.
    pub site_url: Option<String>,
    /// The name of the first accessible Atlassian cloud site.
    pub site_name: Option<String>,
}

impl JiraOAuthState {
    /// Create state from an already-constructed [`JiraOAuthService`] and a
    /// [`CredentialStore`] for token persistence across restarts.
    pub fn new(service: JiraOAuthService, credential_store: Arc<dyn CredentialStore>) -> Self {
        Self {
            service,
            stored_tokens: tokio::sync::Mutex::new(HashMap::new()),
            pkce_verifiers: DashMap::new(),
            credential_store,
        }
    }

    /// Load tokens from the credential store for `scope` when the in-memory cache has no entry.
    pub async fn ensure_tokens_loaded(&self, scope: &CredentialScope) {
        let mut map = self.stored_tokens.lock().await;
        if map.contains_key(scope) {
            return;
        }
        if let Ok(access_token) = self.credential_store.retrieve(KEY_ACCESS_TOKEN, scope) {
            let refresh_token = self
                .credential_store
                .retrieve(KEY_REFRESH_TOKEN, scope)
                .ok();
            let oauth_scope = self
                .credential_store
                .retrieve(KEY_SCOPE, scope)
                .unwrap_or_default();
            let site_url = self.credential_store.retrieve(KEY_SITE_URL, scope).ok();
            let site_name = self.credential_store.retrieve(KEY_SITE_NAME, scope).ok();
            let cloud_id = self.credential_store.retrieve(KEY_CLOUD_ID, scope).ok();
            map.insert(
                scope.clone(),
                JiraStoredTokens {
                    access_token,
                    refresh_token,
                    expires_in: 0,
                    scope: oauth_scope,
                    cloud_id,
                    site_url,
                    site_name,
                },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Query parameters for the OAuth callback.
#[derive(Debug, Deserialize)]
pub struct JiraCallbackQuery {
    /// The authorization code returned by Atlassian.
    pub code: String,
    /// The CSRF state token.
    pub state: String,
}

/// Response from the auth endpoint.
#[derive(Debug, Serialize)]
pub struct JiraAuthResponse {
    pub url: String,
    pub state: String,
}

/// Response from the callback endpoint.
#[derive(Debug, Serialize)]
pub struct JiraCallbackResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
}

/// Response from the status endpoint.
#[derive(Debug, Serialize)]
pub struct JiraStatusResponse {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
}

/// Response from the disconnect endpoint.
#[derive(Debug, Serialize)]
pub struct JiraDisconnectResponse {
    pub success: bool,
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/jira/auth
// ---------------------------------------------------------------------------

/// Generate a Jira authorization URL with a fresh CSRF state token.
#[instrument(skip_all)]
pub async fn jira_auth(
    State(state): State<JiraOAuthStateRef>,
    Query(project): Query<ProjectIdQuery>,
) -> impl IntoResponse {
    let csrf_state = random_oauth_state();
    let cred_scope = project.credential_scope();

    let (url, verifier) = state.service.authorization_url(&csrf_state, DEFAULT_SCOPES);

    state
        .pkce_verifiers
        .insert(csrf_state.clone(), (verifier, cred_scope));

    Json(JiraAuthResponse {
        url,
        state: csrf_state,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/jira/oauth/callback
// ---------------------------------------------------------------------------

/// Exchange the authorization code for tokens and fetch accessible site info.
#[instrument(skip_all, fields(state = %query.state))]
pub async fn jira_oauth_callback(
    State(app_state): State<JiraOAuthStateRef>,
    Query(query): Query<JiraCallbackQuery>,
) -> impl IntoResponse {
    // Retrieve (and consume) the PKCE verifier for this CSRF state.
    let (verifier, cred_scope) = match app_state.pkce_verifiers.remove(&query.state) {
        Some((_, pair)) => pair,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": "Unknown or already-used state token. Please restart the OAuth flow."
                })),
            )
                .into_response();
        }
    };

    let tokens = match app_state
        .service
        .exchange_code(&query.code, &verifier)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            let (status, msg) = jira_oauth_error_to_http(&e);
            return (
                status,
                Json(serde_json::json!({ "success": false, "error": msg })),
            )
                .into_response();
        }
    };

    // Fetch accessible resources to get site URL and cloud ID for REST.
    let (site_url, site_name, cloud_id) = match app_state
        .service
        .get_accessible_resources(&tokens.access_token)
        .await
    {
        Ok(sites) => {
            let first = sites.into_iter().next();
            (
                first.as_ref().map(|s| s.url.clone()),
                first.as_ref().map(|s| s.name.clone()),
                first.map(|s| s.id),
            )
        }
        Err(_) => (None, None, None),
    };

    // Build token struct.
    let new_tokens = JiraStoredTokens {
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        expires_in: tokens.expires_in,
        scope: tokens.scope.clone(),
        cloud_id: cloud_id.clone(),
        site_url: site_url.clone(),
        site_name: site_name.clone(),
    };

    // Persist to credential store (best-effort — log on failure but don't abort).
    if let Err(e) =
        app_state
            .credential_store
            .store(KEY_ACCESS_TOKEN, &cred_scope, &tokens.access_token)
    {
        warn!(error = %e, "failed to persist Jira access token to credential store");
    }
    if let Some(ref rt) = tokens.refresh_token {
        if let Err(e) = app_state
            .credential_store
            .store(KEY_REFRESH_TOKEN, &cred_scope, rt)
        {
            warn!(error = %e, "failed to persist Jira refresh token to credential store");
        }
    }
    if let Err(e) = app_state
        .credential_store
        .store(KEY_SCOPE, &cred_scope, &tokens.scope)
    {
        warn!(error = %e, "failed to persist Jira scope to credential store");
    }
    if let Some(ref url) = site_url {
        if let Err(e) = app_state
            .credential_store
            .store(KEY_SITE_URL, &cred_scope, url)
        {
            warn!(error = %e, "failed to persist Jira site_url to credential store");
        }
    }
    if let Some(ref name) = site_name {
        if let Err(e) = app_state
            .credential_store
            .store(KEY_SITE_NAME, &cred_scope, name)
        {
            warn!(error = %e, "failed to persist Jira site_name to credential store");
        }
    }
    if let Some(ref id) = cloud_id {
        if let Err(e) = app_state
            .credential_store
            .store(KEY_CLOUD_ID, &cred_scope, id)
        {
            warn!(error = %e, "failed to persist Jira cloud_id to credential store");
        }
    }

    // Cache in-memory.
    {
        let mut map = app_state.stored_tokens.lock().await;
        map.insert(cred_scope, new_tokens);
    }

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        oauth_success_html("Jira"),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/jira/status
// ---------------------------------------------------------------------------

/// Return the current Jira connection status.
///
/// If in-memory tokens are absent, attempts to load from the credential store
/// so that connections survive server restarts.
#[instrument(skip_all)]
pub async fn jira_status(
    State(app_state): State<JiraOAuthStateRef>,
    Query(project): Query<ProjectIdQuery>,
) -> impl IntoResponse {
    let cred_scope = project.credential_scope();
    app_state.ensure_tokens_loaded(&cred_scope).await;
    let map = app_state.stored_tokens.lock().await;

    match map.get(&cred_scope) {
        Some(tokens) => Json(JiraStatusResponse {
            connected: true,
            site_url: tokens.site_url.clone(),
            site_name: tokens.site_name.clone(),
        })
        .into_response(),
        None => Json(JiraStatusResponse {
            connected: false,
            site_url: None,
            site_name: None,
        })
        .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Handler: POST /api/integrations/jira/disconnect
// ---------------------------------------------------------------------------

/// Clear all stored Jira OAuth tokens (both in-memory and persisted).
#[instrument(skip_all)]
pub async fn jira_disconnect(
    State(app_state): State<JiraOAuthStateRef>,
    Query(project): Query<ProjectIdQuery>,
) -> impl IntoResponse {
    let cred_scope = project.credential_scope();
    // Clear the credential store (best-effort).
    for key in [
        KEY_ACCESS_TOKEN,
        KEY_REFRESH_TOKEN,
        KEY_SCOPE,
        KEY_SITE_URL,
        KEY_SITE_NAME,
        KEY_CLOUD_ID,
    ] {
        if let Err(e) = app_state.credential_store.delete(key, &cred_scope) {
            warn!(error = %e, key, "failed to delete Jira credential from store");
        }
    }

    let mut map = app_state.stored_tokens.lock().await;
    map.remove(&cred_scope);
    Json(JiraDisconnectResponse { success: true }).into_response()
}

// ---------------------------------------------------------------------------
// Helper: map Jira OAuth errors to HTTP status codes
// ---------------------------------------------------------------------------

fn jira_oauth_error_to_http(e: &OAuthError) -> (StatusCode, String) {
    match e {
        OAuthError::HttpError(_) => (StatusCode::BAD_GATEWAY, e.to_string()),
        OAuthError::AuthServerError { .. } => (StatusCode::UNAUTHORIZED, e.to_string()),
        OAuthError::ParseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        OAuthError::MissingClientSecret => (StatusCode::PRECONDITION_REQUIRED, e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

use axum::routing::{get, post};
use axum::Router;

/// Build the Jira OAuth sub-router.
///
/// Mounted at:
///   `/api/integrations/jira/auth`              — GET (start OAuth)
///   `/api/integrations/jira/oauth/callback`    — GET (handle redirect)
///   `/api/integrations/jira/status`            — GET (connection status)
///   `/api/integrations/jira/disconnect`        — POST (clear tokens)
pub fn jira_oauth_router(state: Arc<JiraOAuthState>) -> Router {
    Router::new()
        .route("/auth", get(jira_auth))
        .route("/oauth/callback", get(jira_oauth_callback))
        .route("/status", get(jira_status))
        .route("/disconnect", post(jira_disconnect))
        .with_state(JiraOAuthStateRef(state))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{CredentialScope, MemoryStore};

    fn make_state() -> Arc<JiraOAuthState> {
        let svc =
            JiraOAuthService::with_client_secret("https://example.com/cb", "test_secret".into());
        let store = Arc::new(MemoryStore::new());
        Arc::new(JiraOAuthState::new(svc, store))
    }

    #[test]
    fn random_oauth_state_is_nonempty() {
        let token = random_oauth_state();
        assert!(!token.is_empty());
        assert_eq!(token.len(), 32); // 16 bytes -> 32 hex chars
    }

    #[test]
    fn random_oauth_state_is_hex() {
        let token = random_oauth_state();
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit()),
            "state token is not valid hex: {token}"
        );
    }

    #[test]
    fn random_oauth_state_differs_across_calls() {
        let t1 = random_oauth_state();
        let t2 = random_oauth_state();
        assert_ne!(t1, t2, "two CSRF tokens must not be identical");
    }

    #[test]
    fn jira_oauth_error_to_http_maps_parse_error() {
        let err = OAuthError::ParseError("bad json".into());
        let (status, msg) = jira_oauth_error_to_http(&err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("bad json"));
    }

    #[test]
    fn jira_oauth_error_to_http_maps_auth_server_error() {
        let err = OAuthError::AuthServerError {
            error: "invalid_grant".into(),
            description: "Code expired".into(),
        };
        let (status, _) = jira_oauth_error_to_http(&err);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn jira_oauth_error_to_http_maps_missing_secret() {
        let err = OAuthError::MissingClientSecret;
        let (status, _) = jira_oauth_error_to_http(&err);
        assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
    }

    #[tokio::test]
    async fn stored_tokens_starts_empty() {
        let state = make_state();
        let map = state.stored_tokens.lock().await;
        assert!(map.is_empty());
    }

    #[tokio::test]
    async fn pkce_verifier_stored_on_auth() {
        let state = make_state();

        let response = jira_auth(
            State(JiraOAuthStateRef(Arc::clone(&state))),
            Query(ProjectIdQuery::default()),
        )
        .await;
        let _ = response.into_response();

        assert_eq!(
            state.pkce_verifiers.len(),
            1,
            "expected one stored PKCE verifier after auth"
        );
    }

    #[tokio::test]
    async fn callback_rejects_unknown_state() {
        let state = make_state();

        state.pkce_verifiers.insert(
            "known-state".to_string(),
            ("verifier123".to_string(), CredentialScope::Global),
        );

        let query = JiraCallbackQuery {
            code: "somecode".to_string(),
            state: "unknown-state".to_string(),
        };
        let response =
            jira_oauth_callback(State(JiraOAuthStateRef(Arc::clone(&state))), Query(query))
                .await
                .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(state.pkce_verifiers.contains_key("known-state"));
    }

    #[tokio::test]
    async fn disconnect_clears_stored_tokens() {
        let state = make_state();

        {
            let mut map = state.stored_tokens.lock().await;
            map.insert(
                CredentialScope::Global,
                JiraStoredTokens {
                    access_token: "tok".into(),
                    refresh_token: Some("ref".into()),
                    expires_in: 3600,
                    scope: "read:jira-work".into(),
                    cloud_id: Some("cloud-1".into()),
                    site_url: Some("https://my-org.atlassian.net".into()),
                    site_name: Some("my-org".into()),
                },
            );
        }

        let response = jira_disconnect(
            State(JiraOAuthStateRef(Arc::clone(&state))),
            Query(ProjectIdQuery::default()),
        )
        .await;
        let _ = response.into_response();

        let map = state.stored_tokens.lock().await;
        assert!(
            !map.contains_key(&CredentialScope::Global),
            "tokens should be cleared after disconnect"
        );
    }

    #[tokio::test]
    async fn status_returns_disconnected_when_no_tokens() {
        let state = make_state();
        let response = jira_status(
            State(JiraOAuthStateRef(Arc::clone(&state))),
            Query(ProjectIdQuery::default()),
        )
        .await;
        let resp = response.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn status_returns_connected_when_tokens_present() {
        let state = make_state();
        {
            let mut map = state.stored_tokens.lock().await;
            map.insert(
                CredentialScope::Global,
                JiraStoredTokens {
                    access_token: "tok".into(),
                    refresh_token: None,
                    expires_in: 3600,
                    scope: "read:jira-work".into(),
                    cloud_id: Some("cloud-1".into()),
                    site_url: Some("https://my-org.atlassian.net".into()),
                    site_name: Some("my-org".into()),
                },
            );
        }

        let response = jira_status(
            State(JiraOAuthStateRef(Arc::clone(&state))),
            Query(ProjectIdQuery::default()),
        )
        .await;
        let resp = response.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
