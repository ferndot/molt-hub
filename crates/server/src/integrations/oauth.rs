//! Atlassian OAuth 2.0 (3LO) service with PKCE (RFC 7636).
//!
//! The app is distributed as a downloadable binary, so a client secret cannot
//! be embedded safely.  Instead, this module implements the PKCE extension:
//!
//! 1. Generate a random `code_verifier` (128 bytes, base64url-encoded).
//! 2. Compute `code_challenge = BASE64URL(SHA-256(code_verifier))`.
//! 3. Send `code_challenge` + `code_challenge_method=S256` in the auth URL.
//! 4. Send `code_verifier` in the token exchange POST (no client secret).
//!
//! Routes:
//!   Authorization URL generation (redirect user to Atlassian)
//!   Code exchange (callback handler)
//!   Token refresh
//!   Accessible resources lookup (to obtain cloud IDs)

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Baked-in client ID (safe to distribute — PKCE replaces the secret)
// ---------------------------------------------------------------------------

/// Atlassian OAuth 2.0 client ID, baked in at build time.
///
/// This is not a secret: PKCE eliminates the need for a client secret.
pub const JIRA_CLIENT_ID: &str = "3yQWy34WyjCn0wtOfawofBTMmtK3gUgs";

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`JiraOAuthService`].
#[derive(Debug, Error)]
pub enum OAuthError {
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
}

// ---------------------------------------------------------------------------
// Token / resource types
// ---------------------------------------------------------------------------

/// Tokens returned by the Atlassian token endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    /// Short-lived access token.
    pub access_token: String,
    /// Long-lived refresh token (may be absent if `offline_access` not requested).
    pub refresh_token: Option<String>,
    /// Seconds until `access_token` expires.
    pub expires_in: u64,
    /// Space-separated list of granted scopes.
    pub scope: String,
}

/// An Atlassian cloud site accessible by the authenticated user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudResource {
    /// Cloud ID — used as the `{cloud_id}` segment in API URLs.
    pub id: String,
    /// Human-readable site name (e.g. `"my-org"`).
    pub name: String,
    /// Base URL of the site (e.g. `"https://my-org.atlassian.net"`).
    pub url: String,
}

// ---------------------------------------------------------------------------
// Internal error shape returned by token endpoint
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TokenErrorResponse {
    error: String,
    #[serde(default)]
    error_description: String,
}

// ---------------------------------------------------------------------------
// JiraOAuthService
// ---------------------------------------------------------------------------

/// Default scopes requested during OAuth authorization.
///
/// Classic scopes cover Jira core (issues, projects, users).
/// Granular Jira Software scopes are required for sprint and board data
/// (classic scopes do not cover Jira Software endpoints).
/// `write:jira-work` is included now so agents can transition issues and post
/// comments without requiring a re-auth flow later.
pub const DEFAULT_SCOPES: &[&str] = &[
    // Jira core — classic scopes
    "read:jira-work",
    "read:jira-user",
    "write:jira-work",
    // Jira Software — granular scopes (required; no classic equivalent)
    "read:sprint:jira-software",
    "read:board-scope:jira-software",
    // Refresh tokens
    "offline_access",
];

/// Atlassian OAuth 2.0 (3LO + PKCE) service.
///
/// Handles authorization URL construction, code exchange (with PKCE verifier),
/// token refresh, and accessible-resources discovery.
///
/// The client ID is baked in via [`JIRA_CLIENT_ID`].  No client secret is
/// required — the PKCE `code_verifier` serves that role.
pub struct JiraOAuthService {
    client_id: String,
    redirect_uri: String,
    http: Client,
}

impl JiraOAuthService {
    /// Create a new service with the baked-in client ID and the given redirect URI.
    pub fn new(redirect_uri: &str) -> Self {
        Self {
            client_id: JIRA_CLIENT_ID.to_owned(),
            redirect_uri: redirect_uri.to_owned(),
            http: Client::new(),
        }
    }

