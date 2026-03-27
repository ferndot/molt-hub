/**
 * Board store — stages and tasks per named board. Fetches
 * GET /api/projects/{workspace}/boards and per-board stages (see `lib/workspace`).
 * WebSocket `board:*` updates apply to the shared task list; the active board
 * filters columns by its stage ids (see missionControlStore).
 */

import { createStore } from "solid-js/store";
import type { ServerMessage } from "../../types";
import { api, type BoardSummary, type BoardTaskItem, type PipelineStage } from "../../lib/api";
import type { Priority } from "../../types/domain";
import { emitHookToast } from "../../lib/hookToasts";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type BoardTaskStatus =
  | "running"
  | "waiting"
  | "blocked"
  | "complete";

export type AgentStatus =
  | 'waiting'
  | 'working'
  | 'succeeded'
  | 'errored'
  | 'needs-attention';

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
  agentStatus?: AgentStatus;
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
  "planning",
  "in-progress",
  "review",
  "testing",
  "deployment",
];

const DEFAULT_PIPELINE_STAGES: PipelineStage[] = [
  { id: "backlog",     label: "Backlog",     wip_limit: null, requires_approval: false, timeout_seconds: null, terminal: false,  color: "#94a3b8", order: 0, hooks: [] },
  { id: "planning",   label: "Planning",    wip_limit: null, requires_approval: true,  timeout_seconds: null, terminal: false,  color: "#a855f7", order: 1, hooks: [] },
  { id: "in-progress",label: "In Progress", wip_limit: 5,    requires_approval: false, timeout_seconds: null, terminal: false,  color: "#6366f1", order: 2, hooks: [] },
  { id: "review",     label: "Review",      wip_limit: null, requires_approval: true,  timeout_seconds: null, terminal: false,  color: "#f59e0b", order: 3, hooks: [] },
  { id: "testing",    label: "Testing",     wip_limit: null, requires_approval: false, timeout_seconds: null, terminal: false,  color: "#10b981", order: 4, hooks: [] },
  { id: "deployment", label: "Deployment",  wip_limit: null, requires_approval: false, timeout_seconds: null, terminal: true,   color: "#22c55e", order: 5, hooks: [] },
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

/** Home redirect: kanban URL from path, last-used board, or boards list. */
export function homeRedirectBoardPath(): string {
  const fromPath = parseBoardIdFromKanbanPath(window.location.pathname);
  if (fromPath) return boardKanbanPath(fromPath);
  const raw = localStorage.getItem(BOARD_STORAGE_KEY);
  if (raw) return boardKanbanPath(raw);
  return "/boards";
}

function preferredInitialBoardId(boards: BoardSummary[]): string | null {
  const fromUrl = parseBoardIdFromKanbanPath(window.location.pathname);
  if (fromUrl && boards.some((b) => b.id === fromUrl)) return fromUrl;
  const stored = localStorage.getItem(BOARD_STORAGE_KEY);
  if (stored && boards.some((b) => b.id === stored)) return stored;
  if (boards.length === 0) return null;
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
  boards: [],
  activeBoardId: "",
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
  if (!boardId || !boardState.boards.some((b) => b.id === boardId)) return;
  setBoardState("activeBoardId", boardId);
  localStorage.setItem(BOARD_STORAGE_KEY, boardId);
  setBoardState("stagesLoaded", false);
  const fetched = await fetchBoardStages(boardId);
  await applyStagesFromFetch(fetched);
  setBoardState("stagesLoaded", true);
}

/** Clear selection when there are no boards (or last board removed). */
async function applyNoActiveBoard(): Promise<void> {
  setBoardState("activeBoardId", "");
  localStorage.removeItem(BOARD_STORAGE_KEY);
  setBoardState("stagesLoaded", false);
  await applyStagesFromFetch(null);
  setBoardState("stagesLoaded", true);
}

/** Derive the UI agent status from a task status string and optional agent name. */
function deriveAgentStatus(
  status: string | undefined,
  agentName: string | undefined,
): AgentStatus | undefined {
  if (!status) return undefined;
  if (status === 'needs_review') return 'needs-attention';
  if (status === 'completed') return 'succeeded';
  if (status === 'failed') return 'errored';
  if (status === 'in_progress') return agentName ? 'working' : 'waiting';
  return undefined;
}

/** Map a raw BoardTaskItem from the REST API into a BoardTask for the store. */
function boardTaskFromItem(t: BoardTaskItem): BoardTask {
  return {
    id: t.task_id,
    name: t.name,
    agentName: t.agent_name ?? "",
    priority: (t.priority as BoardTask["priority"]) ?? "p2",
    status: (t.status as BoardTask["status"]) ?? "waiting",
    stage: t.stage,
    summary: t.summary ?? "",
    timeInStage: "",
    expanded: false,
    agentStatus: deriveAgentStatus(t.status, t.agent_name ?? undefined),
  };
}

/**
 * Fetch the current board task list from the event store and populate the
 * shared task list.  Silently ignored if the endpoint is unavailable.
 */
async function loadBoardTasksFromApi(): Promise<void> {
  try {
    const data = await api.getBoardTasks();
    if (Array.isArray(data.tasks)) {
      setBoardState("tasks", data.tasks.map(boardTaskFromItem));
    }
  } catch {
    // Server may not have event store enabled; fall back to empty / WS-only.
  }
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
    setBoardState("boards", boards);

    const pick = preferredInitialBoardId(boards);
    if (pick) {
      await applySetActiveBoard(pick);
    } else {
      await applyNoActiveBoard();
    }
    setBoardState("boardsSynced", true);

    // Populate tasks from the persisted event store so the board is non-empty
    // on first load (and after restarts).  WS updates continue to apply on top.
    await loadBoardTasksFromApi();
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
    setBoardState("boards", boards);
  });
}

