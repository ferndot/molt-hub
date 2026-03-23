/**
 * Metrics store — operational metrics for the Status Bar.
 *
 * Tracks active agent count, pending decision count, and system resource usage.
 * CPU and memory values are mocked for now.
 *
 * TODO: Wire cpuUsage and memoryUsage to the health monitoring WebSocket stream
 * once the backend exposes a "health:metrics" topic.
 */

import { createSignal } from "solid-js";
import { p0Count, p1Count } from "../layout/attentionStore";
import { subscribe } from "../lib/ws";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface MetricsData {
  activeAgentCount: number;
  /** CPU usage 0–100 */
  cpuUsage: number;
  /** Memory usage in bytes */
  memoryBytes: number;
}

// ---------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------

/** Number of agents currently in "Working" / active state. */
const [_activeAgentCount, _setActiveAgentCount] = createSignal<number>(3);

/**
 * CPU usage percentage (0–100).
 * Mocked — will be wired to WebSocket health stream.
 */
const [_cpuUsage, _setCpuUsage] = createSignal<number>(45);

/**
 * Memory usage in bytes.
 * Mocked — will be wired to WebSocket health stream.
 */
const [_memoryBytes, _setMemoryBytes] = createSignal<number>(1_288_490_189); // ~1.2 GiB

// ---------------------------------------------------------------------------
// Public accessors
// ---------------------------------------------------------------------------

/** Reactive accessor: number of active agents. */
export const activeAgentCount = _activeAgentCount;

/**
 * Reactive accessor: number of pending decisions.
 * Derived from attentionStore p0Count + p1Count.
 */
export const pendingDecisionCount = () => p0Count() + p1Count();

/** Reactive accessor: CPU usage percentage (0–100). */
export const cpuUsage = _cpuUsage;

/** Reactive accessor: memory usage in bytes. */
export const memoryUsage = _memoryBytes;

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/**
 * Update metrics from a WebSocket payload.
 * Call this when the health monitoring WebSocket stream emits data.
 *
 * @param data - Partial metrics update; omitted fields are left unchanged.
 */
export function updateMetrics(data: Partial<MetricsData>): void {
  if (data.activeAgentCount !== undefined) {
    _setActiveAgentCount(data.activeAgentCount);
  }
  if (data.cpuUsage !== undefined) {
    _setCpuUsage(data.cpuUsage);
  }
  if (data.memoryBytes !== undefined) {
    _setMemoryBytes(data.memoryBytes);
  }
}

// ---------------------------------------------------------------------------
// Formatting helpers (pure — safe to use in tests)
// ---------------------------------------------------------------------------

/** Format memory bytes to a compact human-readable string, e.g. "1.2G". */
export function formatMemory(bytes: number): string {
  if (bytes >= 1_073_741_824) {
    return `${(bytes / 1_073_741_824).toFixed(1)}G`;
  }
  if (bytes >= 1_048_576) {
    return `${(bytes / 1_048_576).toFixed(0)}M`;
  }
  return `${(bytes / 1024).toFixed(0)}K`;
}

/**
 * Determine the color class for CPU usage.
 * - normal  : < 70%
 * - warning : 70–89%
 * - critical: >= 90%
 */
export type MetricLevel = "normal" | "warning" | "critical";

export function cpuLevel(usage: number): MetricLevel {
  if (usage >= 90) return "critical";
  if (usage >= 70) return "warning";
  return "normal";
}

// ---------------------------------------------------------------------------
// WebSocket subscription — health:metrics
// ---------------------------------------------------------------------------

subscribe("health:metrics", (msg) => {
  if (msg.type !== "event") return;
  const data = msg.payload as Partial<MetricsData>;
  updateMetrics(data);
});
