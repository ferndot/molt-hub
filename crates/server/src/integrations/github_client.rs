//! GitHub REST API v3 client.
//!
//! Wraps the GitHub REST API with a thin async client.  Authentication
//! uses an OAuth access token (from the GitHub App OAuth flow) or a
//! personal access token, passed as a Bearer header.

use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

fn github_http_client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(45))
        .user_agent("molt-hub")
        .build()
        .unwrap_or_else(|_| Client::new())
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`GitHubClient`] operations.
#[derive(Debug, Error)]
pub enum GitHubError {
    /// HTTP-level transport error (connection refused, timeout, etc.).
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// The server returned HTTP 401 or 403.
    #[error("authentication failed (HTTP {status})")]
    AuthError { status: u16 },

    /// The response body could not be deserialised.
    #[error("parse error: {0}")]
    ParseError(String),

    /// GitHub returned a non-success HTTP status with a JSON error payload.
    #[error("GitHub API error (HTTP {status}): {message}")]
    ApiError { status: u16, message: String },

    /// Issue or pull request not found for this repository.
    #[error("issue not found: {repo}#{number}")]
    NotFound { repo: String, number: i64 },
}

// ---------------------------------------------------------------------------
// API response types
// ---------------------------------------------------------------------------

/// A GitHub issue as returned by the REST API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubIssue {
    /// Issue number within the repository.
    pub number: i64,
    /// Human-readable title.
    pub title: String,
    /// State: `"open"` or `"closed"`.
    pub state: String,
    /// Full body text (Markdown).
    pub body: Option<String>,
    /// Browser URL to the issue.
    pub html_url: String,
    /// Labels attached to the issue.
    #[serde(default)]
    pub labels: Vec<GitHubLabel>,
}

/// A label on a GitHub issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubLabel {
    /// Label display name.
    pub name: String,
}

/// A GitHub repository as returned by the REST API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRepo {
    /// Full repository name, e.g. `"owner/repo"`.
    pub full_name: String,
    /// Repository description.
    pub description: Option<String>,
    /// Browser URL to the repository.
    pub html_url: String,
}

// ---------------------------------------------------------------------------
// GitHubClient
// ---------------------------------------------------------------------------

/// Async GitHub REST API v3 client.
///
/// Authenticates via Bearer token against `https://api.github.com`.
pub struct GitHubClient {
    http: Client,
    token: String,
    base_url: String,
}

impl GitHubClient {
    /// Create a client from an OAuth access token, personal access token,
    /// or GitHub App installation token.
    pub fn new(token: String) -> Self {
        Self {
            http: github_http_client(),
            token,
            base_url: "https://api.github.com".to_owned(),
        }
    }

    /// Bearer token sent to the GitHub API.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Create a client with a custom base URL (useful for testing).
    #[cfg(test)]
    pub fn with_base_url(token: String, base_url: String) -> Self {
        Self {
            http: github_http_client(),
            token,
            base_url,
        }
    }

    /// List repositories accessible to the authenticated user.
    pub async fn list_repos(&self) -> Result<Vec<GitHubRepo>, GitHubError> {
        let url = format!("{}/user/repos", self.base_url);
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "molt-hub")
            .query(&[("per_page", "100"), ("sort", "updated")])
            .send()
            .await?;

        Self::check_auth(&response)?;

        Self::parse_success_json(response).await
    }

    /// GitHub login for the authenticated user (`GET /user`).
    pub async fn get_authenticated_user_login(&self) -> Result<String, GitHubError> {
        let url = format!("{}/user", self.base_url);
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "molt-hub")
            .send()
            .await?;

        Self::check_auth(&response)?;

        let user: GithubUser = Self::parse_success_json(response).await?;
        Ok(user.login)
    }

    /// Search issues in a repository.
    ///
    /// `query` is appended to the GitHub search qualifier
    /// `repo:{owner}/{repo}`.
    pub async fn search_issues(
        &self,
        owner: &str,
        repo: &str,
        query: &str,
    ) -> Result<Vec<GitHubIssue>, GitHubError> {
        let q = format!("repo:{owner}/{repo} {query}");
        let url = format!("{}/search/issues", self.base_url);
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "molt-hub")
            .query(&[("q", q.as_str()), ("per_page", "50")])
            .send()
            .await?;

        Self::check_auth(&response)?;

        let body: SearchResult = Self::parse_success_json(response).await?;
        Ok(body.items)
    }

    /// Fetch a single issue (or pull request) by number.
    pub async fn get_issue(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<GitHubIssue, GitHubError> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            self.base_url, owner, repo, number
        );
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "molt-hub")
            .send()
            .await?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Err(GitHubError::NotFound {
                repo: format!("{owner}/{repo}"),
                number,
            });
        }

        Self::check_auth(&response)?;

        let response = response
            .error_for_status()
            .map_err(GitHubError::HttpError)?;

        response
            .json()
            .await
            .map_err(|e| GitHubError::ParseError(e.to_string()))
    }

    /// Check for 401/403 status codes and return an `AuthError`.
    fn check_auth(response: &reqwest::Response) -> Result<(), GitHubError> {
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(GitHubError::AuthError {
                status: status.as_u16(),
            });
        }
        Ok(())
    }

    /// Read the full body, fail with [`GitHubError::ApiError`] if status is not success,
    /// then deserialize JSON. Avoids treating GitHub error JSON as a success shape (which
    /// produced misleading `parse error: error decoding response body`).
    async fn parse_success_json<T: DeserializeOwned>(
        response: reqwest::Response,
    ) -> Result<T, GitHubError> {
        let status = response.status();
        let bytes = response.bytes().await.map_err(GitHubError::HttpError)?;
        if !status.is_success() {
            return Err(GitHubError::ApiError {
                status: status.as_u16(),
                message: extract_github_api_message(&bytes),
            });
        }
        serde_json::from_slice(&bytes).map_err(|e| GitHubError::ParseError(e.to_string()))
    }
}

