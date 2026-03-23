//! Integration configuration types and shared external item representation.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Auth — OAuth 2.0 only
// ---------------------------------------------------------------------------

/// Jira OAuth 2.0 (3LO) authentication credentials.
///
/// `auth: None` in [`JiraConfig`] means the user has not yet completed the
/// OAuth flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraAuth {
    /// Short-lived access token.
    pub access_token: String,
    /// Long-lived refresh token used to obtain new access tokens.
    pub refresh_token: Option<String>,
    /// Expiry as a Unix timestamp (seconds since epoch).
    pub expires_at: Option<u64>,
    /// Atlassian cloud ID used for API routing (e.g. `"97abc123-…"`).
    pub cloud_id: Option<String>,
}

/// OAuth 2.0 application registration settings.
///
/// These are the static credentials registered in the Atlassian developer
/// console — they are distinct from the per-user [`JiraAuth`] tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    /// Client ID issued by the OAuth provider.
    pub client_id: String,
    /// Client secret issued by the OAuth provider.
    pub client_secret: String,
    /// Redirect URI registered with the OAuth provider.
    pub redirect_uri: String,
    /// OAuth scopes to request, e.g. `["read:jira-work", "write:jira-work"]`.
    pub scopes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Typed integration configs
// ---------------------------------------------------------------------------

/// Connection settings for a Jira Cloud or Server instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraConfig {
    /// Base URL of the Jira instance, e.g. `https://myorg.atlassian.net`.
    pub base_url: String,
    /// Jira project key to scope queries, e.g. `"PROJ"`.  `None` means all
    /// accessible projects.
    pub project_key: Option<String>,
    /// OAuth credentials.  `None` means the user has not yet authenticated.
    pub auth: Option<JiraAuth>,
}

/// Connection settings for a GitHub repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConfig {
    /// Repository owner (user or org), e.g. `"acme"`.
    pub owner: String,
    /// Repository name, e.g. `"my-app"`.
    pub repo: String,
    /// Personal access token or GitHub App installation token.
    pub token: String,
}

/// Connection settings for an outbound webhook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Target URL that receives POST requests.
    pub url: String,
    /// Optional shared secret for HMAC signature verification.
    pub secret: Option<String>,
}

// ---------------------------------------------------------------------------
// IntegrationConfig enum
// ---------------------------------------------------------------------------

/// All supported integration back-ends.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IntegrationConfig {
    Jira(JiraConfig),
    GitHub(GitHubConfig),
    Webhook(WebhookConfig),
}

// ---------------------------------------------------------------------------
// ExternalItem — canonical representation of a fetched item
// ---------------------------------------------------------------------------

/// An item fetched from an external system, normalized to a common shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalItem {
    /// External system identifier (e.g. Jira issue key `"PROJ-123"`).
    pub id: String,
    /// Short human-readable title.
    pub title: String,
    /// Full description or body text, if available.
    pub description: Option<String>,
    /// Current status in the external system (raw string from source).
    pub status: String,
    /// Priority label from the external system.
    pub priority: ExternalPriority,
    /// Labels or tags attached to the item.
    pub labels: Vec<String>,
    /// Direct URL to the item in the external system's UI.
    pub url: String,
    /// Identifies which integration produced this item (e.g. `"jira"`, `"github"`).
    pub source: String,
}

/// Priority levels normalized from external systems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalPriority {
    P0,
    P1,
    P2,
    P3,
    /// Priority was present but could not be mapped to P0–P3.
    Unknown(String),
}

// ---------------------------------------------------------------------------
// SyncStatus
// ---------------------------------------------------------------------------

/// The synchronisation state between a Molt Hub task and an external item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SyncStatus {
    /// Molt Hub and the external system agree on the current state.
    Synced,
    /// The two systems have diverged — manual reconciliation may be needed.
    Diverged {
        /// Human-readable description of what differs.
        detail: String,
    },
    /// The item no longer exists in the external system.
    NotFound,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jira_auth_serializes_and_deserializes() {
        let auth = JiraAuth {
            access_token: "tok_abc".to_string(),
            refresh_token: Some("ref_xyz".to_string()),
            expires_at: Some(9_999_999_999),
            cloud_id: Some("cloud-123".to_string()),
        };
        let json = serde_json::to_string(&auth).expect("serialize");
        let back: JiraAuth = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.access_token, "tok_abc");
        assert_eq!(back.refresh_token.as_deref(), Some("ref_xyz"));
        assert_eq!(back.expires_at, Some(9_999_999_999));
        assert_eq!(back.cloud_id.as_deref(), Some("cloud-123"));
    }

    #[test]
    fn jira_config_with_no_auth_serializes() {
        let cfg = JiraConfig {
            base_url: "https://myorg.atlassian.net".to_string(),
            project_key: None,
            auth: None,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(json.contains("myorg.atlassian.net"));
        let back: JiraConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(back.auth.is_none());
        assert!(back.project_key.is_none());
    }

    #[test]
    fn jira_config_with_auth_and_project_key() {
        let cfg = JiraConfig {
            base_url: "https://example.atlassian.net".to_string(),
            project_key: Some("PROJ".to_string()),
            auth: Some(JiraAuth {
                access_token: "access".to_string(),
                refresh_token: None,
                expires_at: None,
                cloud_id: None,
            }),
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: JiraConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.project_key.as_deref(), Some("PROJ"));
        assert!(back.auth.is_some());
    }

    #[test]
    fn oauth_config_round_trips() {
        let cfg = OAuthConfig {
            client_id: "client-id".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scopes: vec!["read:jira-work".to_string(), "write:jira-work".to_string()],
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: OAuthConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.client_id, "client-id");
        assert_eq!(back.scopes.len(), 2);
    }
}
