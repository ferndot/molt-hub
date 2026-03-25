/**
 * Agent detail store — holds state for a single agent's detail view.
 *
 * Fetches real agent data from the backend API and subscribes
 * to WebSocket topic `agent:{id}` for live output.
 */

import { createStore } from "solid-js/store";
import { subscribe } from "../../lib/ws";
import { api } from "../../lib/api";
import { addAgentMessage } from "./steerStore";
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
  authError?: string;
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

export function clearAuthError(agentId: string): void {
  setState(
    "agents",
    (a) => a.id === agentId,
    "authError",
    undefined,
  );
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
 * Preserves `outputLines` for agents still present so polling does not wipe the terminal buffer.
 */
export async function fetchAgents(): Promise<void> {
  try {
    const data = await api.getAgents();
    const agents = data.agents ?? [];
    setState("agents", (prev) => {
      if (agents.length === 0) return [];
      const prevById = new Map(prev.map((a) => [a.id, a]));
      return agents.map((raw) => {
        const m = mapApiAgent(raw);
        const old = prevById.get(m.id);
        if (old && old.outputLines.length > 0) {
          return { ...m, outputLines: old.outputLines };
        }
        return m;
      });
    });
  } catch {
    // API unreachable — keep existing state
  }
}

function formatOutputTimestamp(iso: string | undefined): string {
  if (!iso) {
    return new Date().toLocaleTimeString("en-US", {
      hour12: false,
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }
  const d = new Date(iso);
  return d.toLocaleTimeString("en-US", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** Ensure a row exists so detail/chat views can render before the next poll. */
export function registerAgentPlaceholder(
  id: string,
  opts?: { taskName?: string },
): void {
  if (state.agents.some((a) => a.id === id)) return;
  setState("agents", (agents) => [
    ...agents,
    {
      id,
      name: "Claude Code",
      taskName: opts?.taskName ?? "Project chat",
      taskDescription: "",
      currentStage: "running",
      stageHistory: [],
      status: "running",
      priority: "p2" as Priority,
      assignedAt: new Date().toISOString(),
      outputLines: [],
    },
  ]);
}

/** Replace buffered output from the server snapshot (e.g. after navigation or resume). */
export async function hydrateAgentOutput(agentId: string): Promise<void> {
  try {
    const res = await api.getAgentOutput(agentId);
    const lines = (res.lines ?? []) as Array<{ line?: string; timestamp?: string }>;
    const mapped: OutputLine[] = lines.map((l) => ({
      text: String(l.line ?? ""),
      timestamp: formatOutputTimestamp(l.timestamp),
    }));
    setState("agents", (agents) =>
      agents.map((a) => (a.id === agentId ? { ...a, outputLines: mapped } : a)),
    );
  } catch {
    /* ignore */
  }
}

export function removeAgentFromStore(agentId: string): void {
  setState("agents", (agents) => agents.filter((a) => a.id !== agentId));
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

    // Handle auth errors.
    if (payload.type === "agent_error") {
      const authRequired = payload.auth_required as boolean | undefined;
      const message = payload.message as string | undefined;
      setState(
        "agents",
        (a) => a.id === agentId,
        "authError",
        authRequired ? (message ?? "Authentication required") : undefined,
      );
      return;
    }

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
      addAgentMessage(agentId, output);
    }
  });
  return unsubscribe;
}
