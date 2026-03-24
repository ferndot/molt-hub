/**
 * API client — typed wrappers around fetch for backend endpoints.
 *
 * All methods return the parsed JSON body. The base path is `/api`,
 * which the Vite dev server proxies to the Rust backend at 127.0.0.1:13401.
 */

const BASE = "/api";

/** Optional monitored-project ULID for per-project integration OAuth tokens. */
function integrationPath(path: string, projectId?: string): string {
  const id = projectId?.trim();
  if (!id || id === "default") return path;
  const sep = path.includes("?") ? "&" : "?";
  return `${path}${sep}projectId=${encodeURIComponent(id)}`;
}

function jsonHeaders(): HeadersInit {
  return { "Content-Type": "application/json" };
}

async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`);
  if (!res.ok) throw new Error(`GET ${path} failed: ${res.status}`);
  return res.json() as Promise<T>;
}

async function put<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: "PUT",
    headers: jsonHeaders(),
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`PUT ${path} failed: ${res.status}`);
  return res.json() as Promise<T>;
}

async function post<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: "POST",
    headers: jsonHeaders(),
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw new Error(`POST ${path} failed: ${res.status}`);
  return res.json() as Promise<T>;
}

async function patch<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: "PATCH",
    headers: jsonHeaders(),
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`PATCH ${path} failed: ${res.status}`);
  return res.json() as Promise<T>;
}

async function del<T = void>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`, { method: "DELETE" });
  if (!res.ok) throw new Error(`DELETE ${path} failed: ${res.status}`);
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

export const projectApi = {
  agents: (pid: string) => `/api/projects/${pid}/agents`,
  pipelineStages: (pid: string) => `/api/projects/${pid}/pipeline/stages`,
};

export const api = {
  // Settings
  getSettings: () => get<Record<string, unknown>>("/settings"),
  updateSettings: (settings: unknown) =>
    put<Record<string, unknown>>("/settings", settings),

  // Pipeline (legacy global routes — prefer project-scoped for multi-tenant UI)
  getStages: () => get<{ stages: PipelineStage[] }>("/pipeline/stages"),
  updateStages: (stages: unknown) =>
    put<{ stages: PipelineStage[] }>("/pipeline/stages", stages),
  patchStage: (id: string, fields: Partial<PipelineStage>) =>
    patch<PipelineStage>(`/pipeline/stages/${id}`, fields),

  /** Per-project pipeline (matches `/api/projects/:pid/pipeline/...` on the server). */
  getProjectPipelineStages: (projectId: string) =>
    get<{ stages: PipelineStage[] }>(
      `/projects/${projectId}/pipeline/stages`,
    ),
  updateProjectPipelineStages: (projectId: string, body: unknown) =>
    put<{ stages: PipelineStage[] }>(
      `/projects/${projectId}/pipeline/stages`,
      body,
    ),
  patchProjectPipelineStage: (
    projectId: string,
    stageId: string,
    fields: Partial<PipelineStage>,
  ) =>
    patch<PipelineStage>(
      `/projects/${projectId}/pipeline/stages/${stageId}`,
      fields,
    ),

  /** Named boards per project (`default` is always present). */
  listProjectBoards: (projectId: string) =>
    get<{ boards: BoardSummary[] }>(`/projects/${projectId}/boards`),
  createProjectBoard: (projectId: string, body: { id: string; name?: string }) =>
    post<{ boards: BoardSummary[] }>(`/projects/${projectId}/boards`, body),
  deleteProjectBoard: (projectId: string, boardId: string) =>
    del<{ boards: BoardSummary[] }>(
      `/projects/${projectId}/boards/${encodeURIComponent(boardId)}`,
    ),
  getProjectBoardStages: (projectId: string, boardId: string) =>
    get<{ stages: PipelineStage[] }>(
      `/projects/${projectId}/boards/${encodeURIComponent(boardId)}/stages`,
    ),
  updateProjectBoardStages: (
    projectId: string,
    boardId: string,
    body: unknown,
  ) =>
    put<{ stages: PipelineStage[] }>(
      `/projects/${projectId}/boards/${encodeURIComponent(boardId)}/stages`,
      body,
    ),
  patchProjectBoardStage: (
    projectId: string,
    boardId: string,
    stageId: string,
    fields: Partial<PipelineStage>,
  ) =>
    patch<PipelineStage>(
      `/projects/${projectId}/boards/${encodeURIComponent(boardId)}/stages/${encodeURIComponent(stageId)}`,
      fields,
    ),

  // Agents
  getAgents: () => get<AgentsListResponse>("/agents"),
  spawnAgent: (req: unknown) =>
    post<Record<string, unknown>>("/agents/spawn", req),
  terminateAgent: (id: string) =>
    post<Record<string, unknown>>(`/agents/${id}/terminate`),
  getAgentOutput: (id: string) =>
    get<{ lines: unknown[] }>(`/agents/${id}/output`),

  // Steering
  steerAgent: (id: string, message: string, priority: "normal" | "urgent" = "normal") =>
    post<Record<string, unknown>>(`/agents/${id}/steer`, { message, priority }),

  // Approval
  approveAgent: (id: string) =>
    post<Record<string, unknown>>(`/agents/${id}/approve`),
  rejectAgent: (id: string, reason: string) =>
    post<Record<string, unknown>>(`/agents/${id}/reject`, { reason }),

  // Tasks
  getTasks: () => get<{ tasks: TaskSummary[] }>("/tasks"),
  getTask: (id: string) =>
    get<TaskDetail>(`/tasks/${id}`),
  getTaskEvents: (id: string) =>
    get<{ events: TaskEvent[] }>(`/tasks/${id}/events`),

  // Audit — server returns a JSON array; normalize to `{ entries }` for the UI.
  getAuditLog: async (limit = 100) => {
    const raw = await get<unknown>(`/audit?limit=${limit}`);
    return { entries: normalizeAuditRows(raw) };
  },

  // GitHub Integration (OAuth)
  getGithubStatus: (projectId?: string) =>
    get<GitHubStatus>(
      integrationPath("/integrations/github/status", projectId),
    ),
  getGithubAuthUrl: (projectId?: string) =>
    get<{ url: string }>(
      integrationPath("/integrations/github/auth", projectId),
    ),
  disconnectGithub: (projectId?: string) =>
    post<Record<string, unknown>>(
      integrationPath("/integrations/github/disconnect", projectId),
    ),
};

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

export interface BoardSummary {
  id: string;
  name: string;
}

export interface PipelineStage {
  id: string;
  label: string;
  wip_limit: number | null;
  requires_approval: boolean;
  timeout_seconds: number | null;
  terminal: boolean;
  color: string | null;
  order: number;
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
