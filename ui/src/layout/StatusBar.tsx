/**
 * StatusBar — slim 24px bottom bar with connection status, agent metrics,
 * and keyboard hint.
 */

import { createMemo, createSignal, createEffect, on, type Component } from "solid-js";
import type { ConnectionStatus } from "../types";
import {
  activeAgentCount,
  pendingDecisionCount,
  cpuUsage,
  memoryUsage,
  formatMemory,
  cpuLevel,
} from "../stores/metricsStore";
import styles from "./StatusBar.module.css";

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

interface Props {
  status: ConnectionStatus;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const statusLabel: Record<ConnectionStatus, string> = {
  connected: "Connected",
  connecting: "Connecting…",
  disconnected: "Disconnected",
  error: "Error",
};

const statusColor: Record<ConnectionStatus, string> = {
  connected: "#22c55e",
  connecting: "#f59e0b",
  disconnected: "#6b7280",
  error: "#ef4444",
};

/** Map a MetricLevel to the appropriate CSS module class name. */
function cpuColorClass(level: ReturnType<typeof cpuLevel>): string {
  if (level === "critical") return styles.metricCritical;
  if (level === "warning") return styles.metricWarning;
  return styles.metricNormal;
}

// ---------------------------------------------------------------------------
// Sub-component: separator
// ---------------------------------------------------------------------------

const Sep: Component = () => (
  <span class={styles.separator} aria-hidden="true">|</span>
);

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const StatusBar: Component<Props> = (props) => {
  const cpuClass = createMemo(() => cpuColorClass(cpuLevel(cpuUsage())));
  const pendingCount = createMemo(() => pendingDecisionCount());

  // Debounce transient connection states ("connecting", "error") so the bar
  // doesn't jump on every reconnect cycle. "connected" and "disconnected" are
  // shown immediately; intermediate states are delayed by 400ms.
  const [stableStatus, setStableStatus] = createSignal<ConnectionStatus>(props.status);
  let statusTimer: ReturnType<typeof setTimeout> | undefined;

  createEffect(on(() => props.status, (next) => {
    clearTimeout(statusTimer);
    if (next === "connected" || next === "disconnected") {
      setStableStatus(next);
    } else {
      statusTimer = setTimeout(() => setStableStatus(next), 400);
    }
  }));

  return (
    <div class={styles.bar} role="status">
      {/* Left section: connection + metrics */}
      <div class={styles.left}>
        {/* Connection status */}
        <span
          class={styles.dot}
          style={{ background: statusColor[stableStatus()] }}
          aria-hidden="true"
        />
        <span class={styles.label}>{statusLabel[stableStatus()]}</span>

        <Sep />

        {/* Active agents — count from metricsStore (updated via WebSocket) */}
        <span class={styles.metricNormal}>
          🟢 {activeAgentCount()} active
        </span>

        <Sep />

        {/* Pending decisions — derived from attentionStore p0Count + p1Count */}
        <span class={styles.metricItem}>
          ⚡ {pendingCount()} pending
        </span>

        <Sep />

        {/* CPU usage — mocked; TODO: wire to health monitoring WebSocket stream */}
        <span class={cpuClass()}>
          CPU {cpuUsage()}%
        </span>

        <Sep />

        {/* Memory usage — mocked; TODO: wire to health monitoring WebSocket stream */}
        <span class={styles.metricItem}>
          MEM {formatMemory(memoryUsage())}
        </span>
      </div>

      {/* Right section: shortcuts hint */}
      <div class={styles.right}>
        <span class={styles.hint}>? shortcuts</span>
      </div>
    </div>
  );
};

export default StatusBar;
