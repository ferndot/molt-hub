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
pub struct ServerSettings {
    pub appearance: AppearanceSettings,
    pub notifications: NotificationSettings,
    pub agent_defaults: AgentDefaultSettings,
    pub kanban_columns: Vec<KanbanColumn>,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            appearance: AppearanceSettings::default(),
            notifications: NotificationSettings::default(),
            agent_defaults: AgentDefaultSettings::default(),
            kanban_columns: default_kanban_columns(),
        }
    }
}

// ---------------------------------------------------------------------------
// Section: appearance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
}
