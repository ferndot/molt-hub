//! GitHub App OAuth 2.0 service with PKCE (RFC 7636).
//!
//! Implements the GitHub App OAuth flow using PKCE for secure authorization
//! without embedding a client secret in the distributed binary.
//!
//! Flow:
//!   1. Generate a random `code_verifier` (128 bytes, base64url-encoded).
//!   2. Compute `code_challenge = BASE64URL(SHA-256(code_verifier))`.
//!   3. Send `code_challenge` + `code_challenge_method=S256` in the auth URL.
//!   4. Exchange the authorization code with `code_verifier` + `client_secret`
//!      (GitHub requires the secret even with PKCE for web apps).
//!
//! Routes:
//!   Authorization URL generation (redirect user to GitHub)
//!   Code exchange (callback handler)
//!   Token refresh

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Baked-in client ID (safe to distribute — unique to this GitHub App)
// ---------------------------------------------------------------------------

/// GitHub App OAuth client ID, baked in at build time.
///
/// This is not a secret: PKCE + client_secret are used for the token exchange.
pub const GITHUB_CLIENT_ID: &str = "Iv23lip4ZuqkEmT9Z2U0";

/// Client secret embedded at compile time via `GITHUB_CLIENT_SECRET` env var.
/// Set it before building: `GITHUB_CLIENT_SECRET=<secret> cargo build`
pub const GITHUB_CLIENT_SECRET: Option<&str> = option_env!("GITHUB_CLIENT_SECRET");

/// Default callback URL for the GitHub OAuth flow.
pub const GITHUB_CALLBACK_URL: &str = "http://localhost:13401/api/integrations/github/oauth/callback";

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`GithubOAuthService`].
#[derive(Debug, Error)]
pub enum GithubOAuthError {
    /// HTTP transport error.
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// The authorization server returned an error response.
    #[error("OAuth error ({error}): {description}")]
    AuthServerError {
        error: String,
        description: String,
    },

    /// Response could not be parsed.
    #[error("parse error: {0}")]
    ParseError(String),

    /// Client secret not configured.
    #[error("client secret not configured — set it via settings")]
    MissingClientSecret,
}

// ---------------------------------------------------------------------------
// Token types
// ---------------------------------------------------------------------------

/// Tokens returned by the GitHub token endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTokenResponse {
    /// OAuth access token.
    pub access_token: String,
    /// Token type (usually `"bearer"`).
    pub token_type: String,
    /// Space-separated list of granted scopes.
    pub scope: String,
    /// Refresh token (GitHub App user-to-server tokens support refresh).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Seconds until `access_token` expires (GitHub App tokens: 8 hours).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    /// Seconds until `refresh_token` expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token_expires_in: Option<u64>,
}

// ---------------------------------------------------------------------------
// Internal error shape returned by GitHub token endpoint
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GithubTokenErrorResponse {
    error: String,
    #[serde(default)]
    error_description: String,
}

// ---------------------------------------------------------------------------
// GithubOAuthService
// ---------------------------------------------------------------------------

/// GitHub App OAuth 2.0 (PKCE) service.
///
/// Handles authorization URL construction, code exchange (with PKCE verifier),
/// and token refresh.
///
/// The client ID is baked in via [`GITHUB_CLIENT_ID`]. The client secret
/// is provided at runtime (never baked in).
pub struct GithubOAuthService {
    client_id: String,
    redirect_uri: String,
    http: Client,
    /// Client secret — provided at runtime by the user.
    client_secret: Option<String>,
}

impl GithubOAuthService {
    /// Create a new service with the baked-in client ID and the given redirect URI.
    pub fn new(redirect_uri: &str) -> Self {
        Self {
            client_id: GITHUB_CLIENT_ID.to_owned(),
            redirect_uri: redirect_uri.to_owned(),
            http: Client::new(),
            client_secret: None,
        }
    }

    /// Create a new service with a client secret.
    pub fn with_secret(redirect_uri: &str, client_secret: String) -> Self {
        Self {
            client_id: GITHUB_CLIENT_ID.to_owned(),
            redirect_uri: redirect_uri.to_owned(),
            http: Client::new(),
            client_secret: Some(client_secret),
        }
    }

    /// Set the client secret at runtime.
    pub fn set_client_secret(&mut self, secret: String) {
        self.client_secret = Some(secret);
    }