    /// Build the Atlassian authorization URL to redirect the user to.
    ///
    /// Returns `(url, code_verifier)`.  The caller **must** persist
    /// `code_verifier` keyed by `state` so it can be retrieved in the callback.
    ///
    /// `state` is a CSRF token that must be verified in the callback.
    /// `scopes` defaults to [`DEFAULT_SCOPES`] if empty.
    pub fn authorization_url(
        &self,
        state: &str,
        scopes: &[&str],
    ) -> (String, String) {
        let scope_list = if scopes.is_empty() {
            DEFAULT_SCOPES.join(" ")
        } else {
            scopes.join(" ")
        };

        let verifier = generate_pkce_verifier();
        let challenge = pkce_challenge(&verifier);

        // Percent-encode each query parameter value (RFC 3986 unreserved chars
        // are left as-is; everything else is %-encoded).
        let encode = |s: &str| {
            s.chars()
                .flat_map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                        vec![c]
                    } else {
                        // Encode each byte of the UTF-8 representation.
                        c.to_string()
                            .bytes()
                            .flat_map(|b| {
                                format!("%{b:02X}").chars().collect::<Vec<_>>()
                            })
                            .collect()
                    }
                })
                .collect::<String>()
        };

        let url = format!(
            "https://auth.atlassian.com/authorize\
             ?audience=api.atlassian.com\
             &client_id={}\
             &scope={}\
             &redirect_uri={}\
             &state={}\
             &response_type=code\
             &prompt=consent\
             &code_challenge={}\
             &code_challenge_method=S256",
            encode(&self.client_id),
            encode(&scope_list),
            encode(&self.redirect_uri),
            encode(state),
            encode(&challenge),
        );

        (url, verifier)
    }

    /// Exchange an authorization code for tokens using the PKCE verifier.
    ///
    /// `code_verifier` is the value generated alongside the authorization URL.
    /// It is sent in place of a client secret.
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<TokenResponse, OAuthError> {
        let params = [
            ("grant_type", "authorization_code"),
            ("client_id", &self.client_id),
            ("code", code),
            ("redirect_uri", &self.redirect_uri),
            ("code_verifier", code_verifier),
        ];

        self.post_token_request(&params).await
    }

    /// Refresh an expired access token.
    ///
    /// Requires that `offline_access` scope was requested during authorization.
    /// PKCE verifier is not needed for refresh — and no client secret either.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenResponse, OAuthError> {
        let params = [
            ("grant_type", "refresh_token"),
            ("client_id", &self.client_id),
            ("refresh_token", refresh_token),
        ];

        self.post_token_request(&params).await
    }

    /// Retrieve the list of Atlassian cloud sites accessible with `access_token`.
    ///
    /// Returns a list of [`CloudResource`]s, each containing a `cloud_id` that
    /// must be used when constructing Jira API URLs.
    pub async fn get_accessible_resources(
        &self,
        access_token: &str,
    ) -> Result<Vec<CloudResource>, OAuthError> {
        let response = self
            .http
            .get("https://api.atlassian.com/oauth/token/accessible-resources")
            .bearer_auth(access_token)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(OAuthError::AuthServerError {
                error: format!("HTTP {status}"),
                description: body,
            });
        }

        // The API returns an array of resource objects; we only need id, name, url.
        #[derive(Deserialize)]
        struct ResourceRaw {
            id: String,
            name: String,
            url: String,
        }

        let resources: Vec<ResourceRaw> = response
            .json()
            .await
            .map_err(|e| OAuthError::ParseError(e.to_string()))?;

        Ok(resources
            .into_iter()
            .map(|r| CloudResource {
                id: r.id,
                name: r.name,
                url: r.url,
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// POST to the Atlassian token endpoint with the given form params.
    async fn post_token_request(
        &self,
        params: &[(&str, &str)],
    ) -> Result<TokenResponse, OAuthError> {
        let response = self
            .http
            .post("https://auth.atlassian.com/oauth/token")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .form(params)
            .send()
            .await?;

        if !response.status().is_success() {
            // Try to parse the error body
            let body: TokenErrorResponse = response
                .json()
                .await
                .unwrap_or(TokenErrorResponse {
                    error: "unknown_error".into(),
                    error_description: String::new(),
                });
            return Err(OAuthError::AuthServerError {
                error: body.error,
                description: body.error_description,
            });
        }

        response
            .json::<TokenResponse>()
            .await
            .map_err(|e| OAuthError::ParseError(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// PKCE helpers (public so oauth_handlers can test them)
// ---------------------------------------------------------------------------

/// Generate a cryptographically random PKCE `code_verifier`.
///
/// Per RFC 7636 §4.1, the verifier must be 43–128 URL-safe characters.
/// We use 96 random bytes → 128 base64url characters (no padding).
pub fn generate_pkce_verifier() -> String {
    let mut bytes = [0u8; 96];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Compute the PKCE `code_challenge` from a verifier.
///
/// `code_challenge = BASE64URL(SHA-256(ASCII(code_verifier)))` (RFC 7636 §4.2).
pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> JiraOAuthService {
        JiraOAuthService::new("https://app.example.com/oauth/callback")
    }

    // -----------------------------------------------------------------------
    // PKCE primitive tests
    // -----------------------------------------------------------------------

    #[test]
    fn pkce_verifier_length_is_valid() {
        let verifier = generate_pkce_verifier();
        // RFC 7636 §4.1: 43–128 characters.
        assert!(
            verifier.len() >= 43 && verifier.len() <= 128,
            "verifier length {} is out of range 43–128",
            verifier.len()
        );
    }

    #[test]
    fn pkce_verifier_is_base64url_safe() {
        let verifier = generate_pkce_verifier();
        // base64url alphabet: A-Z, a-z, 0-9, '-', '_'  (no padding '=')
        assert!(
            verifier.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "verifier contains non-base64url characters: {verifier}"
        );
    }

    #[test]
    fn pkce_verifier_differs_across_calls() {
        let v1 = generate_pkce_verifier();
        let v2 = generate_pkce_verifier();
        assert_ne!(v1, v2, "two different verifiers should not collide");
    }

    #[test]
    fn pkce_challenge_is_sha256_base64url() {
        // Known-good: SHA-256("abc") base64url-encoded (no padding).
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2ec73b00361bbef0469f492c3 (hex).
        // base64url(that) = "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0"
        let challenge = pkce_challenge("abc");
        assert_eq!(challenge, "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0");
    }

    #[test]
    fn pkce_challenge_round_trip_format() {
        let verifier = generate_pkce_verifier();
        let challenge = pkce_challenge(&verifier);
        // SHA-256 produces 32 bytes → base64url(32 bytes) = 43 chars (no padding).
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
        let (url, _verifier) = svc.authorization_url("csrf-state-token", &[]);

        assert!(url.starts_with("https://auth.atlassian.com/authorize"));
        assert!(url.contains("audience=api.atlassian.com"));
        assert!(url.contains(&format!("client_id={JIRA_CLIENT_ID}")));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("state=csrf-state-token"));
    }

    #[test]
    fn authorization_url_contains_pkce_params() {
        let svc = service();
        let (url, _verifier) = svc.authorization_url("state", &[]);
        assert!(url.contains("code_challenge="), "must include code_challenge");
        assert!(url.contains("code_challenge_method=S256"), "must use S256 method");
    }

    #[test]
    fn authorization_url_challenge_matches_verifier() {
        let svc = service();
        let (url, verifier) = svc.authorization_url("state", &[]);
        let expected_challenge = pkce_challenge(&verifier);
        // The challenge in the URL is percent-encoded; base64url uses only
        // A-Z a-z 0-9 - _  so no percent-encoding should occur.
        assert!(
            url.contains(&format!("code_challenge={expected_challenge}")),
            "challenge in URL does not match verifier"
        );
    }

    #[test]
    fn authorization_url_encodes_redirect_uri() {
        let svc = service();
        let (url, _) = svc.authorization_url("state", &[]);
        // The redirect URI contains "://" which gets percent-encoded
        assert!(url.contains("redirect_uri="));
        assert!(!url.contains("https://app.example.com/oauth/callback&"));
    }

    #[test]
    fn authorization_url_uses_default_scopes_when_empty() {
        let svc = service();
        let (url, _) = svc.authorization_url("state", &[]);
        // ':' encodes to %3A, spaces to %20
        assert!(url.contains("read%3Ajira-work"));
        assert!(url.contains("offline_access"));
    }

    #[test]
    fn authorization_url_uses_provided_scopes() {
        let svc = service();
        let (url, _) = svc.authorization_url("state", &["read:jira-work"]);
        assert!(url.contains("read%3Ajira-work"));
        // Should NOT contain read:jira-user if not provided
        assert!(!url.contains("read%3Ajira-user"));
    }

    #[test]
    fn authorization_url_state_is_included() {
        let svc = service();
        let (url, _) = svc.authorization_url("my-unique-csrf", &[]);
        assert!(url.contains("state=my-unique-csrf"));
    }

    #[test]
    fn authorization_url_no_client_secret() {
        let svc = service();
        let (url, _) = svc.authorization_url("state", &[]);
        assert!(!url.contains("client_secret"), "client_secret must not appear in auth URL");
    }

    // -----------------------------------------------------------------------
    // Error display tests
    // -----------------------------------------------------------------------

    #[test]
    fn oauth_error_display_auth_server() {
        let err = OAuthError::AuthServerError {
            error: "invalid_grant".into(),
            description: "Authorization code expired".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("invalid_grant"));
        assert!(msg.contains("Authorization code expired"));
    }

    #[test]
    fn oauth_error_display_parse_error() {
        let err = OAuthError::ParseError("bad json".into());
        assert!(err.to_string().contains("bad json"));
    }

    // -----------------------------------------------------------------------
    // Deserialization tests
    // -----------------------------------------------------------------------

    #[test]
    fn token_response_deserializes() {
        let json = r#"{
            "access_token": "tok123",
            "refresh_token": "ref456",
            "expires_in": 3600,
            "scope": "read:jira-work offline_access"
        }"#;
        let resp: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token, "tok123");
        assert_eq!(resp.refresh_token, Some("ref456".into()));
        assert_eq!(resp.expires_in, 3600);
    }

    #[test]
    fn token_response_no_refresh_token() {
        let json = r#"{
            "access_token": "tok123",
            "expires_in": 3600,
            "scope": "read:jira-work"
        }"#;
        let resp: TokenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.refresh_token.is_none());
    }

    #[test]
    fn cloud_resource_deserializes() {
        let json = r#"{
            "id": "abc-cloud-id",
            "name": "my-org",
            "url": "https://my-org.atlassian.net"
        }"#;
        let resource: CloudResource = serde_json::from_str(json).unwrap();
        assert_eq!(resource.id, "abc-cloud-id");
        assert_eq!(resource.name, "my-org");
        assert_eq!(resource.url, "https://my-org.atlassian.net");
    }
}
