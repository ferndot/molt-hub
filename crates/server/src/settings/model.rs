//! Typed settings model for the Molt Hub server.
//!
//! Persisted as a single JSON file at `~/.config/molt-hub/settings.json`.
//! Each top-level field corresponds to a UI section that can be patched
//! independently via `PATCH /api/settings/:section`.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Top-level settings
// ---------------------------------------------------------------------------

/// Complete server settings, serialised as a single JSON document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettings {
    pub appearance: AppearanceSettings,
    pub notifications: NotificationSettings,
    pub agent_defaults: AgentDefaultSettings,
    #[serde(default = "default_kanban_columns")]
    pub kanban_columns: Vec<KanbanColumn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_widths: Option<SidebarWidths>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jira_config: Option<JiraConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_config: Option<GithubConfig>,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            appearance: AppearanceSettings::default(),
            notifications: NotificationSettings::default(),
            agent_defaults: AgentDefaultSettings::default(),
            kanban_columns: default_kanban_columns(),
            sidebar_widths: None,
            jira_config: None,
            github_config: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Section: appearance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettings {
    pub theme: String,
    pub colorblind_mode: bool,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: "system".to_owned(),
            colorblind_mode: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Section: notifications
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettings {
    pub attention_level: String,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            attention_level: "p0p1".to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// Section: agent_defaults
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefaultSettings {
    pub timeout_minutes: u32,
    pub adapter: String,
}

impl Default for AgentDefaultSettings {
    fn default() -> Self {
        Self {
            timeout_minutes: 30,
            adapter: "claude-code".to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// Section: kanban_columns
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KanbanColumn {
    pub id: String,
    pub title: String,
    pub stage_match: String,
    pub color: Option<String>,
    pub order: u32,
    pub wip_limit: Option<u32>,
    pub require_approval: bool,
}

/// Sensible default kanban columns matching common development workflows.
fn default_kanban_columns() -> Vec<KanbanColumn> {
    vec![
        KanbanColumn {
            id: "backlog".to_owned(),
            title: "Backlog".to_owned(),
            stage_match: "backlog".to_owned(),
            color: None,
            order: 0,
            wip_limit: None,
            require_approval: false,
        },
        KanbanColumn {
            id: "in-progress".to_owned(),
            title: "In Progress".to_owned(),
            stage_match: "in_progress".to_owned(),
            color: None,
            order: 1,
            wip_limit: Some(5),
            require_approval: false,
        },
        KanbanColumn {
            id: "review".to_owned(),
            title: "Review".to_owned(),
            stage_match: "review".to_owned(),
            color: None,
            order: 2,
            wip_limit: Some(3),
            require_approval: true,
        },
        KanbanColumn {
            id: "done".to_owned(),
            title: "Done".to_owned(),
            stage_match: "done".to_owned(),
            color: None,
            order: 3,
            wip_limit: None,
            require_approval: false,
        },
    ]
}

// ---------------------------------------------------------------------------
// Section: sidebar_widths
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SidebarWidths {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nav_sidebar: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox_sidebar: Option<u32>,
}

// ---------------------------------------------------------------------------
// Section: jira_config
// ---------------------------------------------------------------------------

/// Jira integration config (public metadata only — no tokens or secrets).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JiraConfig {
    #[serde(default)]
    pub connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Section: github_config
// ---------------------------------------------------------------------------

/// GitHub integration config (public metadata only — no tokens).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GithubConfig {
    #[serde(default)]
    pub connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_serialises_to_json() {
        let s = ServerSettings::default();
        let json = serde_json::to_string_pretty(&s).unwrap();
        assert!(json.contains("system"));
        assert!(json.contains("claude-code"));
    }

    #[test]
    fn round_trip_through_json() {
        let original = ServerSettings::default();
        let json = serde_json::to_string(&original).unwrap();
        let restored: ServerSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn default_has_four_kanban_columns() {
        let s = ServerSettings::default();
        assert_eq!(s.kanban_columns.len(), 4);
    }

    #[test]
    fn default_new_sections_are_none() {
        let s = ServerSettings::default();
        assert!(s.sidebar_widths.is_none());
        assert!(s.jira_config.is_none());
        assert!(s.github_config.is_none());
    }

    #[test]
    fn sidebar_widths_round_trip_camel_case() {
        let w = SidebarWidths {
            nav_sidebar: Some(280),
            inbox_sidebar: Some(320),
        };
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("navSidebar"));
        assert!(json.contains("inboxSidebar"));
        let restored: SidebarWidths = serde_json::from_str(&json).unwrap();
        assert_eq!(w, restored);
    }

    #[test]
    fn jira_config_round_trip_camel_case() {
        let jc = JiraConfig {
            connected: true,
            base_url: Some("https://test.atlassian.net".to_owned()),
            site_name: Some("Test".to_owned()),
            cloud_id: Some("abc-123".to_owned()),
        };
        let json = serde_json::to_string(&jc).unwrap();
        assert!(json.contains("baseUrl"));
        assert!(json.contains("siteName"));
        assert!(json.contains("cloudId"));
        let restored: JiraConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(jc, restored);
    }

    #[test]
    fn github_config_round_trip_camel_case() {
        let gc = GithubConfig {
            connected: true,
            owner: Some("my-org".to_owned()),
        };
        let json = serde_json::to_string(&gc).unwrap();
        let restored: GithubConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(gc, restored);
    }

    #[test]
    fn settings_with_new_sections_round_trip() {
        let mut s = ServerSettings::default();
        s.sidebar_widths = Some(SidebarWidths {
            nav_sidebar: Some(260),
            inbox_sidebar: None,
        });
        s.jira_config = Some(JiraConfig {
            connected: false,
            base_url: None,
            site_name: None,
            cloud_id: None,
        });
        s.github_config = Some(GithubConfig {
            connected: true,
            owner: Some("octocat".to_owned()),
        });
        let json = serde_json::to_string(&s).unwrap();
        let restored: ServerSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, restored);
    }

    #[test]
    fn deserialise_without_new_sections_uses_none() {
        // JSON without optional sections
        let json = serde_json::json!({
            "appearance": { "theme": "system", "colorblindMode": false },
            "notifications": { "attentionLevel": "p0p1" },
            "agentDefaults": { "timeoutMinutes": 30, "adapter": "claude-code" },
            "kanban_columns": []
        });
        let s: ServerSettings = serde_json::from_value(json).unwrap();
        assert!(s.sidebar_widths.is_none());
        assert!(s.jira_config.is_none());
        assert!(s.github_config.is_none());
    }
}
