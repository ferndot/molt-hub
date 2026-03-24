/**
 * Board store — SolidJS createStore holding board state: stages and tasks
 * grouped by stage. Subscribes to WebSocket topic "board:*" for real-time
 * updates.
 *
 * Stages are fetched from GET /api/pipeline/stages on init, falling back
 * to hardcoded defaults if the API is unavailable.
 */

import { createStore } from "solid-js/store";
import { subscribe } from "../../lib/ws";
import { api, type PipelineStage } from "../../lib/api";
import type { Priority } from "../../types/domain";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type BoardTaskStatus =
  | "running"
  | "waiting"
  | "blocked"
  | "complete";

export interface BoardTask {
  id: string;
  name: string;
  agentName: string;
  priority: Priority;
  status: BoardTaskStatus;
  stage: string;
  summary: string;
  timeInStage: string;
  expanded: boolean;
}

// PipelineStage is imported from ../../lib/api
export type { PipelineStage } from "../../lib/api";

export interface BoardState {
  stages: string[];
  pipelineStages: PipelineStage[];
  stagesLoaded: boolean;
  tasks: BoardTask[];
}

// ---------------------------------------------------------------------------
// Default / fallback stages
// ---------------------------------------------------------------------------

const DEFAULT_STAGES: string[] = [
  "backlog",
  "in-progress",
  "code-review",
  "testing",
  "deployed",
];

const DEFAULT_PIPELINE_STAGES: PipelineStage[] = [
  { id: "backlog", label: "Backlog", wip_limit: null, requires_approval: false, timeout_seconds: null, terminal: false, color: "#6b7280", order: 0 },
  { id: "in-progress", label: "In Progress", wip_limit: null, requires_approval: false, timeout_seconds: null, terminal: false, color: "#3b82f6", order: 1 },
  { id: "code-review", label: "Code Review", wip_limit: null, requires_approval: false, timeout_seconds: null, terminal: false, color: "#f59e0b", order: 2 },
  { id: "testing", label: "Testing", wip_limit: null, requires_approval: false, timeout_seconds: null, terminal: false, color: "#8b5cf6", order: 3 },
  { id: "deployed", label: "Deployed", wip_limit: null, requires_approval: false, timeout_seconds: null, terminal: true, color: "#10b981", order: 4 },
];

// ---------------------------------------------------------------------------
// API fetch
// ---------------------------------------------------------------------------

interface StagesApiResponse {
  stages: PipelineStage[];
}

/**
 * Fetch pipeline stages from the server.
 * Returns null on failure so callers can fall back to defaults.
 */
