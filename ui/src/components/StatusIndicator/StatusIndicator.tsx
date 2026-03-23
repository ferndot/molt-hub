/**
 * StatusIndicator — colorblind-safe status badges using Okabe-Ito palette.
 *
 * Each status is encoded with BOTH color AND shape so that the indicator
 * is distinguishable without relying on color alone.
 *
 * Palette: Okabe & Ito (2008) — optimised for all forms of colour-vision
 * deficiency.
 */

import type { Component } from "solid-js";
import styles from "./StatusIndicator.module.css";
import { STATUS_LABELS } from "./statusTypes";
import type { IndicatorStatus, IndicatorSize, StatusIndicatorProps } from "./statusTypes";

// Re-export types and labels for convenience
export { STATUS_LABELS };
export type { IndicatorStatus, IndicatorSize, StatusIndicatorProps };

// ---------------------------------------------------------------------------
// CSS class map (status → module class name)
// ---------------------------------------------------------------------------

const STATUS_CLASS: Record<IndicatorStatus, string> = {
  pending: styles.pending,
  in_progress: styles.inProgress,
  blocked: styles.blocked,
  awaiting_approval: styles.awaitingApproval,
  success: styles.success,
  failure: styles.failure,
};

const SIZE_CLASS: Record<IndicatorSize, string> = {
  sm: styles.sm,
  md: styles.md,
  lg: styles.lg,
};

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const StatusIndicator: Component<StatusIndicatorProps> = (props) => {
  const size = () => props.size ?? "md";
  const ariaLabel = () => props.label ?? STATUS_LABELS[props.status];

  return (
    <span
      class={`${styles.indicator} ${STATUS_CLASS[props.status]} ${SIZE_CLASS[size()]}`}
      role="img"
      aria-label={ariaLabel()}
    >
      {/* Inner element carries the shape via ::before pseudo-element */}
      <span class={styles.shape} aria-hidden="true" />
    </span>
  );
};

export default StatusIndicator;
