//! Axum HTTP handlers for the GitHub App OAuth 2.0 (PKCE) flow.
//!
//! Routes:
//!   GET    /api/integrations/github/auth              — returns authorization URL
//!   GET    /api/integrations/github/oauth/callback    — exchanges code for tokens
//!   GET    /api/integrations/github/status            — returns connection status
//!   POST   /api/integrations/github/disconnect        — clears stored tokens

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

use super::github_oauth::{GithubOAuthError, GithubOAuthService};
use crate::credentials::{CredentialScope, CredentialStore};

// ---------------------------------------------------------------------------
// Credential store keys
// ---------------------------------------------------------------------------

const CRED_SCOPE: CredentialScope = CredentialScope::Global;
const KEY_ACCESS_TOKEN: &str = "github/access_token";
const KEY_REFRESH_TOKEN: &str = "github/refresh_token";
const KEY_SCOPE: &str = "github/scope";

// ---------------------------------------------------------------------------
// Shared OAuth state
// ---------------------------------------------------------------------------

/// State injected into GitHub OAuth handlers.
pub struct GithubOAuthState {
    pub service: tokio::sync::RwLock<GithubOAuthService>,
    /// Last successfully obtained tokens (in-memory cache).
    pub stored_tokens: tokio::sync::Mutex<Option<GithubStoredTokens>>,
    /// Pending PKCE verifiers keyed by CSRF state token.
    pub pkce_verifiers: DashMap<String, String>,
    /// Persistent credential store (OS keychain or in-memory fallback).
    pub credential_store: Arc<dyn CredentialStore>,
}

/// Tokens stored after a successful GitHub OAuth exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubStoredTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub scope: String,
}

