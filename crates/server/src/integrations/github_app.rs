//! GitHub App authentication: JWT, installation access tokens, and webhook verification.
//!
//! # Environment
//!
//! | Variable | Purpose |
//! |----------|---------|
//! | `GITHUB_APP_ID` | Numeric GitHub App ID. With a private key, enables installation access tokens. |
//! | `GITHUB_APP_PRIVATE_KEY` | PEM-encoded RSA private key (use `\n` inside the value if single-line). |
//! | `GITHUB_APP_PRIVATE_KEY_PATH` | Filesystem path to a PEM file (used when `GITHUB_APP_PRIVATE_KEY` is unset). |
//! | `GITHUB_APP_SLUG` | Slug in `https://github.com/apps/{slug}/installations/new`. |
//! | `GITHUB_WEBHOOK_SECRET` | If set, [`verify_webhook_signature`] checks `X-Hub-Signature-256`. |
//!
//! User-to-server OAuth (PKCE) remains available as a fallback when the app is not fully
//! configured or the user has not completed an installation.

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;

use super::github_client::{GitHubError, GitHubRepo};

type HmacSha256 = Hmac<Sha256>;

/// Loaded GitHub App credentials for JWT signing.
#[derive(Debug, Clone)]
pub struct GithubAppCredentials {
    app_id: u64,
    private_key_pem: String,
}

/// Errors from loading app configuration or calling GitHub App APIs.
#[derive(Debug, Error)]
pub enum GithubAppError {
    #[error("GITHUB_APP_ID is set but could not be parsed as a u64")]
    InvalidAppId,

    #[error("GitHub App private key: {0}")]
    PrivateKey(String),

    #[error("JWT encode error: {0}")]
    Jwt(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("GitHub API error: {0}")]
    Api(String),

    #[error("parse error: {0}")]
    Parse(String),
}

impl GithubAppCredentials {
    /// Load from environment when `GITHUB_APP_ID` is set; otherwise `None`.
    pub fn try_from_env() -> Result<Option<Self>, GithubAppError> {
        let raw = match std::env::var("GITHUB_APP_ID") {
            Ok(s) if !s.trim().is_empty() => s,
            _ => return Ok(None),
        };
        let app_id: u64 = raw
            .trim()
            .parse()
            .map_err(|_| GithubAppError::InvalidAppId)?;

        let pem = load_private_key_pem()?;
        Ok(Some(Self {
            app_id,
            private_key_pem: pem,
        }))
    }

    pub fn app_id(&self) -> u64 {
        self.app_id
    }

    /// GitHub App installation URL for the configured slug (`GITHUB_APP_SLUG`).
    pub fn installations_new_url(slug: &str, state: &str) -> String {
        format!(
            "https://github.com/apps/{}/installations/new?state={}",
            urlencoding_encode(slug),
            urlencoding_encode(state)
        )
    }

    /// Create a short-lived JWT (RS256) for GitHub App API calls (max 10 minutes).
    pub fn create_jwt(&self) -> Result<String, GithubAppError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| GithubAppError::Jwt(e.to_string()))?
            .as_secs() as i64;
        let exp = now + 600;
        let claims = AppJwtClaims {
            iat: now,
            exp,
            iss: self.app_id,
        };
        let key = EncodingKey::from_rsa_pem(self.private_key_pem.as_bytes())
            .map_err(|e| GithubAppError::PrivateKey(e.to_string()))?;
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".into());
        encode(&header, &claims, &key).map_err(|e| GithubAppError::Jwt(e.to_string()))
    }

    /// Exchange JWT for a one-hour installation access token.
    pub async fn create_installation_access_token(
        &self,
        http: &Client,
        installation_id: i64,
    ) -> Result<InstallationAccessToken, GithubAppError> {
        let jwt = self.create_jwt()?;
        let url =
            format!("https://api.github.com/app/installations/{installation_id}/access_tokens");
        let response = http
            .post(&url)
            .bearer_auth(&jwt)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "molt-hub")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(GithubAppError::Api(format!("HTTP {status}: {body}")));
        }

        let parsed: InstallationTokenResponse = response
            .json()
            .await
            .map_err(|e| GithubAppError::Parse(e.to_string()))?;

        let expires_at = DateTime::parse_from_rfc3339(&parsed.expires_at)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| GithubAppError::Parse(e.to_string()))?;

        Ok(InstallationAccessToken {
            token: parsed.token,
            expires_at,
        })
    }

    /// `account.login` for the installation (JWT auth).
    pub async fn installation_account_login(
        &self,
        http: &Client,
        installation_id: i64,
    ) -> Result<String, GithubAppError> {
        let jwt = self.create_jwt()?;
        let url = format!("https://api.github.com/app/installations/{installation_id}");
        let response = http
            .get(&url)
            .bearer_auth(&jwt)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "molt-hub")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(GithubAppError::Api(format!("HTTP {status}: {body}")));
        }

        let inst: InstallationDetail = response
            .json()
            .await
            .map_err(|e| GithubAppError::Parse(e.to_string()))?;

        Ok(inst.account.login)
    }
}

