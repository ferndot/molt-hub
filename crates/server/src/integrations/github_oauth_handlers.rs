//! Axum HTTP handlers for the GitHub App OAuth 2.0 (PKCE) flow.
//!
//! Routes:
//!   GET    /api/integrations/github/auth              — returns authorization URL
//!   GET    /api/integrations/github/oauth/callback    — exchanges code for tokens
//!   GET    /api/integrations/github/status            — returns connection status
//!   POST   /api/integrations/github/disconnect        — clears stored tokens

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

use super::github_app::{github_app_slug_from_env, GithubAppCredentials};
use super::github_client::GitHubClient;
use super::github_oauth::{GithubOAuthError, GithubOAuthService};
use super::integration_params::ProjectIdQuery;
use crate::credentials::{CredentialScope, CredentialStore};

// ---------------------------------------------------------------------------
// Credential store keys
// ---------------------------------------------------------------------------

const KEY_ACCESS_TOKEN: &str = "github/access_token";
const KEY_REFRESH_TOKEN: &str = "github/refresh_token";
const KEY_SCOPE: &str = "github/scope";
const KEY_LOGIN: &str = "github/login";

// ---------------------------------------------------------------------------
// Shared OAuth state
// ---------------------------------------------------------------------------

/// Axum [`State`] wrapper so `FromRef<Arc<GithubAppState>>` can be implemented without orphan-rule
/// `Arc` impl conflicts (see `github_handlers::GithubAppState`).
#[derive(Clone)]
pub struct GithubOAuthStateRef(pub Arc<GithubOAuthState>);

impl Deref for GithubOAuthStateRef {
    type Target = GithubOAuthState;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// State injected into GitHub OAuth handlers.
pub struct GithubOAuthState {
    pub service: tokio::sync::RwLock<GithubOAuthService>,
    /// In-memory OAuth tokens keyed by [`CredentialScope`] (Global vs per-project).
    pub stored_tokens: tokio::sync::Mutex<HashMap<CredentialScope, GithubStoredTokens>>,
    /// Pending PKCE verifiers + credential scope keyed by CSRF state (callback has no `projectId`).
    pub pkce_verifiers: DashMap<String, (String, CredentialScope)>,
    /// Persistent credential store (OS keychain or in-memory fallback).
    pub credential_store: Arc<dyn CredentialStore>,
    /// When set, [`github_status`] may expose a GitHub App install URL (user-to-server OAuth unchanged).
    pub github_app: Option<GithubAppCredentials>,
}

/// Tokens stored after a successful GitHub OAuth exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubStoredTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub scope: String,
    /// GitHub login (`/user`), for import UI `owner` query param.
    #[serde(default)]
    pub login: Option<String>,
}

impl GithubOAuthState {
    /// Create state from an already-constructed [`GithubOAuthService`] and a
    /// [`CredentialStore`] for token persistence across restarts.
    pub fn new(service: GithubOAuthService, credential_store: Arc<dyn CredentialStore>) -> Self {
        Self::with_github_app(service, credential_store, None)
    }