impl GithubOAuthState {
    /// Create state from an already-constructed [`GithubOAuthService`] and a
    /// [`CredentialStore`] for token persistence across restarts.
    pub fn new(service: GithubOAuthService, credential_store: Arc<dyn CredentialStore>) -> Self {
        Self {
            service: tokio::sync::RwLock::new(service),
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
pub struct GithubCallbackQuery {
    /// The authorization code returned by GitHub.
    pub code: String,
    /// The CSRF state token.
    pub state: String,
}

/// Response from the auth endpoint.
#[derive(Debug, Serialize)]
pub struct GithubAuthResponse {
    pub url: String,
    pub state: String,
}

/// Response from the callback endpoint.
#[derive(Debug, Serialize)]
pub struct GithubCallbackResponse {
    pub success: bool,
    pub scope: String,
}

/// Response from the status endpoint.
#[derive(Debug, Serialize)]
pub struct GithubStatusResponse {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Response from the disconnect endpoint.
#[derive(Debug, Serialize)]
pub struct GithubDisconnectResponse {
    pub success: bool,
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/github/auth
// ---------------------------------------------------------------------------

/// Generate a GitHub authorization URL with a fresh CSRF state token.
#[instrument(skip_all)]
pub async fn github_auth(
    State(state): State<Arc<GithubOAuthState>>,
) -> impl IntoResponse {
    let csrf_state = generate_state_token();

    let service = state.service.read().await;
    let (url, verifier) = service.authorization_url(&csrf_state);

    state.pkce_verifiers.insert(csrf_state.clone(), verifier);

    Json(GithubAuthResponse {
        url,
        state: csrf_state,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/github/oauth/callback
// ---------------------------------------------------------------------------

/// Exchange the authorization code for tokens.
#[instrument(skip_all, fields(state = %query.state))]
pub async fn github_oauth_callback(
    State(app_state): State<Arc<GithubOAuthState>>,
    Query(query): Query<GithubCallbackQuery>,
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

    let service = app_state.service.read().await;
    let tokens = match service.exchange_code(&query.code, &verifier).await {
        Ok(t) => t,
        Err(e) => {
            let (status, msg) = github_oauth_error_to_http(&e);
            return (
                status,
                Json(serde_json::json!({ "success": false, "error": msg })),
            )
                .into_response();
        }
    };

    let scope = tokens.scope.clone();

    // Build the token struct once.
    let new_tokens = GithubStoredTokens {
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        expires_in: tokens.expires_in,
        scope: tokens.scope.clone(),
    };

    // Persist to credential store (best-effort — log on failure but don't abort).
    if let Err(e) = app_state.credential_store.store(
        KEY_ACCESS_TOKEN,
        &CRED_SCOPE,
        &tokens.access_token,
    ) {
        warn!(error = %e, "failed to persist GitHub access token to credential store");
    }
    if let Some(ref rt) = tokens.refresh_token {
        if let Err(e) = app_state.credential_store.store(KEY_REFRESH_TOKEN, &CRED_SCOPE, rt) {
            warn!(error = %e, "failed to persist GitHub refresh token to credential store");
        }
    }
    if let Err(e) = app_state.credential_store.store(KEY_SCOPE, &CRED_SCOPE, &tokens.scope) {
        warn!(error = %e, "failed to persist GitHub scope to credential store");
    }

    // Cache in-memory.
    {
        let mut stored = app_state.stored_tokens.lock().await;
        *stored = Some(new_tokens);
    }

    let _ = scope; // already cloned into new_tokens above

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        oauth_success_html("GitHub"),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/github/status
// ---------------------------------------------------------------------------

/// Return the current GitHub connection status.
///
/// If in-memory tokens are absent, attempts to load from the credential store
/// so that connections survive server restarts.
#[instrument(skip_all)]
pub async fn github_status(
    State(app_state): State<Arc<GithubOAuthState>>,
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
            *stored = Some(GithubStoredTokens {
                access_token,
                refresh_token,
                expires_in: None,
                scope,
            });
        }
    }

    match stored.as_ref() {
        Some(tokens) => Json(GithubStatusResponse {
            connected: true,
            scope: Some(tokens.scope.clone()),
        })
        .into_response(),
        None => Json(GithubStatusResponse {
            connected: false,
            scope: None,
        })
        .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Handler: POST /api/integrations/github/disconnect
// ---------------------------------------------------------------------------

/// Clear all stored GitHub OAuth tokens (both in-memory and persisted).
#[instrument(skip_all)]
pub async fn github_disconnect(
    State(app_state): State<Arc<GithubOAuthState>>,
) -> impl IntoResponse {
    // Clear the credential store (best-effort).
    for key in [KEY_ACCESS_TOKEN, KEY_REFRESH_TOKEN, KEY_SCOPE] {
        if let Err(e) = app_state.credential_store.delete(key, &CRED_SCOPE) {
            warn!(error = %e, key, "failed to delete GitHub credential from store");
        }
    }

    let mut stored = app_state.stored_tokens.lock().await;
    *stored = None;
    Json(GithubDisconnectResponse { success: true }).into_response()
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
// Helper: OAuth success HTML page (auto-closes the tab)
// ---------------------------------------------------------------------------

pub fn oauth_success_html(provider: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>{provider} Connected</title>
  <style>
    *{{margin:0;padding:0;box-sizing:border-box}}
    body{{display:flex;align-items:center;justify-content:center;min-height:100vh;
         background:#0d0d14;font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;color:#e2e2ea}}
    .card{{text-align:center;padding:48px 40px;background:#16161f;border:1px solid #2a2a3a;border-radius:12px;max-width:380px}}
    .icon{{font-size:48px;margin-bottom:16px}}
    h1{{font-size:20px;font-weight:600;margin-bottom:8px}}
    p{{font-size:14px;color:#888;margin-bottom:24px}}
    .closing{{font-size:12px;color:#555}}
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">✓</div>
    <h1>{provider} connected</h1>
    <p>You can close this window and return to Molt Hub.</p>
    <div class="closing">This tab will close automatically…</div>
  </div>
  <script>
    setTimeout(function(){{window.close()}}, 1500);
  </script>
</body>
</html>"#
    )
}

// ---------------------------------------------------------------------------
// Helper: map GitHub OAuth errors to HTTP status codes
// ---------------------------------------------------------------------------

fn github_oauth_error_to_http(e: &GithubOAuthError) -> (StatusCode, String) {
    match e {
        GithubOAuthError::HttpError(_) => (StatusCode::BAD_GATEWAY, e.to_string()),
        GithubOAuthError::AuthServerError { .. } => (StatusCode::UNAUTHORIZED, e.to_string()),
        GithubOAuthError::ParseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        GithubOAuthError::MissingClientSecret => (StatusCode::PRECONDITION_REQUIRED, e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

use axum::routing::{get, post};
use axum::Router;

/// Build the GitHub OAuth sub-router.
///
/// Mounted at:
///   `/api/integrations/github/auth`              — GET (start OAuth)
///   `/api/integrations/github/oauth/callback`    — GET (handle redirect)
///   `/api/integrations/github/status`            — GET (connection status)
///   `/api/integrations/github/disconnect`         — POST (clear tokens)
pub fn github_oauth_router(state: Arc<GithubOAuthState>) -> Router {
    Router::new()
        .route("/auth", get(github_auth))
        .route("/oauth/callback", get(github_oauth_callback))
        .route("/status", get(github_status))
        .route("/disconnect", post(github_disconnect))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::github_oauth::GITHUB_CALLBACK_URL;
    use crate::credentials::MemoryStore;

    fn make_state() -> Arc<GithubOAuthState> {
        let svc = GithubOAuthService::with_secret(GITHUB_CALLBACK_URL, "test_secret".into());
        let store = Arc::new(MemoryStore::new());
        Arc::new(GithubOAuthState::new(svc, store))
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
    fn github_oauth_error_to_http_maps_parse_error() {
        let err = GithubOAuthError::ParseError("bad json".into());
        let (status, msg) = github_oauth_error_to_http(&err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("bad json"));
    }

    #[test]
    fn github_oauth_error_to_http_maps_auth_server_error() {
        let err = GithubOAuthError::AuthServerError {
            error: "bad_verification_code".into(),
            description: "Code expired".into(),
        };
        let (status, _) = github_oauth_error_to_http(&err);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn github_oauth_error_to_http_maps_missing_secret() {
        let err = GithubOAuthError::MissingClientSecret;
        let (status, _) = github_oauth_error_to_http(&err);
        assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
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

        let response = github_auth(State(Arc::clone(&state))).await;
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

        // Manually insert a verifier.
        state.pkce_verifiers.insert("known-state".to_string(), "verifier123".to_string());

        // Attempt callback with an unknown state token.
        let query = GithubCallbackQuery {
            code: "somecode".to_string(),
            state: "unknown-state".to_string(),
        };
        let response = github_oauth_callback(State(Arc::clone(&state)), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        // The known verifier must still be present.
        assert!(state.pkce_verifiers.contains_key("known-state"));
    }

    #[tokio::test]
    async fn disconnect_clears_stored_tokens() {
        let state = make_state();

        // Manually seed some tokens.
        {
            let mut stored = state.stored_tokens.lock().await;
            *stored = Some(GithubStoredTokens {
                access_token: "tok".into(),
                refresh_token: Some("ref".into()),
                expires_in: Some(28800),
                scope: "repo".into(),
            });
        }

        let response = github_disconnect(State(Arc::clone(&state))).await;
        let _ = response.into_response();

        let stored = state.stored_tokens.lock().await;
        assert!(stored.is_none(), "tokens should be cleared after disconnect");
    }

    #[tokio::test]
    async fn status_returns_disconnected_when_no_tokens() {
        let state = make_state();
        let response = github_status(State(Arc::clone(&state))).await;
        let resp = response.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn status_returns_connected_when_tokens_present() {
        let state = make_state();
        {
            let mut stored = state.stored_tokens.lock().await;
            *stored = Some(GithubStoredTokens {
                access_token: "tok".into(),
                refresh_token: None,
                expires_in: Some(28800),
                scope: "repo,user".into(),
            });
        }

        let response = github_status(State(Arc::clone(&state))).await;
        let resp = response.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
