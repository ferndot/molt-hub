//! Axum HTTP handlers for the Atlassian OAuth 2.0 (3LO + PKCE) flow.
//!
//! Routes:
//!   GET    /api/integrations/jira/oauth/authorize   — returns redirect URL + CSRF state
//!   GET    /api/integrations/jira/oauth/callback    — exchanges code for tokens
//!   POST   /api/integrations/jira/oauth/refresh     — refreshes expired tokens
//!   DELETE /api/integrations/jira/oauth/disconnect  — clears stored tokens

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use super::oauth::{CloudResource, JiraOAuthService, OAuthError, DEFAULT_SCOPES};

// ---------------------------------------------------------------------------
// Shared OAuth state
// ---------------------------------------------------------------------------

/// State injected into OAuth handlers.
///
/// Stores the OAuth service configuration and any runtime token state.
/// `pkce_verifiers` maps a CSRF `state` token to its PKCE `code_verifier`
/// so the callback handler can retrieve the verifier and send it in the
/// token exchange request.
pub struct OAuthState {
    pub service: JiraOAuthService,
    /// Last successfully obtained tokens (in-memory; not persisted across restarts).
    pub stored_tokens: tokio::sync::Mutex<Option<StoredTokens>>,
    /// Pending PKCE verifiers keyed by CSRF state token.
    ///
    /// Entries are inserted in `oauth_authorize` and removed (consumed) in
    /// `oauth_callback`.  A `DashMap` provides lock-free concurrent access.
    pub pkce_verifiers: DashMap<String, String>,
}

/// Tokens stored after a successful OAuth exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Seconds until the access token expires (from time of issue).
    pub expires_in: u64,
    pub scope: String,
    /// The cloud sites available to this token.
    pub sites: Vec<CloudResource>,
}

impl OAuthState {
    /// Create state from an already-constructed [`JiraOAuthService`].
    pub fn new(service: JiraOAuthService) -> Self {
        Self {
            service,
            stored_tokens: tokio::sync::Mutex::new(None),
            pkce_verifiers: DashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Query parameters for the OAuth callback.
#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    /// The authorization code returned by Atlassian.
    pub code: String,
    /// The CSRF state token — must match what was issued in `/authorize`.
    pub state: String,
}

/// Response from the authorize endpoint.
#[derive(Debug, Serialize)]
pub struct AuthorizeResponse {
    /// The URL the client should redirect the user to.
    pub url: String,
    /// The CSRF state token embedded in `url` (also returned so the client
    /// can verify it in the callback).
    pub state: String,
}

/// Response from the callback endpoint.
#[derive(Debug, Serialize)]
pub struct CallbackResponse {
    pub success: bool,
    /// Accessible Atlassian sites.
    pub sites: Vec<CloudResource>,
}

/// Response from the refresh endpoint.
#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub success: bool,
}

/// Response from the disconnect endpoint.
#[derive(Debug, Serialize)]
pub struct DisconnectResponse {
    pub success: bool,
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/jira/oauth/authorize
// ---------------------------------------------------------------------------

/// Generate an Atlassian authorization URL with a fresh CSRF state token.
///
/// The PKCE `code_verifier` is stored in `state.pkce_verifiers` keyed by the
/// CSRF state so it can be retrieved in the callback.
///
/// The client should redirect the user's browser to the returned `url`.
#[instrument(skip_all)]
pub async fn oauth_authorize(
    State(state): State<Arc<OAuthState>>,
) -> impl IntoResponse {
    // Generate a cryptographically random CSRF state token.
    let csrf_state = generate_state_token();

    // Build the authorization URL; this also generates the PKCE verifier.
    let (url, verifier) = state.service.authorization_url(&csrf_state, DEFAULT_SCOPES);

    // Store the verifier so the callback can retrieve it.
    state.pkce_verifiers.insert(csrf_state.clone(), verifier);

    Json(AuthorizeResponse {
        url,
        state: csrf_state,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Handler: GET /api/integrations/jira/oauth/callback
// ---------------------------------------------------------------------------

/// Exchange the authorization code for tokens and fetch accessible resources.
///
/// The CSRF `state` parameter is verified by looking up the stored PKCE
/// verifier.  If no verifier exists for the given state, the request is
/// rejected.
#[instrument(skip_all, fields(state = %query.state))]
pub async fn oauth_callback(
    State(app_state): State<Arc<OAuthState>>,
    Query(query): Query<CallbackQuery>,
) -> impl IntoResponse {
    // Retrieve (and consume) the PKCE verifier for this CSRF state.
    // If it doesn't exist, the state is unknown or already used — reject.
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

    // Exchange the authorization code for tokens, sending the PKCE verifier.
    let tokens = match app_state.service.exchange_code(&query.code, &verifier).await {
        Ok(t) => t,
        Err(e) => {
            let (status, msg) = oauth_error_to_http(&e);
            return (
                status,
                Json(serde_json::json!({ "success": false, "error": msg })),
            )
                .into_response();
        }
    };

    // Fetch accessible resources (cloud sites) using the new access token.
    let sites = match app_state
        .service
        .get_accessible_resources(&tokens.access_token)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            let (status, msg) = oauth_error_to_http(&e);
            return (
                status,
                Json(serde_json::json!({ "success": false, "error": msg })),
            )
                .into_response();
        }
    };

    // Store tokens in-memory.
    {
        let mut stored = app_state.stored_tokens.lock().await;
        *stored = Some(StoredTokens {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_in: tokens.expires_in,
            scope: tokens.scope,
            sites: sites.clone(),
        });
    }

    Json(CallbackResponse {
        success: true,
        sites,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Handler: POST /api/integrations/jira/oauth/refresh
// ---------------------------------------------------------------------------

/// Refresh the stored access token using the stored refresh token.
///
/// No PKCE verifier is needed for token refresh.
#[instrument(skip_all)]
pub async fn oauth_refresh(
    State(app_state): State<Arc<OAuthState>>,
) -> impl IntoResponse {
    let refresh_token = {
        let stored = app_state.stored_tokens.lock().await;
        match stored.as_ref().and_then(|t| t.refresh_token.clone()) {
            Some(rt) => rt,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "success": false,
                        "error": "No refresh token stored. Please re-authorize."
                    })),
                )
                    .into_response();
            }
        }
    };