/**
 * Switch the visible board; persists selection in localStorage. Does not clear tasks.
 */
export function setActiveBoard(boardId: string): Promise<void> {
  return runBoardStoreOp(() => applySetActiveBoard(boardId));
}

/** Create a new board; server assigns a ULID. Returns the new board id. */
export function createBoard(displayName: string): Promise<string> {
  return runBoardStoreOp(async () => {
    const res = await api.createBoard({ name: displayName.trim() });
    const id = res.boardId;
    if (!id) {
      throw new Error("Server did not return boardId");
    }
    setBoardState("boards", res.boards ?? []);
    await applySetActiveBoard(id);
    return id;
  });
}

/** Delete a board. */
export function deleteBoard(boardId: string): Promise<void> {
  return runBoardStoreOp(async () => {
    const wasActive = boardState.activeBoardId === boardId;
    const res = await api.deleteBoard(boardId);
    const list = res.boards ?? [];
    setBoardState("boards", list);
    if (wasActive) {
      const next = list[0]?.id ?? null;
      if (next) {
        await applySetActiveBoard(next);
      } else {
        await applyNoActiveBoard();
      }
    }
  });
}

/**
 * Push the current pipeline stages to the backend API (active board).
 */
export async function pushStagesToApi(): Promise<void> {
  if (!boardState.activeBoardId) return;
  try {
    await api.updateBoardStages(boardState.activeBoardId, {
      stages: boardState.pipelineStages,
    });
  } catch {
    // Silently ignore — the board still works with local state
  }
}

function normalizeStageOrders(stages: PipelineStage[]): PipelineStage[] {
  return stages.map((s, i) => ({ ...s, order: i }));
}

function suggestNewStageId(existing: Set<string>): string {
  for (let n = 1; n < 10_000; n++) {
    const id = `column-${n}`;
    if (!existing.has(id)) return id;
  }
  return `column-${crypto.randomUUID().slice(0, 8)}`;
}

/**
 * PUT full stage list for the active board and sync `boardState.stages` ids.
 * Rolls back local state and refetches on failure.
 */
async function putBoardStages(stages: PipelineStage[]): Promise<void> {
  const bid = boardState.activeBoardId;
  if (!bid) return;
  const normalized = normalizeStageOrders(stages);
  const prevStages = [...boardState.pipelineStages];
  const prevIds = [...boardState.stages];
  setBoardState("pipelineStages", normalized);
  setBoardState("stages", normalized.map((s) => s.id));
  try {
    const res = await api.updateBoardStages(bid, { stages: normalized });
    if (res.stages?.length) {
      const sorted = [...res.stages].sort((a, b) => a.order - b.order);
      setBoardState("pipelineStages", sorted);
      setBoardState("stages", sorted.map((s) => s.id));
    }
  } catch {
    setBoardState("pipelineStages", prevStages);
    setBoardState("stages", prevIds);
    try {
      const data = await api.getBoardStages(bid);
      if (data.stages?.length) {
        const sorted = [...data.stages].sort((a, b) => a.order - b.order);
        setBoardState("pipelineStages", sorted);
        setBoardState("stages", sorted.map((s) => s.id));
      }
    } catch {
      /* keep rollback */
    }
    throw new Error("Could not save columns to the server.");
  }
}

