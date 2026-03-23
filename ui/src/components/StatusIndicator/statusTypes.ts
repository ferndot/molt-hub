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
 */
export type IndicatorStatus =
  | "pending"
  | "in_progress"
  | "blocked"
  | "awaiting_approval"
  | "success"
  | "failure";

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
] as const;

export const ALL_SIZES: readonly IndicatorSize[] = [
  "sm",
  "md",
  "lg",
] as const;
