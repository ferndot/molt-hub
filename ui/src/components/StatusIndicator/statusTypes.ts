/**
 * Status indicator types and label mappings — pure data, no DOM/CSS imports.
 *
 * Separated from the component so tests can import without triggering
 * CSS module resolution or client-only SolidJS APIs.
 */

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/**
 * The status variants this component supports.
 *
 * Maps closely to TaskState from domain.ts but uses flat strings so the
 * component is reusable across different domain objects.
 *
 * Includes both task-pipeline statuses (pending, in_progress, etc.)
 * and agent-lifecycle statuses (running, paused, idle, terminated).
 */
export type IndicatorStatus =
  | "pending"
  | "in_progress"
  | "blocked"
  | "awaiting_approval"
  | "success"
  | "failure"
  | "running"
  | "paused"
  | "completed"
  | "failed"
  | "idle"
  | "terminated";

export type IndicatorSize = "sm" | "md" | "lg";

export interface StatusIndicatorProps {
  status: IndicatorStatus;
  size?: IndicatorSize;
  /** Optional override for the accessible label */
  label?: string;
}

// ---------------------------------------------------------------------------
// Label map
// ---------------------------------------------------------------------------

export const STATUS_LABELS: Record<IndicatorStatus, string> = {
  pending: "Pending",
  in_progress: "In Progress",
  blocked: "Blocked",
  awaiting_approval: "Awaiting Approval",
  success: "Completed — Success",
  failure: "Completed — Failure",
  running: "Running",
  paused: "Paused",
  completed: "Completed",
  failed: "Failed",
  idle: "Idle",
  terminated: "Terminated",
};

// ---------------------------------------------------------------------------
// All known status values (for exhaustiveness checks)
// ---------------------------------------------------------------------------

export const ALL_STATUSES: readonly IndicatorStatus[] = [
  "pending",
  "in_progress",
  "blocked",
  "awaiting_approval",
  "success",
  "failure",
  "running",
  "paused",
  "completed",
  "failed",
  "idle",
  "terminated",
] as const;

export const ALL_SIZES: readonly IndicatorSize[] = [
  "sm",
  "md",
  "lg",
] as const;
