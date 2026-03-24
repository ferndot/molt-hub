/**
 * Board store — stages and tasks per named board. Fetches
 * GET /api/projects/:id/boards and .../boards/:bid/stages.
 * WebSocket `board:*` updates apply to the shared task list; the active board
 * filters columns by its stage ids (see missionControlStore).
 */

import { createStore } from "solid-js/store";
import type { ServerMessage } from "../../types";
import { api, type BoardSummary, type PipelineStage } from "../../lib/api";
import { projectState } from "../../stores/projectStore";
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

export type { PipelineStage } from "../../lib/api";

export interface BoardState {
  stages: string[];
  pipelineStages: PipelineStage[];
  stagesLoaded: boolean;
  tasks: BoardTask[];
  /** Boards for the active project (from API). */
  boards: BoardSummary[];
  /** Selected pipeline / kanban board id. */
  activeBoardId: string;
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

function boardStorageKey(projectId: string): string {
  return `molt:active-board:${projectId}`;
}

// ---------------------------------------------------------------------------
// API fetch
// ---------------------------------------------------------------------------

/**
 * Fetch stages for a project board. Returns null on failure.
 */
export async function fetchBoardStages(
  projectId: string,
  boardId: string,
): Promise<PipelineStage[] | null> {
  try {
    const data = await api.getProjectBoardStages(projectId, boardId);
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
  boards: [{ id: "default", name: "Default" }],
  activeBoardId: "default",
};

export const [boardState, setBoardState] =
  createStore<BoardState>(initialState);

// ---------------------------------------------------------------------------
// Stage initialisation
// ---------------------------------------------------------------------------

async function applyStagesFromFetch(fetched: PipelineStage[] | null): Promise<void> {
  if (fetched) {
    const sorted = [...fetched].sort((a, b) => a.order - b.order);
    setBoardState("stages", sorted.map((s) => s.id));
    setBoardState("pipelineStages", sorted);
  } else {
    setBoardState("stages", DEFAULT_STAGES);
    setBoardState("pipelineStages", DEFAULT_PIPELINE_STAGES);
  }
}

/**
 * Load board list + stages for the active board. Clears tasks on project switch.
 * Call when the workboard mounts and when `activeProjectId` changes.
 */
export async function initBoardStages(): Promise<void> {
  const projectId = projectState.activeProjectId;
  setBoardState("tasks", []);
  setBoardState("stagesLoaded", false);

  let boards: BoardSummary[] = [];
  try {
    const res = await api.listProjectBoards(projectId);
    boards = res.boards ?? [];
  } catch {
    boards = [];
  }
  if (boards.length === 0) {
    boards = [{ id: "default", name: "Default" }];
  }
  setBoardState("boards", boards);

  const key = boardStorageKey(projectId);
  const stored = localStorage.getItem(key);
  const pick =
    stored && boards.some((b) => b.id === stored)
      ? stored
      : boards[0].id;
  setBoardState("activeBoardId", pick);

  const fetched = await fetchBoardStages(projectId, pick);
  await applyStagesFromFetch(fetched);
  setBoardState("stagesLoaded", true);
}

/**
 * Switch the visible board; persists per project. Does not clear tasks.
 */
export async function setActiveBoard(boardId: string): Promise<void> {
  const projectId = projectState.activeProjectId;
  if (!boardState.boards.some((b) => b.id === boardId)) return;
  setBoardState("activeBoardId", boardId);
  localStorage.setItem(boardStorageKey(projectId), boardId);
  setBoardState("stagesLoaded", false);
  const fetched = await fetchBoardStages(projectId, boardId);
  await applyStagesFromFetch(fetched);
  setBoardState("stagesLoaded", true);
}

/** Create a new board (empty default stages on the server). */
export async function createBoard(id: string, name?: string): Promise<void> {
  const projectId = projectState.activeProjectId;
  const res = await api.createProjectBoard(projectId, {
    id: id.trim(),
    ...(name?.trim() ? { name: name.trim() } : {}),
  });
  setBoardState("boards", res.boards ?? []);
  await setActiveBoard(id.trim());
}

/** Delete a board (not `default`). */
export async function deleteBoard(boardId: string): Promise<void> {
  if (boardId === "default") return;
  const projectId = projectState.activeProjectId;
  const wasActive = boardState.activeBoardId === boardId;
  const res = await api.deleteProjectBoard(projectId, boardId);
  const list = res.boards ?? [];
  setBoardState("boards", list);
  if (wasActive) {
    const next = list[0]?.id ?? "default";
    await setActiveBoard(next);
  }
}

/**
 * Push the current pipeline stages to the backend API (active board).
 */
export async function pushStagesToApi(): Promise<void> {
  try {
    await api.updateProjectBoardStages(
      projectState.activeProjectId,
      boardState.activeBoardId,
      { stages: boardState.pipelineStages },
    );
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

export function getSortedStages(): PipelineStage[] {
  return [...boardState.pipelineStages].sort((a, b) => a.order - b.order);
}

export async function patchStage(
  id: string,
  fields: Partial<PipelineStage>,
): Promise<void> {
  setBoardState("pipelineStages", (stages) =>
    stages.map((s) => (s.id === id ? { ...s, ...fields } : s)),
  );
  try {
    await api.patchProjectBoardStage(
      projectState.activeProjectId,
      boardState.activeBoardId,
      id,
      fields,
    );
  } catch {
    // Silently ignore — the board still works with local state
  }
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

export function handleBoardWsMessage(msg: ServerMessage): void {
  if (msg.type !== "event") return;
  const payload = msg.payload as Record<string, unknown>;
  const taskId = payload.task_id as string | undefined;
  if (!taskId) return;

  const existing = boardState.tasks.find((t) => t.id === taskId);

  if (existing) {
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
}
