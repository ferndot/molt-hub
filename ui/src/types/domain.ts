/**
 * Domain types mirroring the Rust model in crates/core/src/model.rs and
 * crates/core/src/events/types.rs.
 *
 * IDs are ULID strings on the wire.
 */

// ---------------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------------

export type TaskId = string;
export type AgentId = string;
export type PipelineId = string;
export type ProjectId = string;
export type SessionId = string;
export type EventId = string;

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

/** Four-level interrupt classification — rendered as 2 tiers in v0 UI. */
export type Priority = "p0" | "p1" | "p2" | "p3";

// ---------------------------------------------------------------------------
// TaskState
// ---------------------------------------------------------------------------

export type TaskOutcome =
  | { type: "success" }
  | { type: "rejected"; reason: string }
  | { type: "abandoned"; reason: string };

export type TaskState =
  | { type: "pending" }
  | { type: "in_progress" }
  | { type: "blocked"; reason: string; blocked_at: string }
  | { type: "awaiting_approval"; approvers: string[]; approved_by: string[] }
  | { type: "completed"; outcome: TaskOutcome }
  | { type: "failed"; error: string };

// ---------------------------------------------------------------------------
// AgentStatus
// ---------------------------------------------------------------------------

export type AgentStatus =
  | { type: "idle" }
  | { type: "running" }
  | { type: "paused" }
  | { type: "terminated" }
  | { type: "crashed"; error: string }
  | { type: "completed" }
  | { type: "failed" };

// ---------------------------------------------------------------------------
// Core entities
// ---------------------------------------------------------------------------

export interface Task {
  id: TaskId;
  pipeline_id: PipelineId;
  title: string;
  description: string;
  current_stage: string;
  state: TaskState;
  priority: Priority;
  assigned_agent: AgentId | null;
  session_id: SessionId;
  created_at: string;
  updated_at: string;
}

export interface Agent {
  id: AgentId;
  name: string;
  adapter_type: string;
  status: AgentStatus;
  task_id: TaskId | null;
  session_id: SessionId;
  started_at: string;
  last_activity_at: string;
}

export interface StageConfig {
  name: string;
  instructions_template: string | null;
  requires_approval: boolean;
  timeout: number | null;
}

export interface Pipeline {
  id: PipelineId;
  project_id: ProjectId;
  name: string;
  stages: StageConfig[];
  version: number;
  created_at: string;
  updated_at: string;
}

// ---------------------------------------------------------------------------
// Domain events (wire format from server)
// ---------------------------------------------------------------------------

export type HumanDecisionKind =
  | { kind: "approved" }
  | { kind: "rejected"; reason: string }
  | { kind: "redirected"; to_stage: string; reason: string };

export type DomainEvent =
  | {
      type: "task_created";
      title: string;
      description: string;
      initial_stage: string;
      priority: Priority;
    }
  | {
      type: "task_stage_changed";
      from_stage: string;
      to_stage: string;
      new_state: TaskState;
    }
  | { type: "task_priority_changed"; from: Priority; to: Priority }
  | { type: "task_blocked"; reason: string }
  | { type: "task_unblocked"; resolution: string | null }
  | { type: "task_completed"; outcome: TaskOutcome }
  | { type: "agent_assigned"; agent_id: AgentId; agent_name: string }
  | { type: "agent_output"; agent_id: AgentId; output: string }
  | { type: "agent_completed"; agent_id: AgentId; summary: string | null }
  | {
      type: "human_decision";
      decided_by: string;
      decision: HumanDecisionKind;
      note: string | null;
    };

export interface EventEnvelope {
  id: EventId;
  task_id: TaskId;
  session_id: SessionId;
  timestamp: string;
  caused_by: EventId | null;
  payload: DomainEvent;
}
