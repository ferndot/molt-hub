//! GitHub App user OAuth with PKCE via [`oauth2`].

use oauth2::basic::{BasicClient, BasicErrorResponse, BasicTokenResponse};

macro_rules! github_oauth_client {
    ($svc:expr) => {{
        let mut c = BasicClient::new(ClientId::new($svc.client_id.clone()))
            .set_auth_uri(auth_url())
            .set_token_uri(token_url())
            .set_redirect_uri($svc.redirect_url.clone());
        if let Some(ref s) = $svc.client_secret {
            c = c.set_client_secret(ClientSecret::new(s.clone()));
        }
        c
    }};
}
use oauth2::TokenResponse as _;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, HttpClientError,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, RequestTokenError, TokenUrl,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const GITHUB_CLIENT_ID: &str = "Iv23lip4ZuqkEmT9Z2U0";
pub const GITHUB_CLIENT_SECRET: Option<&str> = option_env!("GITHUB_CLIENT_SECRET");
pub const GITHUB_CALLBACK_URL: &str =
    "http://localhost:13401/api/integrations/github/oauth/callback";

fn auth_url() -> AuthUrl {
    AuthUrl::new("https://github.com/login/oauth/authorize".to_string()).expect("static URL")
}

fn token_url() -> TokenUrl {
    TokenUrl::new("https://github.com/login/oauth/access_token".to_string()).expect("static URL")
}

#[derive(Debug, Error)]
pub enum GithubOAuthError {
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("OAuth error ({error}): {description}")]
    AuthServerError { error: String, description: String },
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("client secret not configured — set it via settings")]
    MissingClientSecret,
}

#[derive(Deserialize)]
struct GithubJsonError {
    error: String,
    #[serde(default)]
    error_description: String,
}

fn token_err(
    e: RequestTokenError<HttpClientError<reqwest::Error>, BasicErrorResponse>,
) -> GithubOAuthError {
    match e {
        RequestTokenError::ServerResponse(r) => GithubOAuthError::AuthServerError {
            error: r.error().to_string(),
            description: r.error_description().cloned().unwrap_or_default(),
        },
        RequestTokenError::Parse(p, body) => {
            if let Ok(err) = serde_json::from_slice::<GithubJsonError>(&body) {
                if !err.error.is_empty() {
                    return GithubOAuthError::AuthServerError {
                        error: err.error,
                        description: err.error_description,
                    };
                }
            }
            GithubOAuthError::ParseError(p.to_string())
        }
        RequestTokenError::Request(r) => GithubOAuthError::AuthServerError {
            error: "token_request_failed".into(),
            description: r.to_string(),
        },
        RequestTokenError::Other(o) => GithubOAuthError::ParseError(o),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token_expires_in: Option<u64>,
}

pub struct GithubOAuthService {
    client_id: String,
    redirect_url: RedirectUrl,
    http: Client,
    client_secret: Option<String>,
}

impl GithubOAuthService {
    pub fn new(redirect_uri: &str) -> Self {
        let redirect_url = RedirectUrl::new(redirect_uri.to_owned())
            .unwrap_or_else(|e| panic!("invalid GitHub OAuth redirect URI: {e}"));
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client");
        Self {
            client_id: GITHUB_CLIENT_ID.to_owned(),
            redirect_url,
            http,
            client_secret: None,
        }
    }

    pub fn with_secret(redirect_uri: &str, client_secret: String) -> Self {
        let mut s = Self::new(redirect_uri);
        s.client_secret = Some(client_secret);
        s
    }

    pub fn set_client_secret(&mut self, secret: String) {
        self.client_secret = Some(secret);
    }

    pub fn authorization_url(&self, state: &str) -> (String, String) {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, _) = github_oauth_client!(self)
            .authorize_url(|| CsrfToken::new(state.to_owned()))
            .set_pkce_challenge(challenge)
            .url();
        (url.to_string(), verifier.into_secret())
    }

    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<GithubTokenResponse, GithubOAuthError> {
        if self.client_secret.is_none() {
            return Err(GithubOAuthError::MissingClientSecret);
        }
        github_oauth_client!(self)
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .set_pkce_verifier(PkceCodeVerifier::new(code_verifier.to_owned()))
            .request_async(&self.http)
            .await
            .map_err(token_err)
            .map(from_basic)
    }

    pub async fn refresh_token(&self, rt: &str) -> Result<GithubTokenResponse, GithubOAuthError> {
        if self.client_secret.is_none() {
            return Err(GithubOAuthError::MissingClientSecret);
        }
        github_oauth_client!(self)
            .exchange_refresh_token(&RefreshToken::new(rt.to_owned()))
            .request_async(&self.http)
            .await
            .map_err(token_err)
            .map(from_basic)
    }
}

fn from_basic(t: BasicTokenResponse) -> GithubTokenResponse {
    let scope = t
        .scopes()
        .map(|scopes| {
            scopes
                .iter()
                .map(|s| s.as_str().to_owned())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    GithubTokenResponse {
        access_token: t.access_token().secret().clone(),
        token_type: t.token_type().as_ref().to_string(),
        scope,
        refresh_token: t.refresh_token().map(|r| r.secret().clone()),
        expires_in: t.expires_in().map(|d| d.as_secs()),
        refresh_token_expires_in: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oauth2::PkceCodeVerifier;

    #[test]
    fn authorize_url_github_pkce() {
        let svc = GithubOAuthService::new(GITHUB_CALLBACK_URL);
        let (url, verifier) = svc.authorization_url("st");
        assert!(url.contains("github.com/login/oauth/authorize"));
        let ch = PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(verifier));
        assert!(url.contains(&format!("code_challenge={}", ch.as_str())));
    }

    #[tokio::test]
    async fn exchange_requires_secret() {
        let svc = GithubOAuthService::new(GITHUB_CALLBACK_URL);
        assert!(matches!(
            svc.exchange_code("c", "v").await.unwrap_err(),
            GithubOAuthError::MissingClientSecret
        ));
    }
}