export async function fetchPipelineStages(): Promise<PipelineStage[] | null> {
  try {
    const response = await fetch("/api/pipeline/stages");
    if (!response.ok) return null;
    const ct = response.headers.get("content-type") ?? "";
    if (!ct.includes("application/json")) return null;
    const data = (await response.json()) as StagesApiResponse;
    if (!Array.isArray(data.stages) || data.stages.length === 0) return null;
    return data.stages;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

const initialState: BoardState = {
  stages: DEFAULT_STAGES,
  pipelineStages: DEFAULT_PIPELINE_STAGES,
  stagesLoaded: false,
  tasks: [],
};

export const [boardState, setBoardState] =
  createStore<BoardState>(initialState);

// ---------------------------------------------------------------------------
// Stage initialisation
// ---------------------------------------------------------------------------

/**
 * Load pipeline stages from the server API and update the store.
 * Falls back to the hardcoded defaults if the API is unavailable.
 * Call this once at app startup or when the Board view mounts.
 */
export async function initBoardStages(): Promise<void> {
  const fetched = await fetchPipelineStages();
  if (fetched) {
    setBoardState("stages", fetched.map((s) => s.id));
    setBoardState("pipelineStages", fetched);
  }
  setBoardState("stagesLoaded", true);
}

/**
 * Push the current pipeline stages to the backend API.
 * Called after column edits in the ColumnEditor.
 */
export async function pushStagesToApi(): Promise<void> {
  try {
    await api.updateStages({ stages: boardState.pipelineStages });
  } catch {
    // Silently ignore — the board still works with local state
  }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

export function moveTask(
  taskId: string,
  _fromStage: string,
  toStage: string,
): void {
  setBoardState("tasks", (tasks) =>
    tasks.map((t) => (t.id === taskId ? { ...t, stage: toStage } : t)),
  );
}

export function expandCard(taskId: string): void {
  setBoardState("tasks", (tasks) =>
    tasks.map((t) => (t.id === taskId ? { ...t, expanded: true } : t)),
  );
}

export function collapseCard(taskId: string): void {
  setBoardState("tasks", (tasks) =>
    tasks.map((t) => (t.id === taskId ? { ...t, expanded: false } : t)),
  );
}

export function toggleCard(taskId: string): void {
  setBoardState("tasks", (tasks) =>
    tasks.map((t) =>
      t.id === taskId ? { ...t, expanded: !t.expanded } : t,
    ),
  );
}

// ---------------------------------------------------------------------------
// Priority ordering helper
// ---------------------------------------------------------------------------

const PRIORITY_ORDER: Record<Priority, number> = {
  p0: 0,
  p1: 1,
  p2: 2,
  p3: 3,
};

export function sortByPriority<T extends BoardTask>(tasks: T[]): T[] {
  return [...tasks].sort(
    (a, b) => PRIORITY_ORDER[a.priority] - PRIORITY_ORDER[b.priority],
  );
}

export function tasksForStage(stage: string): BoardTask[] {
  return sortByPriority(boardState.tasks.filter((t) => t.stage === stage));
}

// ---------------------------------------------------------------------------
// Stage helpers
// ---------------------------------------------------------------------------

/**
 * Return pipeline stages sorted by their `order` field.
 */
export function getSortedStages(): PipelineStage[] {
  return [...boardState.pipelineStages].sort((a, b) => a.order - b.order);
}

/**
 * Optimistically update a pipeline stage in local state and persist via PATCH API.
 */
export async function patchStage(
  id: string,
  fields: Partial<PipelineStage>,
): Promise<void> {
  // Optimistic local update
  setBoardState("pipelineStages", (stages) =>
    stages.map((s) => (s.id === id ? { ...s, ...fields } : s)),
  );
  try {
    await api.patchStage(id, fields);
  } catch {
    // Silently ignore — the board still works with local state
  }
}

// ---------------------------------------------------------------------------
// WebSocket subscription (stub — real handler wired when server sends board events)
// ---------------------------------------------------------------------------

subscribe("board:*", (msg) => {
  if (msg.type !== "event") return;
  const payload = msg.payload as Record<string, unknown>;
  const taskId = payload.task_id as string | undefined;
  if (!taskId) return;

  // Check if this is an existing task update or a new task
  const existing = boardState.tasks.find((t) => t.id === taskId);

  if (existing) {
    // Update existing task fields from the payload
    setBoardState("tasks", (tasks) =>
      tasks.map((t) => {
        if (t.id !== taskId) return t;
        return {
          ...t,
          ...(payload.stage != null ? { stage: payload.stage as string } : {}),
          ...(payload.status != null
            ? { status: payload.status as BoardTaskStatus }
            : {}),
          ...(payload.priority != null
            ? { priority: payload.priority as Priority }
            : {}),
          ...(payload.name != null ? { name: payload.name as string } : {}),
          ...(payload.agent_name != null
            ? { agentName: payload.agent_name as string }
            : {}),
          ...(payload.summary != null
            ? { summary: payload.summary as string }
            : {}),
        };
      }),
    );
  } else if (payload.stage && payload.status) {
    // New task — append to the board
    const newTask: BoardTask = {
      id: taskId,
      name: (payload.name as string) ?? "Untitled",
      agentName: (payload.agent_name as string) ?? "unknown",
      priority: (payload.priority as Priority) ?? "p2",
      status: payload.status as BoardTaskStatus,
      stage: payload.stage as string,
      summary: (payload.summary as string) ?? "",
      timeInStage: "0m",
      expanded: false,
    };
    setBoardState("tasks", (tasks) => [...tasks, newTask]);
  }
});
