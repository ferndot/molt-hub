/**
 * Priority badge types and label mappings — pure data, no DOM/CSS imports.
 *
 * Separated from the component so tests can import without triggering
 * CSS module resolution or client-only SolidJS APIs.
 */

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export type PriorityLevel = "p0" | "p1" | "p2" | "p3";

export type BadgeSize = "sm" | "md" | "lg";

export interface PriorityBadgeProps {
  priority: PriorityLevel;
  size?: BadgeSize;
  /** Optional override for the accessible label */
  label?: string;
}

// ---------------------------------------------------------------------------
// Label map
// ---------------------------------------------------------------------------

export const PRIORITY_LABELS: Record<PriorityLevel, string> = {
  p0: "Critical",
  p1: "High",
  p2: "Medium",
  p3: "Low",
};

// ---------------------------------------------------------------------------
// All known values (for exhaustiveness checks)
// ---------------------------------------------------------------------------

export const ALL_PRIORITIES: readonly PriorityLevel[] = [
  "p0",
  "p1",
  "p2",
  "p3",
] as const;

export const ALL_BADGE_SIZES: readonly BadgeSize[] = [
  "sm",
  "md",
  "lg",
] as const;
