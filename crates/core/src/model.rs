//! Domain types — the core entities and value objects of Molt Hub.
//!
//! This module defines all persistent entities and value types used throughout the system.
//! These are pure data definitions; no I/O, no persistence logic, no transition logic.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use ulid::Ulid;

// ---------------------------------------------------------------------------
// ID newtypes
// ---------------------------------------------------------------------------

/// A strongly-typed ULID wrapper.
macro_rules! newtype_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub Ulid);

        impl $name {
            /// Generate a new random ID.
            pub fn new() -> Self {
                Self(Ulid::new())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

newtype_id!(ProjectId);
newtype_id!(PipelineId);
newtype_id!(TaskId);
newtype_id!(AgentId);
newtype_id!(SessionId);
newtype_id!(EventId);

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

/// Four-level interrupt classification for tasks.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// P0 — Critical, immediate human attention required.
    P0,
    /// P1 — Important, surface in next triage batch.
    P1,
    /// P2 — Normal, passive dashboard.
    P2,
    /// P3 — Low, background.
    P3,
}

// ---------------------------------------------------------------------------
// TaskState & TaskOutcome
// ---------------------------------------------------------------------------

/// Outcome of a completed task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskOutcome {
    /// Task succeeded.
    Success,
    /// Task was reviewed and rejected by an approver.
    Rejected { reason: String },
    /// Task was abandoned without completion.
    Abandoned { reason: String },
}

/// The state of a task as it flows through the pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskState {
    /// Waiting to be picked up.
    Pending,
    /// An agent is actively working on this task.
    InProgress,
    /// Task is blocked and cannot proceed without intervention.
    Blocked {
        reason: String,
        blocked_at: DateTime<Utc>,
    },
    /// Task is waiting for explicit human sign-off.
    AwaitingApproval {
        approvers: Vec<String>,
        approved_by: Vec<String>,
    },
    /// Task has finished.
    Completed { outcome: TaskOutcome },
    /// Task failed due to an error.
    Failed { error: String },
}

// ---------------------------------------------------------------------------
// AgentStatus
// ---------------------------------------------------------------------------

/// Lifecycle state of a running agent process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentStatus {
    /// Agent is connected but not assigned to a task.
    Idle,
    /// Agent is executing a task.
    Running,
    /// Agent execution has been suspended.
    Paused,
    /// Agent has shut down normally.
    Terminated,
    /// Agent exited unexpectedly.
    Crashed { error: String },
}

// ---------------------------------------------------------------------------
// Placeholder config/rule types (fleshed out in T14/T17)
// ---------------------------------------------------------------------------

/// Placeholder for a lifecycle hook attached to a stage.
/// Full definition deferred to T14.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    /// Arbitrary hook name for identification.
    pub name: String,
    /// Raw hook definition — schema TBD in T14.
    pub config: serde_json::Value,
}

/// Placeholder for a stage-level transition rule.
/// Full definition deferred to T17.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionRule {
    /// Arbitrary rule name for identification.
    pub name: String,
    /// Raw rule definition — schema TBD in T17.
    pub config: serde_json::Value,
}

// ---------------------------------------------------------------------------
// StageConfig
// ---------------------------------------------------------------------------

/// A user-defined stage within a pipeline.
///
/// Stages are plain strings, not Rust enum variants, so users can define
/// arbitrary workflows without recompiling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageConfig {
    /// Unique name within the pipeline (e.g. "planning", "implementation", "review").
    pub name: String,
    /// Optional Handlebars template reference for agent instructions.
    pub instructions_template: Option<String>,
    /// Lifecycle hooks attached to this stage.
    pub hooks: Vec<HookConfig>,
    /// Automated transition rules evaluated when an event occurs in this stage.
    pub transition_rules: Vec<TransitionRule>,
    /// If true, the task must pass human approval before leaving this stage.
    pub requires_approval: bool,
    /// Optional wall-clock timeout for the stage.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "opt_duration_secs"
    )]
    pub timeout: Option<Duration>,
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

/// Top-level entity grouping pipelines and tasks for a repository or project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// An ordered sequence of stages that tasks flow through.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: PipelineId,
    pub project_id: ProjectId,
    pub name: String,
    /// Ordered list of stages in this pipeline.
    pub stages: Vec<StageConfig>,
    /// Monotonically increasing version for optimistic concurrency / schema evolution.
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

/// A unit of work moving through pipeline stages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub pipeline_id: PipelineId,
    pub title: String,
    pub description: String,
    /// The `StageConfig.name` of the stage this task currently occupies.
    pub current_stage: String,
    pub state: TaskState,
    pub priority: Priority,
    /// The agent currently responsible for this task, if any.
    pub assigned_agent: Option<AgentId>,
    pub session_id: SessionId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

/// A running agent process managed by the harness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: AgentId,
    pub name: String,
    /// Identifies the adapter implementation (e.g. "claude-sdk", "cli", "acpx").
    pub adapter_type: String,
    pub status: AgentStatus,
    /// The task this agent is currently working on, if any.
    pub task_id: Option<TaskId>,
    pub session_id: SessionId,
    pub started_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// Groups a set of related agent activities within a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub project_id: ProjectId,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Serde helper: Duration as seconds (u64)
// ---------------------------------------------------------------------------

mod opt_duration_secs {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(d) => serializer.serialize_some(&d.as_secs()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs: Option<u64> = Option::deserialize(deserializer)?;
        Ok(secs.map(Duration::from_secs))
    }
}