    /// Build the GitHub authorization URL to redirect the user to.
    ///
    /// Returns `(url, code_verifier)`. The caller **must** persist
    /// `code_verifier` keyed by `state` so it can be retrieved in the callback.
    ///
    /// `state` is a CSRF token that must be verified in the callback.
    pub fn authorization_url(&self, state: &str) -> (String, String) {
        let verifier = generate_github_pkce_verifier();
        let challenge = github_pkce_challenge(&verifier);

        let encode = |s: &str| {
            s.chars()
                .flat_map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                        vec![c]
                    } else {
                        c.to_string()
                            .bytes()
                            .flat_map(|b| format!("%{b:02X}").chars().collect::<Vec<_>>())
                            .collect()
                    }
                })
                .collect::<String>()
        };

        let url = format!(
            "https://github.com/login/oauth/authorize\
             ?client_id={}\
             &redirect_uri={}\
             &state={}\
             &response_type=code\
             &code_challenge={}\
             &code_challenge_method=S256",
            encode(&self.client_id),
            encode(&self.redirect_uri),
            encode(state),
            encode(&challenge),
        );

        (url, verifier)
    }

    /// Exchange an authorization code for tokens using the PKCE verifier.
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<GithubTokenResponse, GithubOAuthError> {
        let secret = self
            .client_secret
            .as_deref()
            .ok_or(GithubOAuthError::MissingClientSecret)?;

        let params = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", secret),
            ("code", code),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("code_verifier", code_verifier),
        ];

        self.post_token_request(&params).await
    }

    /// Refresh an expired access token.
    pub async fn refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<GithubTokenResponse, GithubOAuthError> {
        let secret = self
            .client_secret
            .as_deref()
            .ok_or(GithubOAuthError::MissingClientSecret)?;

        let params = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", secret),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];

        self.post_token_request(&params).await
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// POST to the GitHub token endpoint with the given form params.
    async fn post_token_request(
        &self,
        params: &[(&str, &str)],
    ) -> Result<GithubTokenResponse, GithubOAuthError> {
        let response = self
            .http
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(params)
            .send()
            .await?;

        if !response.status().is_success() {
            let body: GithubTokenErrorResponse = response
                .json()
                .await
                .unwrap_or(GithubTokenErrorResponse {
                    error: "unknown_error".into(),
                    error_description: String::new(),
                });
            return Err(GithubOAuthError::AuthServerError {
                error: body.error,
                description: body.error_description,
            });
        }

        // GitHub returns 200 even for errors — check the body for error fields.
        let body_text = response.text().await?;
        if let Ok(err_resp) = serde_json::from_str::<GithubTokenErrorResponse>(&body_text) {
            if !err_resp.error.is_empty() {
                return Err(GithubOAuthError::AuthServerError {
                    error: err_resp.error,
                    description: err_resp.error_description,
                });
            }
        }

        serde_json::from_str::<GithubTokenResponse>(&body_text)
            .map_err(|e| GithubOAuthError::ParseError(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// PKCE helpers
// ---------------------------------------------------------------------------

/// Generate a cryptographically random PKCE `code_verifier`.
///
/// Per RFC 7636 section 4.1, the verifier must be 43-128 URL-safe characters.
/// We use 96 random bytes -> 128 base64url characters (no padding).
pub fn generate_github_pkce_verifier() -> String {
    let mut bytes = [0u8; 96];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Compute the PKCE `code_challenge` from a verifier.
///
/// `code_challenge = BASE64URL(SHA-256(ASCII(code_verifier)))` (RFC 7636 section 4.2).
pub fn github_pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> GithubOAuthService {
        GithubOAuthService::new(GITHUB_CALLBACK_URL)
    }

    fn service_with_secret() -> GithubOAuthService {
        GithubOAuthService::with_secret(GITHUB_CALLBACK_URL, "test_secret".into())
    }

    // -----------------------------------------------------------------------
    // PKCE primitive tests
    // -----------------------------------------------------------------------

    #[test]
    fn pkce_verifier_length_is_valid() {
        let verifier = generate_github_pkce_verifier();
        assert!(
            verifier.len() >= 43 && verifier.len() <= 128,
            "verifier length {} is out of range 43-128",
            verifier.len()
        );
    }

    #[test]
    fn pkce_verifier_is_base64url_safe() {
        let verifier = generate_github_pkce_verifier();
        assert!(
            verifier.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "verifier contains non-base64url characters: {verifier}"
        );
    }

    #[test]
    fn pkce_verifier_differs_across_calls() {
        let v1 = generate_github_pkce_verifier();
        let v2 = generate_github_pkce_verifier();
        assert_ne!(v1, v2, "two different verifiers should not collide");
    }

    #[test]
    fn pkce_challenge_is_sha256_base64url() {
        // Known-good: SHA-256("abc") base64url-encoded (no padding).
        let challenge = github_pkce_challenge("abc");
        assert_eq!(challenge, "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0");
    }

    #[test]
    fn pkce_challenge_round_trip_format() {
        let verifier = generate_github_pkce_verifier();
        let challenge = github_pkce_challenge(&verifier);
        // SHA-256 produces 32 bytes -> base64url(32 bytes) = 43 chars (no padding).
        assert_eq!(challenge.len(), 43, "challenge must be 43 base64url chars");
        assert!(
            challenge.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "challenge contains non-base64url characters: {challenge}"
        );
    }

    // -----------------------------------------------------------------------
    // Authorization URL tests
    // -----------------------------------------------------------------------

    #[test]
    fn authorization_url_contains_required_fields() {
        let svc = service();
        let (url, _verifier) = svc.authorization_url("csrf-state-token");

        assert!(url.starts_with("https://github.com/login/oauth/authorize"));
        assert!(url.contains(&format!("client_id={GITHUB_CLIENT_ID}")));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("state=csrf-state-token"));
    }

    #[test]
    fn authorization_url_contains_pkce_params() {
        let svc = service();
        let (url, _verifier) = svc.authorization_url("state");
        assert!(url.contains("code_challenge="), "must include code_challenge");
        assert!(url.contains("code_challenge_method=S256"), "must use S256 method");
    }

    #[test]
    fn authorization_url_challenge_matches_verifier() {
        let svc = service();
        let (url, verifier) = svc.authorization_url("state");
        let expected_challenge = github_pkce_challenge(&verifier);
        assert!(
            url.contains(&format!("code_challenge={expected_challenge}")),
            "challenge in URL does not match verifier"
        );
    }

    #[test]
    fn authorization_url_contains_redirect_uri() {
        let svc = service();
        let (url, _) = svc.authorization_url("state");
        assert!(url.contains("redirect_uri="));
    }

    #[test]
    fn authorization_url_no_client_secret() {
        let svc = service_with_secret();
        let (url, _) = svc.authorization_url("state");
        assert!(
            !url.contains("client_secret"),
            "client_secret must not appear in auth URL"
        );
        assert!(
            !url.contains("test_secret"),
            "secret value must not appear in auth URL"
        );
    }

    #[test]
    fn authorization_url_state_is_included() {
        let svc = service();
        let (url, _) = svc.authorization_url("my-unique-csrf");
        assert!(url.contains("state=my-unique-csrf"));
    }

    // -----------------------------------------------------------------------
    // Service construction tests
    // -----------------------------------------------------------------------

    #[test]
    fn service_new_has_no_secret() {
        let svc = service();
        assert!(svc.client_secret.is_none());
    }

    #[test]
    fn service_with_secret_stores_secret() {
        let svc = service_with_secret();
        assert_eq!(svc.client_secret.as_deref(), Some("test_secret"));
    }

    #[test]
    fn service_set_client_secret() {
        let mut svc = service();
        assert!(svc.client_secret.is_none());
        svc.set_client_secret("new_secret".into());
        assert_eq!(svc.client_secret.as_deref(), Some("new_secret"));
    }

    // -----------------------------------------------------------------------
    // Error display tests
    // -----------------------------------------------------------------------

    #[test]
    fn github_oauth_error_display_auth_server() {
        let err = GithubOAuthError::AuthServerError {
            error: "bad_verification_code".into(),
            description: "Code has expired".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("bad_verification_code"));
        assert!(msg.contains("Code has expired"));
    }

    #[test]
    fn github_oauth_error_display_parse_error() {
        let err = GithubOAuthError::ParseError("bad json".into());
        assert!(err.to_string().contains("bad json"));
    }

    #[test]
    fn github_oauth_error_display_missing_secret() {
        let err = GithubOAuthError::MissingClientSecret;
        assert!(err.to_string().contains("client secret"));
    }

    // -----------------------------------------------------------------------
    // Deserialization tests
    // -----------------------------------------------------------------------

    #[test]
    fn github_token_response_deserializes() {
        let json = r#"{
            "access_token": "ghu_abc123",
            "token_type": "bearer",
            "scope": "repo,user",
            "refresh_token": "ghr_def456",
            "expires_in": 28800,
            "refresh_token_expires_in": 15897600
        }"#;
        let resp: GithubTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token, "ghu_abc123");
        assert_eq!(resp.token_type, "bearer");
        assert_eq!(resp.scope, "repo,user");
        assert_eq!(resp.refresh_token, Some("ghr_def456".into()));
        assert_eq!(resp.expires_in, Some(28800));
    }

    #[test]
    fn github_token_response_no_refresh_token() {
        let json = r#"{
            "access_token": "ghu_abc123",
            "token_type": "bearer",
            "scope": "repo"
        }"#;
        let resp: GithubTokenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.refresh_token.is_none());
        assert!(resp.expires_in.is_none());
    }

    // -----------------------------------------------------------------------
    // Token exchange format tests (verifies params, not network)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn exchange_code_requires_client_secret() {
        let svc = service(); // no secret
        let result = svc.exchange_code("some_code", "some_verifier").await;
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), GithubOAuthError::MissingClientSecret),
            "should return MissingClientSecret"
        );
    }

    #[tokio::test]
    async fn refresh_token_requires_client_secret() {
        let svc = service(); // no secret
        let result = svc.refresh_token("some_refresh_token").await;
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), GithubOAuthError::MissingClientSecret),
            "should return MissingClientSecret"
        );
    }
}