/// Short-lived token for GitHub API calls as an installation.
#[derive(Debug, Clone)]
pub struct InstallationAccessToken {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct AppJwtClaims {
    iat: i64,
    exp: i64,
    iss: u64,
}

#[derive(Deserialize)]
struct InstallationTokenResponse {
    token: String,
    expires_at: String,
}

#[derive(Deserialize)]
struct InstallationDetail {
    account: InstallationAccount,
}

#[derive(Deserialize)]
struct InstallationAccount {
    login: String,
}

fn load_private_key_pem() -> Result<String, GithubAppError> {
    if let Ok(pem) = std::env::var("GITHUB_APP_PRIVATE_KEY") {
        let t = pem.trim();
        if !t.is_empty() {
            return Ok(pem.replace("\\n", "\n"));
        }
    }
    let path = std::env::var("GITHUB_APP_PRIVATE_KEY_PATH").map_err(|_| {
        GithubAppError::PrivateKey(
            "set GITHUB_APP_PRIVATE_KEY or GITHUB_APP_PRIVATE_KEY_PATH when GITHUB_APP_ID is set"
                .into(),
        )
    })?;
    std::fs::read_to_string(path.trim()).map_err(|e| GithubAppError::PrivateKey(e.to_string()))
}

/// Public app slug from `GITHUB_APP_SLUG` (trimmed); empty if unset.
pub fn github_app_slug_from_env() -> Option<String> {
    std::env::var("GITHUB_APP_SLUG")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Webhook secret from `GITHUB_WEBHOOK_SECRET`; `None` if unset or empty.
pub fn github_webhook_secret_from_env() -> Option<Vec<u8>> {
    std::env::var("GITHUB_WEBHOOK_SECRET")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| s.into_bytes())
}

/// Verify `X-Hub-Signature-256: sha256=<hex>` for the raw webhook body.
pub fn verify_webhook_signature(secret: &[u8], body: &[u8], signature_header: &str) -> bool {
    let Some(hex_part) = signature_header.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex::decode(hex_part.trim()) else {
        return false;
    };
    let mut mac = match HmacSha256::new_from_slice(secret) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(body);
    let computed = mac.finalize().into_bytes();
    if computed.len() != expected.len() {
        return false;
    }
    computed.as_slice().ct_eq(expected.as_slice()).into()
}

fn urlencoding_encode(s: &str) -> String {
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
        .collect()
}

/// Fetch repositories visible to an installation token (`GET /installation/repositories`).
pub async fn list_installation_repositories(
    http: &Client,
    installation_token: &str,
) -> Result<Vec<GitHubRepo>, GitHubError> {
    let base = "https://api.github.com";
    let mut all = Vec::new();
    let mut page = 1u32;
    loop {
        let url = format!("{base}/installation/repositories");
        let response = http
            .get(&url)
            .bearer_auth(installation_token)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "molt-hub")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .query(&[("per_page", "100"), ("page", &page.to_string())])
            .send()
            .await?;

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(GitHubError::AuthError {
                status: status.as_u16(),
            });
        }

        let body: InstallationRepositoriesPage = response
            .json()
            .await
            .map_err(|e| GitHubError::ParseError(e.to_string()))?;

        let n = body.repositories.len() as u32;
        all.extend(body.repositories);
        if n < 100 {
            break;
        }
        page += 1;
        if page > 50 {
            break;
        }
    }
    Ok(all)
}

#[derive(Deserialize)]
struct InstallationRepositoriesPage {
    repositories: Vec<GitHubRepo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PEM: &str = include_str!("../../tests/fixtures/github_app_test_rsa.pem");

    #[test]
    fn jwt_round_trip_encode() {
        let creds = GithubAppCredentials {
            app_id: 12345,
            private_key_pem: TEST_PEM.to_string(),
        };
        let jwt = creds.create_jwt().expect("jwt");
        assert!(!jwt.is_empty());
        assert_eq!(jwt.matches('.').count(), 2);
    }

    #[test]
    fn verify_webhook_signature_accepts_valid_mac() {
        let secret = b"mysecret";
        let body = b"payload";
        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        mac.update(body);
        let hex = hex::encode(mac.finalize().into_bytes());
        let header = format!("sha256={hex}");
        assert!(verify_webhook_signature(secret, body, &header));
    }

    #[test]
    fn verify_webhook_signature_rejects_bad_mac() {
        let secret = b"mysecret";
        let body = b"payload";
        assert!(!verify_webhook_signature(secret, body, "sha256=deadbeef"));
    }
}
