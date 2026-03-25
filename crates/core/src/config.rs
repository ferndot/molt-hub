//! Stage configuration — schema and validation for pipeline stage definitions.
//!
//! This module defines the YAML input schema for pipeline configuration.
//! These types are distinct from the runtime model types in `model.rs`.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

use crate::integrations::config::IntegrationConfig;

// ─── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Error)]
pub enum ConfigError {
    #[error("parse error: {0}")]
    Parse(String),

    #[error("stage '{stage}': {message}")]
    StageError { stage: String, message: String },

    #[error("duplicate stage name: '{0}'")]
    DuplicateStage(String),

    #[error("unknown stage reference: '{0}'")]
    UnknownStageRef(String),

    #[error("pipeline must contain at least one stage")]
    NoStages,

    #[error("warning: {0}")]
    Warning(String),
}

// ─── Hook types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    Shell,
    StartDevEnvironment,
    TeardownDevEnvironment,
    AgentDispatch,
    Webhook,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookTrigger {
    Enter,
    Exit,
    OnStall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookDefinition {
    pub kind: HookKind,
    pub on: HookTrigger,
    #[serde(flatten)]
    pub config: serde_json::Value,
}

// ─── Transition types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransitionTrigger {
    AgentCompleted,
    Approved,
    Rejected,
    Timeout,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransitionDefinition {
    pub when: TransitionTrigger,
    pub then: String,
    pub guard: Option<serde_json::Value>,
}

// ─── Stage definition ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageDefinition {
    /// Stable stage identifier (`id` is accepted as a YAML/JSON alias).
    #[serde(alias = "id")]
    pub name: String,
    /// Display label for the board; when omitted, APIs default to `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub instructions: Option<String>,
    pub instructions_template: Option<String>,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub approvers: Vec<String>,
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default)]
    pub hooks: Vec<HookDefinition>,
    #[serde(default)]
    pub transition_rules: Vec<TransitionDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub order: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wip_limit: Option<u32>,
}

// ─── Column types ─────────────────────────────────────────────────────────────

/// WIP limits, auto-assignment, and gating behavior for a kanban column.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ColumnBehavior {
    /// Maximum number of tasks allowed in the column at once.
    pub wip_limit: Option<u32>,
    /// Automatically assign an agent when a task enters this column.
    #[serde(default)]
    pub auto_assign: bool,
    /// Automatically move tasks to this stage name when a condition is met.
    pub auto_transition: Option<String>,
    /// Require human approval before a task is allowed to leave this column.
    #[serde(default)]
    pub require_approval: bool,
}

/// Hook IDs that fire when tasks enter, exit, or stall in a column.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ColumnHooks {
    /// Hook IDs to run when a task enters this column.
    #[serde(default)]
    pub on_enter: Vec<String>,
    /// Hook IDs to run when a task leaves this column.
    #[serde(default)]
    pub on_exit: Vec<String>,
    /// Hook IDs to run when a task stalls (exceeds inactivity threshold) in this column.
    #[serde(default)]
    pub on_stall: Vec<String>,
}

/// A kanban column definition, including display settings, stage mapping, and behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnConfig {
    /// Stable identifier for this column, used in hook references and API calls.
    pub id: String,
    /// Human-readable column heading shown in the UI.
    pub title: String,
    /// Stage name patterns (exact or glob) that this column renders.
    #[serde(default)]
    pub stage_match: Vec<String>,
    /// Optional hex color code for the column header, e.g. `"#FF5733"`.
    pub color: Option<String>,
    /// Display order among sibling columns (lower = further left).
    pub order: u32,
    /// WIP and gating behavior.
    #[serde(default)]
    pub behavior: ColumnBehavior,
    /// Lifecycle hooks for this column.
    #[serde(default)]
    pub hooks: ColumnHooks,
}

// ─── Integration section ───────────────────────────────────────────────────────

/// An entry in the `integrations` section of a pipeline config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationEntry {
    /// Human-readable label for this integration entry.
    pub name: String,
    /// The typed connection settings.
    #[serde(flatten)]
    pub config: IntegrationConfig,
}

// ─── Pipeline config (top-level) ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub name: String,
    pub description: Option<String>,
    pub version: u32,
    pub stages: Vec<StageDefinition>,
    /// Optional external integrations configured for this pipeline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integrations: Vec<IntegrationEntry>,
    /// Kanban column definitions for this pipeline's board view.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ColumnConfig>,
}

