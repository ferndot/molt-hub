/**
 * Board store — stages and tasks per named board. Fetches
 * GET /api/projects/{workspace}/boards and per-board stages (see `lib/workspace`).
 * WebSocket `board:*` updates apply to the shared task list; the active board
 * filters columns by its stage ids (see missionControlStore).
 */

import { createStore } from "solid-js/store";
import type { ServerMessage } from "../../types";
import { api, type BoardSummary, type PipelineStage } from "../../lib/api";
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
  /** Boards from API (workspace scope). */
  boards: BoardSummary[];
  /** Selected pipeline / kanban board id. */
  activeBoardId: string;
  /** True after the first `initBoardStages` board-list fetch (for URL validation). */
  boardsSynced: boolean;
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

const BOARD_STORAGE_KEY = "molt:active-board";

/** Kanban URL for a board (single path segment, encoded). */
export function boardKanbanPath(boardId: string): string {
  return `/boards/${encodeURIComponent(boardId)}`;
}

/**
 * Returns board id if pathname is exactly `/boards/:id` (one segment), else null.
 */
export function parseBoardIdFromKanbanPath(pathname: string): string | null {
  if (!pathname.startsWith("/boards/")) return null;
  const rest = pathname.slice("/boards/".length);
  if (!rest || rest.includes("/")) return null;
  try {
    return decodeURIComponent(rest);
  } catch {
    return null;
  }
}

/** Home / legacy `/board` redirect target from URL (if already on a kanban path) or localStorage. */
export function homeRedirectBoardPath(): string {
  const fromPath = parseBoardIdFromKanbanPath(window.location.pathname);
  if (fromPath) return boardKanbanPath(fromPath);
  const raw = localStorage.getItem(BOARD_STORAGE_KEY);
  if (raw) return boardKanbanPath(raw);
  return boardKanbanPath("default");
}

function preferredInitialBoardId(boards: BoardSummary[]): string {
  const fromUrl = parseBoardIdFromKanbanPath(window.location.pathname);
  if (fromUrl && boards.some((b) => b.id === fromUrl)) return fromUrl;
  const stored = localStorage.getItem(BOARD_STORAGE_KEY);
  if (stored && boards.some((b) => b.id === stored)) return stored;
  return boards[0].id;
}

/**
 * Serialize board-list and active-board updates. `initBoardStages` runs on mount;
 * without this, its stale `listBoards` result can be applied after `createBoard`
 * and wipe the new board from the store ("create does nothing").
 */
let boardStoreOpChain: Promise<unknown> = Promise.resolve();

function runBoardStoreOp<T>(fn: () => Promise<T>): Promise<T> {
  const p = boardStoreOpChain.then(() => fn());
  boardStoreOpChain = p.then(() => undefined, () => undefined);
  return p;
}

// ---------------------------------------------------------------------------
// API fetch
// ---------------------------------------------------------------------------

/**
 * Fetch stages for a board. Returns null on failure.
 */
export async function fetchBoardStages(
  boardId: string,
): Promise<PipelineStage[] | null> {
  try {
    const data = await api.getBoardStages(boardId);
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
  boardsSynced: false,
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

async function applySetActiveBoard(boardId: string): Promise<void> {
  if (!boardState.boards.some((b) => b.id === boardId)) return;
  setBoardState("activeBoardId", boardId);
  localStorage.setItem(BOARD_STORAGE_KEY, boardId);
  setBoardState("stagesLoaded", false);
  const fetched = await fetchBoardStages(boardId);
  await applyStagesFromFetch(fetched);
  setBoardState("stagesLoaded", true);
}

/**
 * Load board list + stages for the active board.
 * Call when the app mounts (and after external board changes if needed).
 */
export function initBoardStages(): Promise<void> {
  return runBoardStoreOp(async () => {
    setBoardState("tasks", []);
    setBoardState("stagesLoaded", false);
    setBoardState("boardsSynced", false);

    let boards: BoardSummary[] = [];
    try {
      const res = await api.listBoards();
      boards = res.boards ?? [];
    } catch {
      boards = [];
    }
    if (boards.length === 0) {
      boards = [{ id: "default", name: "Default" }];
    }
    setBoardState("boards", boards);

    const pick = preferredInitialBoardId(boards);
    await applySetActiveBoard(pick);
    setBoardState("boardsSynced", true);
  });
}

/**
 * Refresh only the board list from the server (no stage reload, no task clear).
 * Use on the boards index page so navigation does not reset the workboard.
 */
export function refreshBoardList(): Promise<void> {
  return runBoardStoreOp(async () => {
    let boards: BoardSummary[] = [];
    try {
      const res = await api.listBoards();
      boards = res.boards ?? [];
    } catch {
      boards = [];
    }
    if (boards.length === 0) {
      boards = [{ id: "default", name: "Default" }];
    }
    setBoardState("boards", boards);
  });
}

/**
 * Switch the visible board; persists selection in localStorage. Does not clear tasks.
 */
export function setActiveBoard(boardId: string): Promise<void> {
  return runBoardStoreOp(() => applySetActiveBoard(boardId));
}

/** Create a new board (empty default stages on the server). */
export function createBoard(id: string, name?: string): Promise<void> {
  return runBoardStoreOp(async () => {
    const res = await api.createBoard({
      id: id.trim(),
      ...(name?.trim() ? { name: name.trim() } : {}),
    });
    setBoardState("boards", res.boards ?? []);
    await applySetActiveBoard(id.trim());
  });
}

/** Delete a board (not `default`). */
export function deleteBoard(boardId: string): Promise<void> {
  return runBoardStoreOp(async () => {
    if (boardId === "default") return;
    const wasActive = boardState.activeBoardId === boardId;
    const res = await api.deleteBoard(boardId);
    const list = res.boards ?? [];
    setBoardState("boards", list);
    if (wasActive) {
      const next = list[0]?.id ?? "default";
      await applySetActiveBoard(next);
    }
  });
}

/**
 * Push the current pipeline stages to the backend API (active board).
 */
export async function pushStagesToApi(): Promise<void> {
  try {
    await api.updateBoardStages(boardState.activeBoardId, {
      stages: boardState.pipelineStages,
    });
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
    await api.patchBoardStage(boardState.activeBoardId, id, fields);
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