    let new_tokens = match app_state.service.refresh_token(&refresh_token).await {
        Ok(t) => t,
        Err(e) => {
            let (status, msg) = oauth_error_to_http(&e);
            return (
                status,
                Json(serde_json::json!({ "success": false, "error": msg })),
            )
                .into_response();
        }
    };

    // Update stored tokens while preserving other fields.
    {
        let mut stored = app_state.stored_tokens.lock().await;
        if let Some(ref mut s) = *stored {
            s.access_token = new_tokens.access_token;
            if let Some(rt) = new_tokens.refresh_token {
                s.refresh_token = Some(rt);
            }
            s.expires_in = new_tokens.expires_in;
            s.scope = new_tokens.scope;
        }
    }

    Json(RefreshResponse { success: true }).into_response()
}

// ---------------------------------------------------------------------------
// Handler: DELETE /api/integrations/jira/oauth/disconnect
// ---------------------------------------------------------------------------

/// Clear all stored OAuth tokens, effectively disconnecting the integration.
#[instrument(skip_all)]
pub async fn oauth_disconnect(
    State(app_state): State<Arc<OAuthState>>,
) -> impl IntoResponse {
    let mut stored = app_state.stored_tokens.lock().await;
    *stored = None;
    Json(DisconnectResponse { success: true }).into_response()
}

// ---------------------------------------------------------------------------
// Helper: generate CSRF state token (cryptographic)
// ---------------------------------------------------------------------------

/// Generate a cryptographically random hex string for use as an OAuth CSRF token.
fn generate_state_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Helper: map OAuth errors to HTTP status codes
// ---------------------------------------------------------------------------

fn oauth_error_to_http(e: &OAuthError) -> (StatusCode, String) {
    match e {
        OAuthError::HttpError(_) => (StatusCode::BAD_GATEWAY, e.to_string()),
        OAuthError::AuthServerError { .. } => (StatusCode::UNAUTHORIZED, e.to_string()),
        OAuthError::ParseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

use axum::routing::{delete, get, post};
use axum::Router;

/// Build the `/api/integrations/jira/oauth` sub-router.
pub fn oauth_router(state: Arc<OAuthState>) -> Router {
    Router::new()
        .route("/authorize", get(oauth_authorize))
        .route("/callback", get(oauth_callback))
        .route("/refresh", post(oauth_refresh))
        .route("/disconnect", delete(oauth_disconnect))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> Arc<OAuthState> {
        let svc = JiraOAuthService::new("https://example.com/cb");
        Arc::new(OAuthState::new(svc))
    }

    #[test]
    fn generate_state_token_is_nonempty() {
        let token = generate_state_token();
        assert!(!token.is_empty());
        assert_eq!(token.len(), 32); // 16 bytes → 32 hex chars
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
        // Cryptographic RNG — should never collide in practice.
        assert_ne!(t1, t2, "two CSRF tokens must not be identical");
    }

    #[test]
    fn oauth_error_to_http_maps_http_error() {
        let err = OAuthError::ParseError("bad json".into());
        let (status, msg) = oauth_error_to_http(&err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("bad json"));
    }

    #[test]
    fn oauth_error_to_http_maps_auth_server_error() {
        let err = OAuthError::AuthServerError {
            error: "invalid_grant".into(),
            description: "Code expired".into(),
        };
        let (status, _) = oauth_error_to_http(&err);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn stored_tokens_starts_empty() {
        let state = make_state();
        let stored = state.stored_tokens.lock().await;
        assert!(stored.is_none());
    }

    #[tokio::test]
    async fn pkce_verifier_stored_on_authorize() {
        let state = make_state();

        let response = oauth_authorize(State(Arc::clone(&state))).await;
        let _ = response.into_response(); // consume

        // The authorize handler should have stored exactly one PKCE verifier.
        assert_eq!(
            state.pkce_verifiers.len(),
            1,
            "expected one stored PKCE verifier after authorize"
        );
    }

    #[tokio::test]
    async fn pkce_verifier_consumed_by_unknown_state() {
        let state = make_state();

        // Manually insert a verifier.
        state.pkce_verifiers.insert("known-state".to_string(), "verifier123".to_string());

        // Attempt callback with an unknown state token.
        let query = CallbackQuery {
            code: "somecode".to_string(),
            state: "unknown-state".to_string(),
        };
        let response = oauth_callback(State(Arc::clone(&state)), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        // The known verifier must still be present (we didn't touch it).
        assert!(state.pkce_verifiers.contains_key("known-state"));
    }

    #[tokio::test]
    async fn disconnect_clears_stored_tokens() {
        let state = make_state();

        // Manually seed some tokens.
        {
            let mut stored = state.stored_tokens.lock().await;
            *stored = Some(StoredTokens {
                access_token: "tok".into(),
                refresh_token: Some("ref".into()),
                expires_in: 3600,
                scope: "read:jira-work".into(),
                sites: vec![],
            });
        }

        let response = oauth_disconnect(State(Arc::clone(&state))).await;
        let _ = response.into_response(); // consume

        let stored = state.stored_tokens.lock().await;
        assert!(stored.is_none(), "tokens should be cleared after disconnect");
    }
}
