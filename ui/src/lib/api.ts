/**
 * API client — typed wrappers around fetch for backend endpoints.
 *
 * All methods return the parsed JSON body. The base path is `/api`,
 * which the Vite dev server proxies to the Rust backend at 127.0.0.1:13401.
 */

import { WORKSPACE_ID } from "./workspace";

const BASE = "/api";

/** `/projects/{workspace}/…` segment (boards and per-board pipeline). */
const WS_PROJECT = `/projects/${WORKSPACE_ID}`;

function jsonHeaders(): HeadersInit {
  return { "Content-Type": "application/json" };
}

async function readErrorDetail(res: Response): Promise<string | null> {
  const text = await res.text();
  if (!text.trim()) return null;
  try {
    const j = JSON.parse(text) as { error?: unknown };
    if (typeof j.error === "string" && j.error.length > 0) return j.error;
  } catch {
    /* not JSON */
  }
  return text.length > 240 ? `${text.slice(0, 240)}…` : text;
}

function formatFetchError(
  method: string,
  path: string,
  res: Response,
  detail: string | null,
): string {
  if (detail) {
    return `${method} ${path} failed: ${res.status} — ${detail}`;
  }
  if (res.status === 404) {
    return (
      `${method} ${path} failed: 404. ` +
      "The server has no handler for this path (often an outdated `molt-hub serve` on port 13401). " +
      "Stop it and restart from a fresh build: `cargo run --bin molt-hub -- serve` or `./dev.sh`."
    );
  }
  return `${method} ${path} failed: ${res.status}`;
}

async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`);
  if (!res.ok) {
    const detail = await readErrorDetail(res);
    throw new Error(formatFetchError("GET", path, res, detail));
  }
  return res.json() as Promise<T>;
}

async function put<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: "PUT",
    headers: jsonHeaders(),
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const detail = await readErrorDetail(res);
    throw new Error(formatFetchError("PUT", path, res, detail));
  }
  return res.json() as Promise<T>;
}

async function post<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: "POST",
    headers: jsonHeaders(),
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) {
    const detail = await readErrorDetail(res);
    throw new Error(formatFetchError("POST", path, res, detail));
  }
  return res.json() as Promise<T>;
}

async function patch<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: "PATCH",
    headers: jsonHeaders(),
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const detail = await readErrorDetail(res);
    throw new Error(formatFetchError("PATCH", path, res, detail));
  }
  return res.json() as Promise<T>;
}

async function del<T = void>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`, { method: "DELETE" });
  if (!res.ok) {
    const detail = await readErrorDetail(res);
    throw new Error(formatFetchError("DELETE", path, res, detail));
  }
  if (res.status === 204 || res.headers.get("content-length") === "0") {
    return undefined as T;
  }
  const ct = res.headers.get("content-type") ?? "";
  if (!ct.includes("application/json")) return undefined as T;
  return res.json() as Promise<T>;
}

/** Backend returns a JSON array; older clients expected `{ entries }`. */
function normalizeAuditRows(raw: unknown): AuditEntry[] {
  const list = Array.isArray(raw)
    ? raw
    : raw &&
        typeof raw === "object" &&
        Array.isArray((raw as { entries?: unknown }).entries)
      ? (raw as { entries: unknown[] }).entries
      : [];
  return list.map((row, index) => {
    if (!row || typeof row !== "object") {
      return {
        id: `invalid-${index}`,
        timestamp: "",
        action: "",
        actor: "",
        details: "",
      };
    }
    const r = row as Record<string, unknown>;
    const action = String(r.action ?? "");
    const timestamp =
      typeof r.timestamp === "string"
        ? r.timestamp
        : r.timestamp != null
          ? String(r.timestamp)
          : "";
    const actor =
      typeof r.actor === "string"
        ? r.actor
        : typeof r.actor_id === "string"
          ? r.actor_id
          : "";
    let details = "";
    const d = r.details;
    if (typeof d === "string") details = d;
    else if (d !== undefined && d !== null) {
      try {
        details = JSON.stringify(d);
      } catch {
        details = String(d);
      }
    }
    const id =
      typeof r.id === "string"
        ? r.id
        : `${timestamp}:${index}:${action}:${actor}`;
    return { id, timestamp, action, actor, details };
  });
}

