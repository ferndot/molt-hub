//! Jira REST API v3 client.
//!
//! Wraps the Jira Cloud REST API with a thin async client.  Authentication
//! uses OAuth 2.0 Bearer tokens — call [`JiraClient::from_oauth`] to construct
//! a client from an access token and Atlassian cloud ID.

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`JiraClient`] operations.
#[derive(Debug, Error)]
pub enum JiraError {
    /// HTTP-level transport error (connection refused, timeout, etc.).
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// The server returned HTTP 401 or 403.
    #[error("authentication failed (HTTP {status})")]
    AuthError { status: u16 },

    /// The requested resource was not found (HTTP 404).
    #[error("resource not found: {key}")]
    NotFound { key: String },

    /// The response body could not be deserialised.
    #[error("parse error: {0}")]
    ParseError(String),

    /// Jira returned a non-success HTTP status (e.g. invalid JQL → 400).
    #[error("Jira API error (HTTP {status}): {message}")]
    ApiError { status: u16, message: String },
}

// ---------------------------------------------------------------------------
// API response types
// ---------------------------------------------------------------------------

/// A Jira issue as returned by the REST API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraIssue {
    /// Issue key, e.g. `PROJ-42`.
    pub key: String,
    /// Human-readable summary line.
    pub summary: String,
    /// Full description text (Jira wiki markup or ADF stripped to plain text).
    pub description: Option<String>,
    /// Status category name (e.g. "To Do", "In Progress", "Done").
    pub status: String,
    /// Priority name (e.g. "High", "Medium", "Low").
    pub priority: Option<String>,
    /// Issue labels.
    pub labels: Vec<String>,
    /// Browser URL to the issue.
    pub url: String,
}

/// A Jira project as returned by the REST API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraProject {
    /// Short project key (e.g. `PROJ`).
    pub key: String,
    /// Display name.
    pub name: String,
}

// ---------------------------------------------------------------------------
// Internal API response shapes (not exported)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SearchResponse {
    issues: Vec<IssueRaw>,
}

#[derive(Deserialize)]
struct IssueRaw {
    key: String,
    #[serde(rename = "self")]
    self_url: String,
    fields: IssueFields,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueFields {
    summary: String,
    description: Option<serde_json::Value>,
    status: StatusField,
    priority: Option<PriorityField>,
    labels: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct StatusField {
    name: String,
}

#[derive(Deserialize)]
struct PriorityField {
    name: String,
}

#[derive(Deserialize)]
struct ProjectRaw {
    key: String,
    name: String,
}

/// Typical Jira REST error envelope (`errorMessages`, `errors`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JiraRestErrorEnvelope {
    #[serde(default)]
    error_messages: Vec<String>,
    #[serde(default)]
    errors: std::collections::HashMap<String, serde_json::Value>,
}

fn summarize_jira_error_response(body: &str) -> String {
    if let Ok(env) = serde_json::from_str::<JiraRestErrorEnvelope>(body) {
        let mut parts: Vec<String> = env
            .error_messages
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        for (k, v) in env.errors {
            let vstr = match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            parts.push(format!("{k}: {vstr}"));
        }
        if !parts.is_empty() {
            return parts.join("; ");
        }
    }
    let t = body.trim();
    if t.is_empty() {
        "(empty response body)".to_owned()
    } else if t.len() > 400 {
        format!("{}…", &t[..400])
    } else {
        t.to_owned()
    }
}

// ---------------------------------------------------------------------------
// JiraClient
// ---------------------------------------------------------------------------

/// Async Jira REST API v3 client (OAuth 2.0).
///
/// Authenticates via Bearer token against the Atlassian cloud API endpoint:
/// `https://api.atlassian.com/ex/jira/{cloud_id}/rest/api/3/`
pub struct JiraClient {
    client: Client,
    /// Base URL including the cloud ID prefix.
    /// e.g. `https://api.atlassian.com/ex/jira/abc-cloud-id/rest/api/3`
    pub(crate) base_url: String,
    /// OAuth 2.0 access token.
    pub(crate) access_token: String,
}

impl JiraClient {
    /// Read the response body as text, then map non-success statuses to [`JiraError::ApiError`].
    async fn response_text(response: reqwest::Response) -> Result<String, JiraError> {
        Self::check_auth_response(&response)?;
        let status = response.status();
        let bytes = response.bytes().await?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if !status.is_success() {
            return Err(JiraError::ApiError {
                status: status.as_u16(),
                message: summarize_jira_error_response(text.trim()),
            });
        }
        Ok(text)
    }

    /// Create a client from an OAuth 2.0 access token and Atlassian cloud ID.
    ///
    /// `cloud_id` is the UUID obtained from the accessible-resources endpoint
    /// (see [`crate::integrations::oauth::JiraOAuthService::get_accessible_resources`]).
    pub fn from_oauth(cloud_id: &str, access_token: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: format!("https://api.atlassian.com/ex/jira/{}/rest/api/3", cloud_id),
            access_token: access_token.to_owned(),
        }
    }

