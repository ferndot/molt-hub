/**
 * Agent detail store — holds state for a single agent's detail view.
 *
 * Fetches real agent data from the backend API and subscribes
 * to WebSocket topic `agent:{id}` for live output.
 */

import { createStore } from "solid-js/store";
import { subscribe } from "../../lib/ws";
import { api } from "../../lib/api";
import type { AgentSummary } from "../../lib/api";
import type { Priority } from "../../types/domain";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface OutputLine {
  timestamp: string;
  text: string;
}

export interface StageEntry {
  stage: string;
  enteredAt: string;
}

export interface AgentDetail {
  id: string;
  name: string;
  taskName: string;
  taskDescription: string;
  currentStage: string;
  stageHistory: StageEntry[];
  status: "running" | "paused" | "terminated" | "idle";
  priority: Priority;
  assignedAt: string;
  outputLines: OutputLine[];
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export interface AgentDetailState {
  agents: AgentDetail[];
  activeId: string | null;
}

const [state, setState] = createStore<AgentDetailState>({
  agents: [],
  activeId: null,
});

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

export function setActiveAgent(id: string): void {
  setState("activeId", id);
}

export function appendOutputLine(agentId: string, line: OutputLine): void {
  setState("agents", (agents) =>
    agents.map((a) =>
      a.id === agentId
        ? { ...a, outputLines: [...a.outputLines, line] }
        : a,
    ),
  );
}

// ---------------------------------------------------------------------------
// Selectors
// ---------------------------------------------------------------------------

export function getAgent(id: string): AgentDetail | undefined {
  return state.agents.find((a) => a.id === id);
}

/** Read-only store access. */
export function useAgentDetailStore() {
  return { state };
}

// ---------------------------------------------------------------------------
// WebSocket subscription (stub)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// API fetch — load real agents from the backend
// ---------------------------------------------------------------------------

function mapApiAgent(a: AgentSummary): AgentDetail {
  return {
    id: a.agent_id,
    name: a.agent_id.slice(0, 8),
    taskName: a.task_id ?? "",
    taskDescription: "",
    currentStage: a.status ?? "unknown",
    stageHistory: [],
    status: (a.status?.toLowerCase() as AgentDetail["status"]) ?? "idle",
    priority: "p2" as Priority,
    assignedAt: new Date().toISOString(),
    outputLines: [],
  };
}

/**
 * Fetch agents from the backend and merge into the store.
 */
export async function fetchAgents(): Promise<void> {
  try {
    const data = await api.getAgents();
    const agents = data.agents ?? [];
    if (agents.length > 0) {
      setState("agents", agents.map(mapApiAgent));
    }
  } catch {
    // API unreachable — keep existing state
  }
}

/**
 * Start polling agents every `intervalMs` milliseconds.
 * Returns a cleanup function that stops the polling interval.
 */
export function startAgentPolling(intervalMs = 3000): () => void {
  // Initial fetch
  fetchAgents();
  const timer = setInterval(fetchAgents, intervalMs);
  return () => clearInterval(timer);
}

export function setupAgentSubscription(agentId: string): () => void {
  const topic = `agent:${agentId}`;
  const unsubscribe = subscribe(topic, (msg) => {
    if (msg.type !== "event") return;
    const payload = msg.payload as Record<string, unknown>;

    // Append agent output lines to the store.
    const output = payload.output as string | undefined;
    const timestamp = payload.timestamp as string | undefined;
    if (output) {
      const ts = timestamp
        ? new Date(timestamp).toLocaleTimeString("en-US", {
            hour12: false,
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
          })
        : new Date().toLocaleTimeString("en-US", {
            hour12: false,
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
          });
      appendOutputLine(agentId, { timestamp: ts, text: output });
    }
  });
  return unsubscribe;
}
