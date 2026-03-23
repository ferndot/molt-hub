//! Jira-specific types and conversions.
//!
//! `JiraIssue` matches the shape returned by the Jira REST API v3
//! `GET /rest/api/3/issue/{issueKey}` endpoint (relevant fields only).
//! `From<JiraIssue> for ExternalItem` normalises it to the canonical shape.

use serde::{Deserialize, Serialize};

use super::config::{ExternalItem, ExternalPriority};

// ---------------------------------------------------------------------------
// Jira REST API response shape
// ---------------------------------------------------------------------------

/// Top-level object returned by the Jira issue REST endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraIssue {
    /// Issue key, e.g. `"PROJ-123"`.
    pub key: String,
    /// URL to the Jira issue (self link from the API).
    #[serde(rename = "self")]
    pub self_url: String,
    /// Nested field bag.
    pub fields: JiraIssueFields,
}

/// The `fields` object inside a Jira issue response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraIssueFields {
    /// One-line summary (title).
    pub summary: String,
    /// Rich-text description; may be null in the API response.
    pub description: Option<JiraDescription>,
    /// Current workflow status.
    pub status: JiraStatus,
    /// Priority level.
    pub priority: JiraPriority,
    /// Labels attached to the issue.
    #[serde(default)]
    pub labels: Vec<String>,
}

/// Jira description content node (simplified — we only need the plain text).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraDescription {
    /// Plain-text representation extracted from the Atlassian Document Format.
    /// In practice the server would flatten the ADF content; here we store
    /// the result as a simple string.
    pub text: Option<String>,
}

/// Jira workflow status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraStatus {
    /// Raw status name, e.g. `"In Progress"`, `"Done"`.
    pub name: String,
}

/// Jira priority level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraPriority {
    /// Raw priority name from Jira, e.g. `"Highest"`, `"Medium"`, `"Lowest"`.
    pub name: String,
}

// ---------------------------------------------------------------------------
// Priority mapping
// ---------------------------------------------------------------------------

/// Map a Jira priority name to an `ExternalPriority`.
///
/// Mapping:
/// - `"Highest"` / `"High"`  → `P0`
/// - `"Medium"`              → `P1`
/// - `"Low"`                 → `P2`
/// - `"Lowest"`              → `P3`
/// - anything else           → `Unknown(name)`
pub fn map_jira_priority(name: &str) -> ExternalPriority {
    match name {
        "Highest" | "High" => ExternalPriority::P0,
        "Medium" => ExternalPriority::P1,
        "Low" => ExternalPriority::P2,
        "Lowest" => ExternalPriority::P3,
        other => ExternalPriority::Unknown(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

impl From<JiraIssue> for ExternalItem {
    fn from(issue: JiraIssue) -> Self {
        let description = issue
            .fields
            .description
            .as_ref()
            .and_then(|d| d.text.clone());

        ExternalItem {
            id: issue.key.clone(),
            title: issue.fields.summary,
            description,
            status: issue.fields.status.name,
            priority: map_jira_priority(&issue.fields.priority.name),
            labels: issue.fields.labels,
            url: issue.self_url,
            source: "jira".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_issue(priority: &str) -> JiraIssue {
        JiraIssue {
            key: "PROJ-42".to_string(),
            self_url: "https://myorg.atlassian.net/rest/api/3/issue/PROJ-42".to_string(),
            fields: JiraIssueFields {
                summary: "Fix the thing".to_string(),
                description: Some(JiraDescription {
                    text: Some("Detailed description here.".to_string()),
                }),
                status: JiraStatus {
                    name: "In Progress".to_string(),
                },
                priority: JiraPriority {
                    name: priority.to_string(),
                },
                labels: vec!["backend".to_string(), "urgent".to_string()],
            },
        }
    }

    #[test]
    fn from_jira_issue_maps_fields_correctly() {
        let issue = make_issue("Medium");
        let item = ExternalItem::from(issue);

        assert_eq!(item.id, "PROJ-42");
        assert_eq!(item.title, "Fix the thing");
        assert_eq!(
            item.description.as_deref(),
            Some("Detailed description here.")
        );
        assert_eq!(item.status, "In Progress");
        assert_eq!(item.priority, ExternalPriority::P1);
        assert_eq!(item.labels, vec!["backend", "urgent"]);
        assert_eq!(item.source, "jira");
    }

    #[test]
    fn priority_highest_maps_to_p0() {
        let item = ExternalItem::from(make_issue("Highest"));
        assert_eq!(item.priority, ExternalPriority::P0);
    }

    #[test]
    fn priority_high_maps_to_p0() {
        let item = ExternalItem::from(make_issue("High"));
        assert_eq!(item.priority, ExternalPriority::P0);
    }

    #[test]
    fn priority_medium_maps_to_p1() {
        let item = ExternalItem::from(make_issue("Medium"));
        assert_eq!(item.priority, ExternalPriority::P1);
    }

    #[test]
    fn priority_low_maps_to_p2() {
        let item = ExternalItem::from(make_issue("Low"));
        assert_eq!(item.priority, ExternalPriority::P2);
    }

    #[test]
    fn priority_lowest_maps_to_p3() {
        let item = ExternalItem::from(make_issue("Lowest"));
        assert_eq!(item.priority, ExternalPriority::P3);
    }

    #[test]
    fn priority_unknown_preserved() {
        let item = ExternalItem::from(make_issue("Critical"));
        assert_eq!(
            item.priority,
            ExternalPriority::Unknown("Critical".to_string())
        );
    }

    #[test]
    fn null_description_becomes_none() {
        let mut issue = make_issue("Low");
        issue.fields.description = None;
        let item = ExternalItem::from(issue);
        assert!(item.description.is_none());
    }

    #[test]
    fn empty_labels_roundtrip() {
        let mut issue = make_issue("Low");
        issue.fields.labels = vec![];
        let item = ExternalItem::from(issue);
        assert!(item.labels.is_empty());
    }

    #[test]
    fn jira_issue_serde_roundtrip() {
        let issue = make_issue("High");
        let json = serde_json::to_string(&issue).expect("serialize");
        let back: JiraIssue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.key, "PROJ-42");
        assert_eq!(back.fields.priority.name, "High");
    }
}
