//! Event store — append-only log of domain events driving the system's state.
//!
//! `DomainEvent` is the sealed union of every event the system can produce.
//! All events share a common envelope (id, task_id, session_id, timestamp,
//! caused_by) with type-specific payload data in each variant.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use std::path::PathBuf;

use crate::model::{
    AgentId, EventId, Priority, ProjectId, SessionId, TaskId, TaskOutcome, TaskState,
};

// ---------------------------------------------------------------------------
// Shared event envelope fields
// ---------------------------------------------------------------------------
// Rather than embedding these in every variant, callers wrap a `DomainEvent`
// in `EventEnvelope` for storage.  This keeps variant payloads clean.

/// Metadata common to every persisted event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Unique identifier for this event instance.
    pub id: EventId,
    /// The task this event relates to (None for project-level events).
    pub task_id: Option<TaskId>,
    /// The project this event belongs to.
    pub project_id: String,
    /// The session in which this event occurred.
    pub session_id: SessionId,
    /// Wall-clock time the event was recorded.
    pub timestamp: DateTime<Utc>,
    /// The event that caused this one, if any (for causal chains).
    pub caused_by: Option<EventId>,
    /// The event payload.
    pub payload: DomainEvent,
}

// ---------------------------------------------------------------------------
// DomainEvent variants
// ---------------------------------------------------------------------------

/// The sealed union of all domain events.
///
/// Each variant carries only the fields specific to that event type.
/// Common envelope fields (id, task_id, session_id, timestamp, caused_by)
/// live in `EventEnvelope`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEvent {
    // ------------------------------------------------------------------
    // Task lifecycle
    // ------------------------------------------------------------------
    /// A new task was created in the system.
    TaskCreated {
        /// Human-readable title of the task.
        title: String,
        /// Full task description.
        description: String,
        /// Name of the initial stage the task enters.
        initial_stage: String,
        /// Initial priority assigned at creation.
        priority: Priority,
        /// The board this task belongs to (if any).
        #[serde(default)]
        board_id: Option<String>,
    },

    /// A task moved from one stage to another within its pipeline.
    TaskStageChanged {
        /// Stage the task moved from.
        from_stage: String,
        /// Stage the task moved to.
        to_stage: String,
        /// New state of the task after the transition.
        new_state: TaskState,
    },

    /// The priority of a task was changed.
    TaskPriorityChanged {
        /// Previous priority.
        from: Priority,
        /// New priority.
        to: Priority,
    },

    /// A task became blocked and requires intervention.
    TaskBlocked {
        /// Human-readable reason for the block.
        reason: String,
    },

    /// A previously blocked task has been unblocked.
    TaskUnblocked {
        /// Optional note explaining how the block was resolved.
        resolution: Option<String>,
    },

    /// A task reached its terminal state.
    TaskCompleted {
        /// How the task ended.
        outcome: TaskOutcome,
    },

    // ------------------------------------------------------------------
    // Agent lifecycle
    // ------------------------------------------------------------------
    /// An agent was assigned to work on the task.
    AgentAssigned {
        /// The agent that was assigned.
        agent_id: AgentId,
        /// Human-readable agent name for diagnostics.
        agent_name: String,
    },

    /// An agent produced output (log, partial result, status update).
    AgentOutput {
        /// The agent that produced the output.
        agent_id: AgentId,
        /// Raw output text from the agent.
        output: String,
        /// Optional turn identifier for grouping output lines into turns.
        #[serde(default)]
        turn_id: Option<uuid::Uuid>,
    },

    /// An agent finished its work on the task.
    AgentCompleted {
        /// The agent that completed.
        agent_id: AgentId,
        /// Summary of what the agent produced, if any.
        summary: Option<String>,
    },

    // ------------------------------------------------------------------
    // Human-in-the-loop
    // ------------------------------------------------------------------
    /// A human reviewer made a decision on the task (approve / reject / redirect).
    HumanDecision {
        /// The account or name of the human who decided.
        decided_by: String,
        /// The decision taken.
        decision: HumanDecisionKind,
        /// Optional note accompanying the decision.
        note: Option<String>,
    },

    // ------------------------------------------------------------------
    // Integration events
    // ------------------------------------------------------------------
    /// A task was imported from an external system (e.g. Jira, GitHub).
    TaskImported {
        /// Integration source identifier, e.g. `"jira"` or `"github"`.
        source: String,
        /// External system's identifier for the item (e.g. `"PROJ-42"`).
        external_id: String,
        /// Direct URL to the item in the external system's UI.
        external_url: String,
    },

    /// An integration was configured (or reconfigured) for a project scope.
    IntegrationConfigured {
        /// The integration type, e.g. `"jira"`, `"github"`, `"webhook"`.
        integration_type: String,
        /// Scope this configuration applies to, e.g. a project key or repo slug.
        project_scope: String,
    },

    // ------------------------------------------------------------------
    // Project lifecycle
    // ------------------------------------------------------------------
    /// A new project was created.
    ProjectCreated {
        /// The project's unique identifier.
        project_id: ProjectId,
        /// Human-readable project name.
        name: String,
        /// Path to the repository on disk.
        repo_path: PathBuf,
    },

    /// A project was archived (soft-deleted).
    ProjectArchived {
        /// The project that was archived.
        project_id: ProjectId,
    },

    /// A project's metadata was updated.
    ProjectUpdated {
        /// The project that was updated.
        project_id: ProjectId,
        /// The new name for the project.
        name: String,
    },
}

/// The specific decision a human reviewer made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HumanDecisionKind {
    /// Task is approved to continue to the next stage.
    Approved,
    /// Task is rejected; work must be revised or abandoned.
    Rejected { reason: String },
    /// Task is redirected to a different stage.
    Redirected { to_stage: String, reason: String },
}