// ---------------------------------------------------------------------------
// Public API surface
// ---------------------------------------------------------------------------

export const api = {
  // Settings
  getSettings: () => get<Record<string, unknown>>("/settings"),
  updateSettings: (settings: unknown) =>
    put<Record<string, unknown>>("/settings", settings),

  /** Named kanban boards (empty until you create one). */
  listBoards: () =>
    get<{ boards: BoardSummary[] }>(`${WS_PROJECT}/boards`),
  /** Default stages/columns applied to each new board (preview before create). */
  getBoardTemplate: () =>
    get<{ stages: PipelineStage[]; columns?: unknown }>(
      `${WS_PROJECT}/board-template`,
    ),
  createBoard: (body: { name: string }) =>
    post<{ boards: BoardSummary[]; boardId: string }>(
      `${WS_PROJECT}/boards`,
      body,
    ),
  deleteBoard: (boardId: string) =>
    del<{ boards: BoardSummary[] }>(
      `${WS_PROJECT}/boards/${encodeURIComponent(boardId)}`,
    ),
  getBoardStages: (boardId: string) =>
    get<{ stages: PipelineStage[] }>(
      `${WS_PROJECT}/boards/${encodeURIComponent(boardId)}/stages`,
    ),
  updateBoardStages: (boardId: string, body: unknown) =>
    put<{ stages: PipelineStage[] }>(
      `${WS_PROJECT}/boards/${encodeURIComponent(boardId)}/stages`,
      body,
    ),
  patchBoardStage: (
    boardId: string,
    stageId: string,
    fields: Partial<PipelineStage>,
  ) =>
    patch<PipelineStage>(
      `${WS_PROJECT}/boards/${encodeURIComponent(boardId)}/stages/${encodeURIComponent(stageId)}`,
      fields,
    ),

  /** All registered projects (YAML). First project’s `repo_path` is a typical cwd for agents. */
  listProjects: () => get<{ projects: ProjectSummary[] }>("/projects"),

  // Agents
  getAgents: () => get<AgentsListResponse>("/agents"),
  spawnAgent: (req: unknown) =>
    post<{ agentId: string; message?: string }>("/agents/spawn", req),
  terminateAgent: (id: string) =>
    post<Record<string, unknown>>(`/agents/${id}/terminate`),
  getAgentOutput: (id: string) =>
    get<{ lines: unknown[] }>(`/agents/${id}/output`),

  // Steering
  steerAgent: (id: string, message: string, priority: "normal" | "urgent" = "normal") =>
    post<Record<string, unknown>>(`/agents/${id}/steer`, { message, priority }),

  // Auth
  loginAgent: (adapterType = "claude") =>
    post<{ ok: boolean }>("/agents/login", { adapter_type: adapterType }),

  // Tasks
  getTasks: () => get<{ tasks: TaskSummary[] }>("/tasks"),
  /** Derive current board state for all tasks from the event store. */
  getBoardTasks: (boardId?: string) =>
    get<{ tasks: BoardTaskItem[] }>(
      boardId ? `/tasks/board?boardId=${encodeURIComponent(boardId)}` : "/tasks/board"
    ),
  /** Derive triage items (blocked or awaiting approval) from the event store. */
  getTriage: () => get<{ items: Array<{
    id: string; task_id: string; task_name: string; agent_name: string;
    stage: string; priority: string; type: string;
    created_at: string; summary: string;
  }> }>("/tasks/triage"),
  getTask: (id: string) =>
    get<TaskDetail>(`/tasks/${id}`),
  getTaskEvents: (id: string) =>
    get<{ events: TaskEvent[] }>(`/tasks/${id}/events`),
  createTask: (body: {
    title: string;
    description?: string;
    initialStage?: string;
    boardId?: string;
  }) => post<{ taskId: string }>("/tasks/create", body),

  /** Persisted kanban move + pipeline enter/exit hooks (requires active board id). */
  moveTask: (
    taskId: string,
    body: { toStage: string; boardId: string },
  ) =>
    post<{ taskId: string; stage: string; status: string }>(
      `/tasks/${encodeURIComponent(taskId)}/move`,
      body,
    ),

  /**
   * Human decision while task is awaiting approval (persists `HumanDecision`, runs hooks, WS update).
   */
  submitTaskHumanDecision: (
    taskId: string,
    body: {
      boardId: string;
      kind: "approved" | "rejected" | "redirected";
      reason?: string;
      toStage?: string;
      decidedBy?: string;
    },
  ) =>
    post<{ taskId: string; status: string }>(
      `/tasks/${encodeURIComponent(taskId)}/decision`,
      body,
    ),

  /** Uses the same harness as agents (Claude CLI by default; CLI adapter if set in settings). */
  suggestTaskTitle: (body: { text: string; adapterConfig?: unknown }) =>
    post<{ title: string; source: string }>("/agents/suggest-task-title", body),

  // Audit — server returns a JSON array; normalize to `{ entries }` for the UI.
  getAuditLog: async (limit = 100) => {
    const raw = await get<unknown>(`/audit?limit=${limit}`);
    return { entries: normalizeAuditRows(raw) };
  },

  // GitHub Integration (OAuth)
  getGithubStatus: () => get<GitHubStatus>("/integrations/github/status"),
  getGithubAuthUrl: () => get<{ url: string }>("/integrations/github/auth"),
  disconnectGithub: () =>
    post<Record<string, unknown>>("/integrations/github/disconnect"),
};

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