    /// Search for issues using JQL.
    ///
    /// Returns up to `max_results` issues.
    pub async fn search_issues(
        &self,
        jql: &str,
        max_results: u32,
    ) -> Result<Vec<JiraIssue>, JiraError> {
        let url = format!("{}/search", self.base_url);
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .header("Accept", "application/json")
            .query(&[
                ("jql", jql),
                ("maxResults", &max_results.to_string()),
                ("fields", "summary,description,status,priority,labels"),
            ])
            .send()
            .await?;

        let text = Self::response_text(response).await?;
        let body: SearchResponse = serde_json::from_str(text.trim()).map_err(|e| {
            JiraError::ParseError(format!("invalid search response JSON: {e}"))
        })?;

        Ok(body.issues.into_iter().map(raw_to_issue).collect())
    }

    /// Fetch a single issue by key.
    pub async fn get_issue(&self, issue_key: &str) -> Result<JiraIssue, JiraError> {
        let url = format!("{}/issue/{issue_key}", self.base_url);
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .header("Accept", "application/json")
            .query(&[("fields", "summary,description,status,priority,labels")])
            .send()
            .await?;

        Self::check_auth_response(&response)?;
        let status = response.status();
        let bytes = response.bytes().await?;
        let text = String::from_utf8_lossy(&bytes);

        if status == StatusCode::NOT_FOUND {
            return Err(JiraError::NotFound {
                key: issue_key.to_owned(),
            });
        }

        if !status.is_success() {
            return Err(JiraError::ApiError {
                status: status.as_u16(),
                message: summarize_jira_error_response(text.trim()),
            });
        }

        let raw: IssueRaw = serde_json::from_str(text.trim()).map_err(|e| {
            JiraError::ParseError(format!("invalid issue JSON: {e}"))
        })?;

        Ok(raw_to_issue(raw))
    }

    /// List all projects visible to the authenticated user.
    pub async fn list_projects(&self) -> Result<Vec<JiraProject>, JiraError> {
        let url = format!("{}/project", self.base_url);
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .header("Accept", "application/json")
            .send()
            .await?;

        let text = Self::response_text(response).await?;
        let projects: Vec<ProjectRaw> = serde_json::from_str(text.trim()).map_err(|e| {
            JiraError::ParseError(format!("invalid project list JSON: {e}"))
        })?;

        Ok(projects
            .into_iter()
            .map(|p| JiraProject {
                key: p.key,
                name: p.name,
            })
            .collect())
    }

