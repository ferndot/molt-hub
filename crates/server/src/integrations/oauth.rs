//! Atlassian OAuth 2.0 (3LO) with PKCE via [`oauth2`].

use oauth2::basic::{BasicClient, BasicErrorResponse, BasicTokenResponse};

macro_rules! jira_oauth_client {
    ($svc:expr) => {
        BasicClient::new(ClientId::new($svc.client_id.clone()))
            .set_auth_uri(auth_url())
            .set_token_uri(token_url())
            .set_redirect_uri($svc.redirect_url.clone())
    };
}
use oauth2::TokenResponse as _;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, HttpClientError, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RefreshToken, RequestTokenError, Scope, TokenUrl,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const JIRA_CLIENT_ID: &str = "3yQWy34WyjCn0wtOfawofBTMmtK3gUgs";

fn auth_url() -> AuthUrl {
    AuthUrl::new("https://auth.atlassian.com/authorize".to_string()).expect("static URL")
}

fn token_url() -> TokenUrl {
    TokenUrl::new("https://auth.atlassian.com/oauth/token".to_string()).expect("static URL")
}

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("OAuth error ({error}): {description}")]
    AuthServerError { error: String, description: String },
    #[error("parse error: {0}")]
    ParseError(String),
}

fn token_err(
    e: RequestTokenError<HttpClientError<reqwest::Error>, BasicErrorResponse>,
) -> OAuthError {
    match e {
        RequestTokenError::ServerResponse(r) => OAuthError::AuthServerError {
            error: r.error().to_string(),
            description: r.error_description().cloned().unwrap_or_default(),
        },
        RequestTokenError::Parse(p, _) => OAuthError::ParseError(p.to_string()),
        RequestTokenError::Request(r) => OAuthError::AuthServerError {
            error: "token_request_failed".into(),
            description: r.to_string(),
        },
        RequestTokenError::Other(o) => OAuthError::ParseError(o),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudResource {
    pub id: String,
    pub name: String,
    pub url: String,
}

pub const DEFAULT_SCOPES: &[&str] = &[
    "read:jira-work",
    "read:jira-user",
    "write:jira-work",
    "read:sprint:jira-software",
    "read:board-scope:jira-software",
    "offline_access",
];

pub struct JiraOAuthService {
    client_id: String,
    redirect_url: RedirectUrl,
    http: Client,
}

impl JiraOAuthService {
    pub fn new(redirect_uri: &str) -> Self {
        let client_id =
            std::env::var("MOLTHUB_JIRA_CLIENT_ID").unwrap_or_else(|_| JIRA_CLIENT_ID.to_owned());
        let redirect_url = RedirectUrl::new(redirect_uri.to_owned())
            .unwrap_or_else(|e| panic!("invalid Jira OAuth redirect URI: {e}"));
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client");
        Self {
            client_id,
            redirect_url,
            http,
        }
    }

    pub fn authorization_url(&self, state: &str, scopes: &[&str]) -> (String, String) {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let c = jira_oauth_client!(self);
        let mut req = c
            .authorize_url(|| CsrfToken::new(state.to_owned()))
            .set_pkce_challenge(challenge)
            .add_extra_param("audience", "api.atlassian.com")
            .add_extra_param("prompt", "consent");
        if scopes.is_empty() {
            for s in DEFAULT_SCOPES {
                req = req.add_scope(Scope::new((*s).to_string()));
            }
        } else {
            for s in scopes {
                req = req.add_scope(Scope::new((*s).to_string()));
            }
        }
        let (url, _) = req.url();
        (url.to_string(), verifier.into_secret())
    }

    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<JiraTokenResponse, OAuthError> {
        let t = jira_oauth_client!(self)
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .set_pkce_verifier(PkceCodeVerifier::new(code_verifier.to_owned()))
            .request_async(&self.http)
            .await
            .map_err(token_err)?;
        Ok(from_basic(t))
    }

    pub async fn refresh_token(&self, rt: &str) -> Result<JiraTokenResponse, OAuthError> {
        let t = jira_oauth_client!(self)
            .exchange_refresh_token(&RefreshToken::new(rt.to_owned()))
            .request_async(&self.http)
            .await
            .map_err(token_err)?;
        Ok(from_basic(t))
    }

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

        #[derive(Deserialize)]
        struct Raw {
            id: String,
            name: String,
            url: String,
        }

        let resources: Vec<Raw> = response
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
}

fn from_basic(t: BasicTokenResponse) -> JiraTokenResponse {
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
    JiraTokenResponse {
        access_token: t.access_token().secret().clone(),
        refresh_token: t.refresh_token().map(|r| r.secret().clone()),
        expires_in: t.expires_in().map(|d| d.as_secs()).unwrap_or(0),
        scope,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oauth2::PkceCodeVerifier;

    #[test]
    fn authorize_url_has_atlassian_fields_and_pkce() {
        let svc = JiraOAuthService::new("https://app.example.com/oauth/callback");
        let (url, verifier) = svc.authorization_url("csrf", &[]);
        assert!(url.starts_with("https://auth.atlassian.com/authorize"));
        assert!(url.contains("audience=api.atlassian.com"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("code_challenge_method=S256"));
        let ch = PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(verifier));
        assert!(url.contains(&format!("code_challenge={}", ch.as_str())));
    }

    #[test]
    fn jira_token_json_roundtrip() {
        let json = r#"{"access_token":"a","refresh_token":"r","expires_in":3600,"scope":"s"}"#;
        let t: JiraTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(t.access_token, "a");
    }
}