/** Append a new column (stable id `column-N`) and persist. */
export function addBoardColumn(): Promise<void> {
  return runBoardStoreOp(async () => {
    const sorted = getSortedStages();
    const ids = new Set(sorted.map((s) => s.id));
    const id = suggestNewStageId(ids);
    const next: PipelineStage[] = [
      ...sorted,
      {
        id,
        label: "New column",
        wip_limit: null,
        requires_approval: false,
        timeout_seconds: null,
        terminal: false,
        color: "#6366f1",
        order: sorted.length,
        hooks: [],
      },
    ];
    await putBoardStages(next);
  });
}

/**
 * Remove a column by id. Fails if it is the last column or any task still uses it.
 */
export function removeBoardColumn(stageId: string): Promise<void> {
  return runBoardStoreOp(async () => {
    const sorted = getSortedStages();
    if (sorted.length <= 1) {
      throw new Error("The board must keep at least one column.");
    }
    if (boardState.tasks.some((t) => t.stage === stageId)) {
      throw new Error(
        "Move or remove every task from this column before deleting it.",
      );
    }
    const next = sorted.filter((s) => s.id !== stageId);
    await putBoardStages(next);
  });
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

export function moveTask(
  taskId: string,
  _fromStage: string,
  toStage: string,
): void {
  const prev = boardState.tasks.map((t) => ({ ...t }));
  setBoardState("tasks", (tasks) =>
    tasks.map((t) => (t.id === taskId ? { ...t, stage: toStage } : t)),
  );
  const boardId = boardState.activeBoardId;
  if (!boardId) return;
  void (async () => {
    try {
      await api.moveTask(taskId, { toStage, boardId });
    } catch {
      setBoardState("tasks", prev);
    }
  })();
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
  if (!boardState.activeBoardId) return;
  try {
    const updated = await api.patchBoardStage(
      boardState.activeBoardId,
      id,
      fields,
    );
    const merged = boardState.pipelineStages.map((s) =>
      s.id === id ? { ...s, ...updated } : s,
    );
    const sorted = [...merged].sort((a, b) => a.order - b.order);
    setBoardState("pipelineStages", sorted);
    setBoardState(
      "stages",
      sorted.map((s) => s.id),
    );
  } catch {
    // Keep optimistic state — the board still works locally
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
    const newStage = payload.stage != null ? payload.stage as string : null;
    if (newStage && existing.stage !== newStage) {
      // on_exit the old stage, on_enter the new stage
      emitHookToast(existing.stage, "on_exit", existing.name);
      emitHookToast(newStage, "on_enter", existing.name);
    }
    setBoardState("tasks", (tasks) =>
      tasks.map((t) => {
        if (t.id !== taskId) return t;
        const updatedStatus = payload.status != null
          ? payload.status as BoardTaskStatus
          : t.status;
        const updatedAgentName = payload.agent_name != null
          ? payload.agent_name as string
          : t.agentName;
        return {
          ...t,
          ...(payload.stage != null ? { stage: payload.stage as string } : {}),
          ...(payload.status != null
            ? { status: updatedStatus }
            : {}),
          ...(payload.priority != null
            ? { priority: payload.priority as Priority }
            : {}),
          ...(payload.name != null ? { name: payload.name as string } : {}),
          ...(payload.agent_name != null
            ? { agentName: updatedAgentName }
            : {}),
          ...(payload.summary != null
            ? { summary: payload.summary as string }
            : {}),
          ...(payload.status != null || payload.agent_name != null
            ? { agentStatus: deriveAgentStatus(updatedStatus, updatedAgentName) }
            : {}),
        };
      }),
    );
  } else if (payload.stage && payload.status) {
    const newAgentName = (payload.agent_name as string) ?? "";
    const newStatus = payload.status as BoardTaskStatus;
    const newTask: BoardTask = {
      id: taskId,
      name: (payload.name as string) ?? "Untitled",
      agentName: newAgentName,
      priority: (payload.priority as Priority) ?? "p2",
      status: newStatus,
      stage: payload.stage as string,
      summary: (payload.summary as string) ?? "",
      timeInStage: "0m",
      expanded: false,
      agentStatus: deriveAgentStatus(newStatus, newAgentName),
    };
    setBoardState("tasks", (tasks) => [...tasks, newTask]);
  }
}