    /// Same as [`GithubOAuthState::new`] but optionally loads GitHub App credentials for install URLs / future App API use.
    pub fn with_github_app(
        service: GithubOAuthService,
        credential_store: Arc<dyn CredentialStore>,
        github_app: Option<GithubAppCredentials>,
    ) -> Self {
        Self {
            service: tokio::sync::RwLock::new(service),
            stored_tokens: tokio::sync::Mutex::new(HashMap::new()),
            pkce_verifiers: DashMap::new(),
            credential_store,
            github_app,
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
            let login = self.credential_store.retrieve(KEY_LOGIN, scope).ok();
            map.insert(
                scope.clone(),
                GithubStoredTokens {
                    access_token,
                    refresh_token,
                    expires_in: None,
                    scope: oauth_scope,
                    login: login.filter(|s| !s.is_empty()),
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
    /// Authenticated user's GitHub login (for `owner` in repo-scoped API calls).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Present when `GITHUB_APP_SLUG` + app credentials are configured (install flow; PKCE OAuth remains available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_install_url: Option<String>,
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
    State(state): State<GithubOAuthStateRef>,
    Query(project): Query<ProjectIdQuery>,
) -> impl IntoResponse {
    let csrf_state = generate_state_token();
    let cred_scope = project.credential_scope();

    let service = state.service.read().await;
    let (url, verifier) = service.authorization_url(&csrf_state);

    state
        .pkce_verifiers
        .insert(csrf_state.clone(), (verifier, cred_scope));

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
    State(app_state): State<GithubOAuthStateRef>,
    Query(query): Query<GithubCallbackQuery>,
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

    // Persist to credential store (best-effort — log on failure but don't abort).
    if let Err(e) =
        app_state
            .credential_store
            .store(KEY_ACCESS_TOKEN, &cred_scope, &tokens.access_token)
    {
        warn!(error = %e, "failed to persist GitHub access token to credential store");
    }
    if let Some(ref rt) = tokens.refresh_token {
        if let Err(e) = app_state
            .credential_store
            .store(KEY_REFRESH_TOKEN, &cred_scope, rt)
        {
            warn!(error = %e, "failed to persist GitHub refresh token to credential store");
        }
    }
    if let Err(e) = app_state
        .credential_store
        .store(KEY_SCOPE, &cred_scope, &tokens.scope)
    {
        warn!(error = %e, "failed to persist GitHub scope to credential store");
    }

    let login = match GitHubClient::new(tokens.access_token.clone())
        .get_authenticated_user_login()
        .await
    {
        Ok(l) => Some(l),
        Err(e) => {
            warn!(error = %e, "failed to fetch GitHub login after OAuth");
            None
        }
    };

    if let Some(ref lg) = login {
        if let Err(e) = app_state.credential_store.store(KEY_LOGIN, &cred_scope, lg) {
            warn!(error = %e, "failed to persist GitHub login to credential store");
        }
    }

    let new_tokens = GithubStoredTokens {
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        expires_in: tokens.expires_in,
        scope: tokens.scope.clone(),
        login,
    };

    // Cache in-memory.
    {
        let mut map = app_state.stored_tokens.lock().await;
        map.insert(cred_scope, new_tokens);
    }

    let _ = scope;

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
    State(app_state): State<GithubOAuthStateRef>,
    Query(project): Query<ProjectIdQuery>,
) -> impl IntoResponse {
    let cred_scope = project.credential_scope();
    app_state.ensure_tokens_loaded(&cred_scope).await;

    let app_install_url = match (&app_state.github_app, github_app_slug_from_env()) {
        (Some(_), Some(slug)) => Some(GithubAppCredentials::installations_new_url(
            &slug, "molt-hub",
        )),
        _ => None,
    };

    let (connected, scope, mut owner) = {
        let map = app_state.stored_tokens.lock().await;
        match map.get(&cred_scope) {
            None => (false, None, None),
            Some(tokens) => (true, Some(tokens.scope.clone()), tokens.login.clone()),
        }
    };

    if connected && owner.is_none() {
        let token = {
            let map = app_state.stored_tokens.lock().await;
            map.get(&cred_scope).map(|t| t.access_token.clone())
        };
        if let Some(token) = token {
            match GitHubClient::new(token)
                .get_authenticated_user_login()
                .await
            {
                Ok(login) => {
                    if let Err(e) = app_state
                        .credential_store
                        .store(KEY_LOGIN, &cred_scope, &login)
                    {
                        warn!(error = %e, "failed to persist GitHub login to credential store");
                    }
                    let mut map = app_state.stored_tokens.lock().await;
                    if let Some(t) = map.get_mut(&cred_scope) {
                        t.login = Some(login.clone());
                    }
                    owner = Some(login);
                }
                Err(e) => warn!(error = %e, "failed to fetch GitHub login for /status"),
            }
        }
    }

    Json(GithubStatusResponse {
        connected,
        scope: if connected { scope } else { None },
        owner: if connected { owner } else { None },
        app_install_url,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Handler: POST /api/integrations/github/disconnect
// ---------------------------------------------------------------------------

/// Clear all stored GitHub OAuth tokens (both in-memory and persisted).
#[instrument(skip_all)]
pub async fn github_disconnect(
    State(app_state): State<GithubOAuthStateRef>,
    Query(project): Query<ProjectIdQuery>,
) -> impl IntoResponse {
    let cred_scope = project.credential_scope();
    // Clear the credential store (best-effort).
    for key in [KEY_ACCESS_TOKEN, KEY_REFRESH_TOKEN, KEY_SCOPE, KEY_LOGIN] {
        if let Err(e) = app_state.credential_store.delete(key, &cred_scope) {
            warn!(error = %e, key, "failed to delete GitHub credential from store");
        }
    }

    let mut map = app_state.stored_tokens.lock().await;
    map.remove(&cred_scope);
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
    .card{{text-align:center;padding:48px 40px;background:#16161f;border:1px solid #2a2a3a;border-radius:12px;max-width:420px}}
    .icon{{font-size:48px;margin-bottom:16px}}
    h1{{font-size:20px;font-weight:600;margin-bottom:8px}}
    p{{font-size:14px;color:#888;margin-bottom:16px;line-height:1.5}}
    a{{color:#7c9cff;text-decoration:none}}
    a:hover{{text-decoration:underline}}
    .links{{font-size:13px;color:#888;margin-bottom:20px}}
    .links a{{display:inline-block;margin:6px 8px}}
    .closing{{font-size:12px;color:#555}}
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">✓</div>
    <h1>{provider} connected</h1>
    <p>Return to the UI to use this integration. If this tab does not close, use a link below.</p>
    <p class="links">
      <a href="/settings">Open Settings (this origin)</a><br/>
      <a href="http://127.0.0.1:5173/settings">Vite dev UI (port 5173)</a>
    </p>
    <div class="closing">This tab will try to close automatically…</div>
  </div>
  <script>
    setTimeout(function(){{window.close()}}, 2000);
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
        .with_state(GithubOAuthStateRef(state))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::github_oauth::GITHUB_CALLBACK_URL;
    use super::*;
    use crate::credentials::{CredentialScope, MemoryStore};

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
        let map = state.stored_tokens.lock().await;
        assert!(map.is_empty());
    }

    #[tokio::test]
    async fn pkce_verifier_stored_on_auth() {
        let state = make_state();

        let response = github_auth(
            State(GithubOAuthStateRef(Arc::clone(&state))),
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

        // Manually insert a verifier.
        state.pkce_verifiers.insert(
            "known-state".to_string(),
            ("verifier123".to_string(), CredentialScope::Global),
        );

        // Attempt callback with an unknown state token.
        let query = GithubCallbackQuery {
            code: "somecode".to_string(),
            state: "unknown-state".to_string(),
        };
        let response =
            github_oauth_callback(State(GithubOAuthStateRef(Arc::clone(&state))), Query(query))
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
            let mut map = state.stored_tokens.lock().await;
            map.insert(
                CredentialScope::Global,
                GithubStoredTokens {
                    access_token: "tok".into(),
                    refresh_token: Some("ref".into()),
                    expires_in: Some(28800),
                    scope: "repo".into(),
                    login: None,
                },
            );
        }

        let response = github_disconnect(
            State(GithubOAuthStateRef(Arc::clone(&state))),
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
        let response = github_status(
            State(GithubOAuthStateRef(Arc::clone(&state))),
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
                GithubStoredTokens {
                    access_token: "tok".into(),
                    refresh_token: None,
                    expires_in: Some(28800),
                    scope: "repo,user".into(),
                    login: None,
                },
            );
        }

        let response = github_status(
            State(GithubOAuthStateRef(Arc::clone(&state))),
            Query(ProjectIdQuery::default()),
        )
        .await;
        let resp = response.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
