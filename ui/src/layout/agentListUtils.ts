/**
 * Pure utility functions, types, and reactive store for AgentList.
 * Separated from the component to enable server-safe testing.
 *
 * The store fetches real agent data from GET /api/agents.
 */

import { createSignal } from "solid-js";
import { api } from "../lib/api";
import type { AgentSummary } from "../lib/api";
import { subscribe } from "../lib/ws";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type AgentStatus = "running" | "paused" | "idle" | "terminated";

export interface Agent {
  id: string;
  taskId: string;
  name: string;
  status: AgentStatus;
  stage: string;
}

// ---------------------------------------------------------------------------
// Status group ordering & labels
// ---------------------------------------------------------------------------

export const STATUS_GROUP_ORDER: AgentStatus[] = ["running", "paused", "idle", "terminated"];

export const STATUS_LABELS: Record<AgentStatus, string> = {
  running: "Running",
  paused: "Paused",
  idle: "Idle",
  terminated: "Terminated",
};

export const STATUS_COLOR: Record<AgentStatus, string> = {
  running: "var(--status-running, #22c55e)",
  paused: "var(--status-paused, #f59e0b)",
  idle: "var(--status-idle, #6b7280)",
  terminated: "var(--status-terminated, #e63946)",
};

// ---------------------------------------------------------------------------
// API → Agent mapping
// ---------------------------------------------------------------------------

/**
 * Map a server agent status string (e.g. "Running", "Idle") to the
 * local AgentStatus type. Unknown statuses default to "idle".
 */
function mapApiStatus(status: string): AgentStatus {
  const lower = status.toLowerCase();
  if (lower === "running") return "running";
  if (lower === "paused" || lower === "waiting") return "paused";
  if (lower === "terminated" || lower === "stopped" || lower === "failed") return "terminated";
  return "idle";
}

/**
 * Convert an API AgentSummary to the Agent shape used by the UI.
 */
export function mapApiAgent(a: AgentSummary): Agent {
  return {
    id: a.agent_id,
    taskId: a.task_id,
    name: `Agent ${a.agent_id.slice(-4)}`,
    status: mapApiStatus(a.status),
    stage: mapApiStatus(a.status) === "running" ? "in-progress" : a.status.toLowerCase(),
  };
}

// ---------------------------------------------------------------------------
// Reactive agent store
// ---------------------------------------------------------------------------

const [agents, setAgents] = createSignal<Agent[]>([]);
const [agentsLoaded, setAgentsLoaded] = createSignal(false);

export { agents, agentsLoaded };

/**
 * Fetch agents from the server API and update the reactive store.
 */
export async function fetchAgents(): Promise<Agent[] | null> {
  try {
    const data = await api.getAgents();
    if (!Array.isArray(data.agents)) return null;
    return data.agents.map(mapApiAgent);
  } catch {
    return null;
  }
}

/**
 * Load agents from the API and populate the store.
 * Call once at startup.
 */
export async function initAgents(): Promise<void> {
  const fetched = await fetchAgents();
  setAgents(fetched ?? []);
  setAgentsLoaded(true);
}

/**
 * Re-fetch agents from the API and update the store.
 */
export async function refreshAgents(): Promise<void> {
  const fetched = await fetchAgents();
  if (fetched !== null) {
    setAgents(fetched);
  }
}

// ---------------------------------------------------------------------------
// Periodic refresh
// ---------------------------------------------------------------------------

let agentRefreshInterval: ReturnType<typeof setInterval> | null = null;

export function startAgentRefresh(): void {
  if (agentRefreshInterval !== null) return;
  agentRefreshInterval = setInterval(() => {
    void refreshAgents();
  }, 5_000);
}

export function stopAgentRefresh(): void {
  if (agentRefreshInterval !== null) {
    clearInterval(agentRefreshInterval);
    agentRefreshInterval = null;
  }
}

// ---------------------------------------------------------------------------
// WebSocket-triggered refresh
// ---------------------------------------------------------------------------

subscribe("agent_output:*", () => {
  void refreshAgents();
});

// ---------------------------------------------------------------------------
// Grouping helper
// ---------------------------------------------------------------------------

export interface StatusGroup {
  status: AgentStatus;
  label: string;
  agents: Agent[];
}

export function groupAgentsByStatus(agents: Agent[]): StatusGroup[] {
  const grouped: Record<AgentStatus, Agent[]> = {
    running: [],
    paused: [],
    idle: [],
    terminated: [],
  };

  for (const agent of agents) {
    if (grouped[agent.status]) {
      grouped[agent.status].push(agent);
    }
  }

  return STATUS_GROUP_ORDER
    .filter((s) => grouped[s].length > 0)
    .map((s) => ({
      status: s,
      label: STATUS_LABELS[s],
      agents: grouped[s],
    }));
}