impl PipelineConfig {
    /// Default pipeline matching the built-in board stages (backlog → deployed).
    pub fn board_defaults() -> Self {
        Self {
            name: "default".into(),
            description: None,
            version: 1,
            stages: vec![
                StageDefinition {
                    name: "backlog".into(),
                    label: Some("Backlog".into()),
                    instructions: None,
                    instructions_template: None,
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: false,
                    hooks: vec![],
                    transition_rules: vec![],
                    color: Some("#94a3b8".into()),
                    order: 0,
                    wip_limit: None,
                },
                StageDefinition {
                    name: "planning".into(),
                    label: Some("Planning".into()),
                    instructions: None,
                    instructions_template: None,
                    requires_approval: true,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: false,
                    hooks: vec![],
                    transition_rules: vec![],
                    color: Some("#a855f7".into()),
                    order: 1,
                    wip_limit: None,
                },
                StageDefinition {
                    name: "in-progress".into(),
                    label: Some("In Progress".into()),
                    instructions: None,
                    instructions_template: None,
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: false,
                    hooks: vec![],
                    transition_rules: vec![],
                    color: Some("#6366f1".into()),
                    order: 2,
                    wip_limit: Some(5),
                },
                StageDefinition {
                    name: "review".into(),
                    label: Some("Review".into()),
                    instructions: None,
                    instructions_template: None,
                    requires_approval: true,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: false,
                    hooks: vec![],
                    transition_rules: vec![],
                    color: Some("#f59e0b".into()),
                    order: 3,
                    wip_limit: None,
                },
                StageDefinition {
                    name: "testing".into(),
                    label: Some("Testing".into()),
                    instructions: None,
                    instructions_template: None,
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: false,
                    hooks: vec![],
                    transition_rules: vec![],
                    color: Some("#10b981".into()),
                    order: 4,
                    wip_limit: None,
                },
                StageDefinition {
                    name: "deployment".into(),
                    label: Some("Deployment".into()),
                    instructions: None,
                    instructions_template: None,
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: true,
                    hooks: vec![],
                    transition_rules: vec![],
                    color: Some("#22c55e".into()),
                    order: 5,
                    wip_limit: None,
                },
            ],
            integrations: vec![],
            columns: vec![
                ColumnConfig {
                    id: "backlog".into(),
                    title: "Backlog".into(),
                    stage_match: vec!["backlog".into()],
                    color: Some("#94a3b8".into()),
                    order: 0,
                    behavior: ColumnBehavior {
                        wip_limit: None,
                        ..Default::default()
                    },
                    hooks: ColumnHooks::default(),
                },
                ColumnConfig {
                    id: "planning".into(),
                    title: "Planning".into(),
                    stage_match: vec!["planning".into()],
                    color: Some("#a855f7".into()),
                    order: 1,
                    behavior: ColumnBehavior::default(),
                    hooks: ColumnHooks::default(),
                },
                ColumnConfig {
                    id: "in-progress".into(),
                    title: "In Progress".into(),
                    stage_match: vec!["in-progress".into()],
                    color: Some("#6366f1".into()),
                    order: 2,
                    behavior: ColumnBehavior {
                        wip_limit: Some(5),
                        ..Default::default()
                    },
                    hooks: ColumnHooks::default(),
                },
                ColumnConfig {
                    id: "review".into(),
                    title: "Review".into(),
                    stage_match: vec!["review".into()],
                    color: Some("#f59e0b".into()),
                    order: 3,
                    behavior: ColumnBehavior::default(),
                    hooks: ColumnHooks::default(),
                },
                ColumnConfig {
                    id: "testing".into(),
                    title: "Testing".into(),
                    stage_match: vec!["testing".into()],
                    color: Some("#10b981".into()),
                    order: 4,
                    behavior: ColumnBehavior::default(),
                    hooks: ColumnHooks::default(),
                },
                ColumnConfig {
                    id: "deployment".into(),
                    title: "Deployment".into(),
                    stage_match: vec!["deployment".into()],
                    color: Some("#22c55e".into()),
                    order: 5,
                    behavior: ColumnBehavior::default(),
                    hooks: ColumnHooks::default(),
                },
            ],
        }
    }

