/**
 * PriorityBadge — colorblind-safe priority indicators.
 *
 * Each priority level is encoded with BOTH color AND a distinctive icon
 * so the indicator is distinguishable without relying on color alone.
 *
 * In colorblind mode (.colorblind on root), CSS background patterns
 * provide an additional redundant visual cue.
 *
 * Icons per priority:
 *  - P0 (Critical): "!" exclamation
 *  - P1 (High): filled triangle "▲"
 *  - P2 (Medium): filled square "■"
 *  - P3 (Low): hollow circle "○"
 */

import type { Component } from "solid-js";
import styles from "./PriorityBadge.module.css";
import { PRIORITY_LABELS } from "./priorityTypes";
import type { PriorityLevel, BadgeSize, PriorityBadgeProps } from "./priorityTypes";

// Re-export types and labels for convenience
export { PRIORITY_LABELS };
export type { PriorityLevel, BadgeSize, PriorityBadgeProps };

// ---------------------------------------------------------------------------
// Icon map — distinctive shape per priority
// ---------------------------------------------------------------------------

const PRIORITY_ICON: Record<PriorityLevel, string> = {
  p0: "!",
  p1: "\u25B2", // ▲
  p2: "\u25A0", // ■
  p3: "\u25CB", // ○
};

// ---------------------------------------------------------------------------
// CSS class map
// ---------------------------------------------------------------------------

const PRIORITY_CLASS: Record<PriorityLevel, string> = {
  p0: styles.p0,
  p1: styles.p1,
  p2: styles.p2,
  p3: styles.p3,
};

const SIZE_CLASS: Record<BadgeSize, string> = {
  sm: styles.sm,
  md: styles.md,
  lg: styles.lg,
};

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const PriorityBadge: Component<PriorityBadgeProps> = (props) => {
  const size = () => props.size ?? "md";
  const ariaLabel = () =>
    props.label ?? `Priority: ${PRIORITY_LABELS[props.priority]}`;

  return (
    <span
      class={`${styles.badge} ${PRIORITY_CLASS[props.priority]} ${SIZE_CLASS[size()]}`}
      role="img"
      aria-label={ariaLabel()}
    >
      <span class={styles.icon} aria-hidden="true">
        {PRIORITY_ICON[props.priority]}
      </span>
      {props.priority.toUpperCase()}
    </span>
  );
};

export default PriorityBadge;
