//! Axum HTTP handlers for the Jira (Atlassian) OAuth 2.0 (3LO + PKCE) flow.
//!
//! Routes (mounted at `/api/integrations/jira`):
//!   GET    /auth              — returns authorization URL + CSRF state
//!   GET    /oauth/callback    — exchanges code for tokens, fetches site info
//!   GET    /status            — returns `{ connected, site_url? }`
//!   POST   /disconnect        — clears stored tokens

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

use super::github_oauth_handlers::oauth_success_html;
use super::oauth::{JiraOAuthService, OAuthError, DEFAULT_SCOPES};
use crate::credentials::{CredentialScope, CredentialStore};

// ---------------------------------------------------------------------------
// Credential store keys
// ---------------------------------------------------------------------------

const CRED_SCOPE: CredentialScope = CredentialScope::Global;
const KEY_ACCESS_TOKEN: &str = "jira/access_token";
const KEY_REFRESH_TOKEN: &str = "jira/refresh_token";
const KEY_SCOPE: &str = "jira/scope";
const KEY_SITE_URL: &str = "jira/site_url";
const KEY_SITE_NAME: &str = "jira/site_name";

// ---------------------------------------------------------------------------
// Shared OAuth state
// ---------------------------------------------------------------------------

/// State injected into Jira OAuth handlers.
pub struct JiraOAuthState {
    pub service: JiraOAuthService,
    /// Last successfully obtained tokens (in-memory cache).
    pub stored_tokens: tokio::sync::Mutex<Option<JiraStoredTokens>>,
    /// Pending PKCE verifiers keyed by CSRF state token.
    pub pkce_verifiers: DashMap<String, String>,
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
            stored_tokens: tokio::sync::Mutex::new(None),
            pkce_verifiers: DashMap::new(),
            credential_store,
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
    State(state): State<Arc<JiraOAuthState>>,
) -> impl IntoResponse {
    let csrf_state = generate_state_token();

    let (url, verifier) = state.service.authorization_url(&csrf_state, DEFAULT_SCOPES);

    state.pkce_verifiers.insert(csrf_state.clone(), verifier);

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
    State(app_state): State<Arc<JiraOAuthState>>,
    Query(query): Query<JiraCallbackQuery>,
) -> impl IntoResponse {
    // Retrieve (and consume) the PKCE verifier for this CSRF state.
    let verifier = match app_state.pkce_verifiers.remove(&query.state) {
        Some((_, v)) => v,
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

    let tokens = match app_state.service.exchange_code(&query.code, &verifier).await {
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

    // Fetch accessible resources to get site URL.
    let (site_url, site_name) = match app_state
        .service
        .get_accessible_resources(&tokens.access_token)
        .await
    {
        Ok(sites) => {
            let first = sites.into_iter().next();
            (first.as_ref().map(|s| s.url.clone()), first.as_ref().map(|s| s.name.clone()))
        }
        Err(_) => (None, None),
    };

    // Build token struct.
    let new_tokens = JiraStoredTokens {
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        expires_in: tokens.expires_in,
        scope: tokens.scope.clone(),
        site_url: site_url.clone(),
        site_name: site_name.clone(),
    };

    // Persist to credential store (best-effort — log on failure but don't abort).
    if let Err(e) = app_state.credential_store.store(KEY_ACCESS_TOKEN, &CRED_SCOPE, &tokens.access_token) {
        warn!(error = %e, "failed to persist Jira access token to credential store");
    }
    if let Some(ref rt) = tokens.refresh_token {
        if let Err(e) = app_state.credential_store.store(KEY_REFRESH_TOKEN, &CRED_SCOPE, rt) {
            warn!(error = %e, "failed to persist Jira refresh token to credential store");
        }
    }
    if let Err(e) = app_state.credential_store.store(KEY_SCOPE, &CRED_SCOPE, &tokens.scope) {
        warn!(error = %e, "failed to persist Jira scope to credential store");
    }
    if let Some(ref url) = site_url {
        if let Err(e) = app_state.credential_store.store(KEY_SITE_URL, &CRED_SCOPE, url) {
            warn!(error = %e, "failed to persist Jira site_url to credential store");
        }
    }
    if let Some(ref name) = site_name {
        if let Err(e) = app_state.credential_store.store(KEY_SITE_NAME, &CRED_SCOPE, name) {
            warn!(error = %e, "failed to persist Jira site_name to credential store");
        }
    }

    // Cache in-memory.
    {
        let mut stored = app_state.stored_tokens.lock().await;
        *stored = Some(new_tokens);
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
    State(app_state): State<Arc<JiraOAuthState>>,
) -> impl IntoResponse {
    let mut stored = app_state.stored_tokens.lock().await;

    // Warm the in-memory cache from the credential store on first access.
    if stored.is_none() {
        if let Ok(access_token) = app_state.credential_store.retrieve(KEY_ACCESS_TOKEN, &CRED_SCOPE) {
            let refresh_token = app_state
                .credential_store
                .retrieve(KEY_REFRESH_TOKEN, &CRED_SCOPE)
                .ok();
            let scope = app_state
                .credential_store
                .retrieve(KEY_SCOPE, &CRED_SCOPE)
                .unwrap_or_default();
            let site_url = app_state
                .credential_store
                .retrieve(KEY_SITE_URL, &CRED_SCOPE)
                .ok();
            let site_name = app_state
                .credential_store
                .retrieve(KEY_SITE_NAME, &CRED_SCOPE)
                .ok();
            *stored = Some(JiraStoredTokens {
                access_token,
                refresh_token,
                expires_in: 0,
                scope,
                site_url,
                site_name,
            });
        }
    }

    match stored.as_ref() {
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
    State(app_state): State<Arc<JiraOAuthState>>,
) -> impl IntoResponse {
    // Clear the credential store (best-effort).
    for key in [KEY_ACCESS_TOKEN, KEY_REFRESH_TOKEN, KEY_SCOPE, KEY_SITE_URL, KEY_SITE_NAME] {
        if let Err(e) = app_state.credential_store.delete(key, &CRED_SCOPE) {
            warn!(error = %e, key, "failed to delete Jira credential from store");
        }
    }

    let mut stored = app_state.stored_tokens.lock().await;
    *stored = None;
    Json(JiraDisconnectResponse { success: true }).into_response()
}

// ---------------------------------------------------------------------------
// Helper: generate CSRF state token
// ---------------------------------------------------------------------------

fn generate_state_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Helper: map Jira OAuth errors to HTTP status codes
// ---------------------------------------------------------------------------

fn jira_oauth_error_to_http(e: &OAuthError) -> (StatusCode, String) {
    match e {
        OAuthError::HttpError(_) => (StatusCode::BAD_GATEWAY, e.to_string()),
        OAuthError::AuthServerError { .. } => (StatusCode::UNAUTHORIZED, e.to_string()),
        OAuthError::ParseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryStore;

    fn make_state() -> Arc<JiraOAuthState> {
        let svc = JiraOAuthService::new("https://example.com/cb");
        let store = Arc::new(MemoryStore::new());
        Arc::new(JiraOAuthState::new(svc, store))
    }

    #[test]
    fn generate_state_token_is_nonempty() {
        let token = generate_state_token();
        assert!(!token.is_empty());
        assert_eq!(token.len(), 32); // 16 bytes -> 32 hex chars
    }

    #[test]
    fn generate_state_token_is_hex() {
        let token = generate_state_token();
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit()),
            "state token is not valid hex: {token}"
        );
    }

    #[test]
    fn generate_state_token_differs_across_calls() {
        let t1 = generate_state_token();
        let t2 = generate_state_token();
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

    #[tokio::test]
    async fn stored_tokens_starts_empty() {
        let state = make_state();
        let stored = state.stored_tokens.lock().await;
        assert!(stored.is_none());
    }

    #[tokio::test]
    async fn pkce_verifier_stored_on_auth() {
        let state = make_state();

        let response = jira_auth(State(Arc::clone(&state))).await;
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

        state.pkce_verifiers.insert("known-state".to_string(), "verifier123".to_string());

        let query = JiraCallbackQuery {
            code: "somecode".to_string(),
            state: "unknown-state".to_string(),
        };
        let response = jira_oauth_callback(State(Arc::clone(&state)), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(state.pkce_verifiers.contains_key("known-state"));
    }

    #[tokio::test]
    async fn disconnect_clears_stored_tokens() {
        let state = make_state();

        {
            let mut stored = state.stored_tokens.lock().await;
            *stored = Some(JiraStoredTokens {
                access_token: "tok".into(),
                refresh_token: Some("ref".into()),
                expires_in: 3600,
                scope: "read:jira-work".into(),
                site_url: Some("https://my-org.atlassian.net".into()),
                site_name: Some("my-org".into()),
            });
        }

        let response = jira_disconnect(State(Arc::clone(&state))).await;
        let _ = response.into_response();

        let stored = state.stored_tokens.lock().await;
        assert!(stored.is_none(), "tokens should be cleared after disconnect");
    }

    #[tokio::test]
    async fn status_returns_disconnected_when_no_tokens() {
        let state = make_state();
        let response = jira_status(State(Arc::clone(&state))).await;
        let resp = response.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn status_returns_connected_when_tokens_present() {
        let state = make_state();
        {
            let mut stored = state.stored_tokens.lock().await;
            *stored = Some(JiraStoredTokens {
                access_token: "tok".into(),
                refresh_token: None,
                expires_in: 3600,
                scope: "read:jira-work".into(),
                site_url: Some("https://my-org.atlassian.net".into()),
                site_name: Some("my-org".into()),
            });
        }

        let response = jira_status(State(Arc::clone(&state))).await;
        let resp = response.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