fn extract_github_api_message(bytes: &[u8]) -> String {
    #[derive(Deserialize)]
    struct GhErr {
        message: Option<String>,
    }
    serde_json::from_slice::<GhErr>(bytes)
        .ok()
        .and_then(|e| e.message)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            String::from_utf8_lossy(bytes)
                .trim()
                .chars()
                .take(200)
                .collect()
        })
}

// ---------------------------------------------------------------------------
// Internal API response shapes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SearchResult {
    items: Vec<GitHubIssue>,
}

#[derive(Deserialize)]
struct GithubUser {
    login: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_client_new_sets_base_url() {
        let client = GitHubClient::new("ghp_test123".into());
        assert_eq!(client.base_url, "https://api.github.com");
        assert_eq!(client.token, "ghp_test123");
    }

    #[test]
    fn github_client_with_base_url() {
        let client = GitHubClient::with_base_url("tok".into(), "http://localhost:9999".into());
        assert_eq!(client.base_url, "http://localhost:9999");
    }

    #[test]
    fn github_error_display() {
        let err = GitHubError::AuthError { status: 401 };
        assert!(err.to_string().contains("401"));

        let err2 = GitHubError::ParseError("bad json".into());
        assert!(err2.to_string().contains("bad json"));

        let err3 = GitHubError::ApiError {
            status: 422,
            message: "Validation Failed".into(),
        };
        assert!(err3.to_string().contains("422"));
        assert!(err3.to_string().contains("Validation Failed"));
    }

    #[test]
    fn extract_github_api_message_reads_json_message() {
        let j = br#"{"message":"Validation Failed","documentation_url":"https://docs.github.com"}"#;
        assert_eq!(super::extract_github_api_message(j), "Validation Failed");
    }

    #[test]
    fn github_issue_deserializes() {
        let json = serde_json::json!({
            "number": 42,
            "title": "Fix the thing",
            "state": "open",
            "body": "Detailed description",
            "html_url": "https://github.com/owner/repo/issues/42",
            "labels": [{"name": "bug"}, {"name": "urgent"}]
        });
        let issue: GitHubIssue = serde_json::from_value(json).expect("deserialize");
        assert_eq!(issue.number, 42);
        assert_eq!(issue.title, "Fix the thing");
        assert_eq!(issue.state, "open");
        assert_eq!(issue.body, Some("Detailed description".into()));
        assert_eq!(issue.labels.len(), 2);
        assert_eq!(issue.labels[0].name, "bug");
    }

    #[test]
    fn github_repo_deserializes() {
        let json = serde_json::json!({
            "full_name": "owner/repo",
            "description": "A cool repo",
            "html_url": "https://github.com/owner/repo"
        });
        let repo: GitHubRepo = serde_json::from_value(json).expect("deserialize");
        assert_eq!(repo.full_name, "owner/repo");
        assert_eq!(repo.description, Some("A cool repo".into()));
    }

    #[test]
    fn search_result_deserializes() {
        let json = serde_json::json!({
            "total_count": 1,
            "incomplete_results": false,
            "items": [{
                "number": 1,
                "title": "Issue 1",
                "state": "open",
                "body": null,
                "html_url": "https://github.com/o/r/issues/1",
                "labels": []
            }]
        });
        let result: SearchResult = serde_json::from_value(json).expect("deserialize");
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].number, 1);
    }
}