    /// Parse a YAML string into a `PipelineConfig`.
    pub fn from_yaml(yaml: &str) -> Result<Self, ConfigError> {
        serde_yaml::from_str(yaml).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Validate the config and return any errors/warnings.
    /// An empty vec means the config is valid.
    pub fn validate(&self) -> Vec<ConfigError> {
        let mut errors: Vec<ConfigError> = Vec::new();

        // Rule: at least one stage
        if self.stages.is_empty() {
            errors.push(ConfigError::NoStages);
            // Nothing else to check without stages
            return errors;
        }

        // Collect stage names for reference checks
        let stage_names: HashSet<&str> = self.stages.iter().map(|s| s.name.as_str()).collect();

        // Rule: unique stage names
        {
            let mut seen: HashSet<&str> = HashSet::new();
            for stage in &self.stages {
                if !seen.insert(stage.name.as_str()) {
                    errors.push(ConfigError::DuplicateStage(stage.name.clone()));
                }
            }
        }

        // Rule: at most one terminal stage
        let terminal_count = self.stages.iter().filter(|s| s.terminal).count();
        if terminal_count > 1 {
            errors.push(ConfigError::Warning(format!(
                "pipeline has {terminal_count} terminal stages; at most one is expected"
            )));
        }

        // Per-stage validation
        for stage in &self.stages {
            // Rule: instructions and instructions_template are mutually exclusive
            if stage.instructions.is_some() && stage.instructions_template.is_some() {
                errors.push(ConfigError::StageError {
                    stage: stage.name.clone(),
                    message: "instructions and instructions_template are mutually exclusive"
                        .to_string(),
                });
            }

            // Rule: transition targets must reference known stage names
            for rule in &stage.transition_rules {
                if !stage_names.contains(rule.then.as_str()) {
                    errors.push(ConfigError::UnknownStageRef(rule.then.clone()));
                }
            }
        }

        errors
    }

    /// Parse YAML and validate in one step.
    /// Returns `Ok(config)` only when there are no errors (warnings are treated as errors here).
    pub fn load_and_validate(yaml: &str) -> Result<Self, Vec<ConfigError>> {
        let config = Self::from_yaml(yaml).map_err(|e| vec![e])?;
        let errors = config.validate();
        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_yaml() -> &'static str {
        r#"
name: My Pipeline
version: 1
stages:
  - name: review
    requires_approval: true
    approvers:
      - alice
  - name: done
    terminal: true
    transition_rules:
      - when: approved
        then: done
"#
    }

    #[test]
    fn parse_minimal_config() {
        let cfg = PipelineConfig::from_yaml(minimal_yaml()).expect("should parse");
        assert_eq!(cfg.name, "My Pipeline");
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.stages.len(), 2);
        assert!(cfg.stages[0].requires_approval);
        assert!(cfg.stages[1].terminal);
    }

    #[test]
    fn validate_valid_config() {
        let cfg = PipelineConfig::from_yaml(minimal_yaml()).unwrap();
        let errs = cfg.validate();
        // The transition `then: done` is a known stage, so only possible issue would be
        // the approved transition pointing to "done" which exists — no errors expected.
        // (Warnings count as errors in load_and_validate, but validate() returns them raw.)
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    }

    #[test]
    fn error_on_no_stages() {
        let yaml = "name: empty\nversion: 1\nstages: []\n";
        let cfg = PipelineConfig::from_yaml(yaml).unwrap();
        let errs = cfg.validate();
        assert!(errs.iter().any(|e| matches!(e, ConfigError::NoStages)));
    }

    #[test]
    fn error_on_duplicate_stage() {
        let yaml = r#"
name: dup
version: 1
stages:
  - name: review
  - name: review
"#;
        let cfg = PipelineConfig::from_yaml(yaml).unwrap();
        let errs = cfg.validate();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ConfigError::DuplicateStage(n) if n == "review")));
    }

    #[test]
    fn error_on_unknown_stage_ref() {
        let yaml = r#"
name: bad-ref
version: 1
stages:
  - name: start
    transition_rules:
      - when: agent_completed
        then: nonexistent
"#;
        let cfg = PipelineConfig::from_yaml(yaml).unwrap();
        let errs = cfg.validate();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ConfigError::UnknownStageRef(n) if n == "nonexistent")));
    }

    #[test]
    fn error_on_mutually_exclusive_instructions() {
        let yaml = r#"
name: both-instructions
version: 1
stages:
  - name: work
    instructions: "Do the thing."
    instructions_template: "tmpl/work.md"
"#;
        let cfg = PipelineConfig::from_yaml(yaml).unwrap();
        let errs = cfg.validate();
        assert!(errs.iter().any(|e| matches!(
            e,
            ConfigError::StageError { stage, .. } if stage == "work"
        )));
    }

    #[test]
    fn warning_on_multiple_terminal_stages() {
        let yaml = r#"
name: multi-terminal
version: 1
stages:
  - name: done1
    terminal: true
  - name: done2
    terminal: true
"#;
        let cfg = PipelineConfig::from_yaml(yaml).unwrap();
        let errs = cfg.validate();
        assert!(errs.iter().any(|e| matches!(e, ConfigError::Warning(_))));
    }

    #[test]
    fn load_and_validate_returns_err_on_invalid() {
        let yaml = "name: empty\nversion: 1\nstages: []\n";
        let result = PipelineConfig::load_and_validate(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn hook_definition_round_trips() {
        let yaml = r#"
name: hooked
version: 1
stages:
  - name: build
    hooks:
      - kind: shell
        on: enter
        command: "make build"
"#;
        let cfg = PipelineConfig::from_yaml(yaml).unwrap();
        let hook = &cfg.stages[0].hooks[0];
        assert_eq!(hook.kind, HookKind::Shell);
        assert_eq!(hook.on, HookTrigger::Enter);
        assert_eq!(hook.config["command"], "make build");
    }

    #[test]
    fn stage_accepts_id_alias_and_label_in_yaml() {
        let yaml = r#"
name: aliased
version: 1
stages:
  - id: build
    label: Build
"#;
        let cfg = PipelineConfig::from_yaml(yaml).unwrap();
        assert_eq!(cfg.stages[0].name, "build");
        assert_eq!(cfg.stages[0].label.as_deref(), Some("Build"));
    }
}