    /// Check for 401/403 status codes and return an `AuthError`.
    fn check_auth_response(response: &reqwest::Response) -> Result<(), JiraError> {
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(JiraError::AuthError {
                status: status.as_u16(),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Raw → domain conversion
// ---------------------------------------------------------------------------

fn raw_to_issue(raw: IssueRaw) -> JiraIssue {
    // Derive the browser URL from the self_url (API) by replacing
    // /rest/api/3/issue/KEY with /browse/KEY.
    //
    // For the new OAuth API endpoint the self_url looks like:
    //   https://api.atlassian.com/ex/jira/{cloud_id}/rest/api/3/issue/KEY
    // We convert this to the human-readable site URL by extracting the key:
    //   https://{site}.atlassian.net/browse/KEY
    //
    // However, we don't always have the site URL at this point, so we fall
    // back to the self_url if the pattern doesn't match.
    let url = {
        if let Some(pos) = raw.self_url.find("/rest/api/") {
            // Works for site-hosted and cloud API `self_url` shapes.
            // For cloud API URLs this gives us a non-browsable URL, but it's
            // still a unique identifier.  A future improvement could store the
            // site URL alongside the client and use it here.
            format!("{}/browse/{}", &raw.self_url[..pos], raw.key)
        } else {
            raw.self_url
        }
    };

    let description = raw.fields.description.map(|v| extract_plain_text(&v));

    JiraIssue {
        key: raw.key,
        summary: raw.fields.summary,
        description,
        status: raw.fields.status.name,
        priority: raw.fields.priority.map(|p| p.name),
        labels: raw.fields.labels.unwrap_or_default(),
        url,
    }
}

/// Extract plain text from a Jira ADF document or plain string value.
///
/// Jira v3 API returns description as Atlassian Document Format (ADF) JSON.
/// This function does a best-effort extraction — it walks the doc tree and
/// collects text nodes.  For plain string fallbacks it returns the value
/// directly.
fn extract_plain_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(obj) => {
            // ADF root node has a "content" array
            if let Some(content) = obj.get("content") {
                extract_text_from_content(content)
            } else if let Some(text) = obj.get("text") {
                text.as_str().unwrap_or("").to_owned()
            } else {
                String::new()
            }
        }
        serde_json::Value::Array(arr) => arr
            .iter()
            .map(extract_plain_text)
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn extract_text_from_content(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::Array(arr) => arr
            .iter()
            .map(|node| {
                if let Some(text) = node.get("text").and_then(|t| t.as_str()) {
                    text.to_owned()
                } else if let Some(sub_content) = node.get("content") {
                    extract_text_from_content(sub_content)
                } else {
                    String::new()
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jira_client_from_oauth_builds_correct_base_url() {
        let client = JiraClient::from_oauth("my-cloud-id", "access-token-123");
        assert_eq!(
            client.base_url,
            "https://api.atlassian.com/ex/jira/my-cloud-id/rest/api/3"
        );
    }

    #[test]
    fn jira_client_from_oauth_stores_access_token() {
        let client = JiraClient::from_oauth("cloud-id", "my-bearer-token");
        assert_eq!(client.access_token, "my-bearer-token");
    }

    #[test]
    fn jira_client_uses_bearer_auth_not_basic() {
        // Verifying that we store access_token (Bearer) rather than email+api_token.
        let client = JiraClient::from_oauth("cloud-id", "token-xyz");
        // There should be no email/api_token fields.
        assert_eq!(client.access_token, "token-xyz");
    }

    #[test]
    fn raw_to_issue_derives_browser_url() {
        let raw = IssueRaw {
            key: "PROJ-1".into(),
            self_url: "https://org.atlassian.net/rest/api/3/issue/PROJ-1".into(),
            fields: IssueFields {
                summary: "Test issue".into(),
                description: None,
                status: StatusField {
                    name: "To Do".into(),
                },
                priority: Some(PriorityField {
                    name: "High".into(),
                }),
                labels: Some(vec!["bug".into()]),
            },
        };

        let issue = raw_to_issue(raw);
        assert_eq!(issue.key, "PROJ-1");
        assert_eq!(issue.summary, "Test issue");
        assert_eq!(issue.url, "https://org.atlassian.net/browse/PROJ-1");
        assert_eq!(issue.status, "To Do");
        assert_eq!(issue.priority, Some("High".into()));
        assert_eq!(issue.labels, vec!["bug"]);
    }

    #[test]
    fn raw_to_issue_cloud_api_url() {
        // When self_url comes from the cloud API endpoint.
        let raw = IssueRaw {
            key: "PROJ-42".into(),
            self_url: "https://api.atlassian.com/ex/jira/cloud-id/rest/api/3/issue/PROJ-42".into(),
            fields: IssueFields {
                summary: "OAuth issue".into(),
                description: None,
                status: StatusField {
                    name: "Done".into(),
                },
                priority: None,
                labels: None,
            },
        };

        let issue = raw_to_issue(raw);
        assert_eq!(issue.key, "PROJ-42");
        // URL is derived from self_url with /rest/api replaced by /browse
        assert!(issue.url.ends_with("/browse/PROJ-42"));
    }

    #[test]
    fn raw_to_issue_plain_description() {
        let raw = IssueRaw {
            key: "PROJ-2".into(),
            self_url: "https://org.atlassian.net/rest/api/3/issue/PROJ-2".into(),
            fields: IssueFields {
                summary: "Test".into(),
                description: Some(serde_json::json!("plain text description")),
                status: StatusField {
                    name: "Done".into(),
                },
                priority: None,
                labels: None,
            },
        };
        let issue = raw_to_issue(raw);
        assert_eq!(issue.description, Some("plain text description".into()));
    }

    #[test]
    fn extract_plain_text_from_adf_doc() {
        // Minimal ADF document with one paragraph and one text node.
        let adf = serde_json::json!({
            "version": 1,
            "type": "doc",
            "content": [
                {
                    "type": "paragraph",
                    "content": [
                        { "type": "text", "text": "Hello " },
                        { "type": "text", "text": "world" }
                    ]
                }
            ]
        });
        let text = extract_plain_text(&adf);
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn jira_error_display() {
        let err = JiraError::NotFound {
            key: "PROJ-99".into(),
        };
        assert!(err.to_string().contains("PROJ-99"));

        let err2 = JiraError::AuthError { status: 401 };
        assert!(err2.to_string().contains("401"));
    }

    #[test]
    fn summarize_jira_error_extracts_error_messages() {
        let body = r#"{"errorMessages":["Expected operator but got 0."],"errors":{}}"#;
        assert_eq!(
            summarize_jira_error_response(body),
            "Expected operator but got 0."
        );
    }

    #[test]
    fn summarize_jira_error_joins_errors_map() {
        let body = r#"{"errorMessages":[],"errors":{"jql":"invalid"}}"#;
        assert_eq!(summarize_jira_error_response(body), "jql: invalid");
    }
}
