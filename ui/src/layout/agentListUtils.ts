/**
 * Pure utility functions and types for AgentList grouping logic.
 * Separated from the component to enable server-safe testing.
 */

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type AgentStatus = "running" | "paused" | "idle" | "terminated";

export interface MockAgent {
  id: string;
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
// Mock data (replaced by real data from T25/T29)
// ---------------------------------------------------------------------------

export const MOCK_AGENTS: MockAgent[] = [
  { id: "agent-001", name: "frontend", status: "running", stage: "Working" },
  { id: "agent-002", name: "backend-api", status: "paused", stage: "Needs Review" },
  { id: "agent-003", name: "core-engine", status: "running", stage: "Testing" },
  { id: "agent-004", name: "infra", status: "terminated", stage: "Completed" },
  { id: "agent-005", name: "docs-agent", status: "idle", stage: "Idle" },
];

// ---------------------------------------------------------------------------
// Grouping helper
// ---------------------------------------------------------------------------

export interface StatusGroup {
  status: AgentStatus;
  label: string;
  agents: MockAgent[];
}

export function groupAgentsByStatus(agents: MockAgent[]): StatusGroup[] {
  const grouped: Record<AgentStatus, MockAgent[]> = {
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