export interface BoardSummary {
  id: string;
  name: string;
}

/** Registered project with a repository root (used for agent working directories). */
export interface ProjectSummary {
  id: string;
  name: string;
  repo_path: string;
}

/** Serialized [`HookDefinition`](Rust) — extra keys are hook-specific config. */
export type HookDefinitionJson = Record<string, unknown> & {
  kind: string;
  on: string;
};

export interface PipelineStage {
  id: string;
  label: string;
  wip_limit: number | null;
  requires_approval: boolean;
  timeout_seconds: number | null;
  terminal: boolean;
  color: string | null;
  order: number;
  hooks?: HookDefinitionJson[];
}

export interface AuditEntry {
  id: string;
  timestamp: string;
  action: string;
  actor: string;
  details: string;
}

export interface TaskDetail {
  id: string;
  title: string;
  description: string;
  current_stage: string;
  priority: string;
  assigned_agent: string | null;
  agent_name: string | null;
  state_type: string;
  created_at: string;
  updated_at: string;
}

export interface TaskSummary {
  task_id: string;
  title: string | null;
  event_count: number;
  last_event_at: string | null;
}

/** Board-ready task projection returned by GET /api/tasks/board. */
export interface BoardTaskItem {
  task_id: string;
  name: string;
  stage: string;
  status: string;
  priority?: string;
  agent_name?: string | null;
  summary?: string;
  board_id?: string | null;
}

export interface AgentSummary {
  agent_id: string;
  task_id: string;
  status: string;
}

export interface AgentsListResponse {
  agents: AgentSummary[];
  count: number;
}

export interface TaskEvent {
  id: string;
  timestamp: string;
  event_type: string;
  actor: string;
  description: string;
}

export interface GitHubStatus {
  connected: boolean;
  owner?: string;
  scope?: string;
  /** When the server has GitHub App slug + credentials configured. */
  app_install_url?: string;
}
