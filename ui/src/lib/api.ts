/**
 * API client — typed wrappers around fetch for backend endpoints.
 *
 * All methods return the parsed JSON body. The base path is `/api`,
 * which the Vite dev server proxies to the Rust backend at 127.0.0.1:3001.
 */

const BASE = "/api";

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

// ---------------------------------------------------------------------------
// Public API surface
// ---------------------------------------------------------------------------

export const api = {
  // Settings
  getSettings: () => get<Record<string, unknown>>("/settings"),
  updateSettings: (settings: unknown) =>
    put<Record<string, unknown>>("/settings", settings),

  // Pipeline
  getStages: () => get<{ stages: PipelineStage[] }>("/pipeline/stages"),
  updateStages: (stages: unknown) =>
    put<{ stages: PipelineStage[] }>("/pipeline/stages", stages),
  patchStage: (id: string, fields: Partial<PipelineStage>) =>
    patch<PipelineStage>(`/pipeline/stages/${id}`, fields),

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

  // Audit
  getAuditLog: (limit = 100) =>
    get<{ entries: AuditEntry[] }>(`/audit?limit=${limit}`),

  // GitHub Integration (OAuth)
  getGithubStatus: () =>
    get<GitHubStatus>("/integrations/github/status"),
  getGithubAuthUrl: () =>
    get<{ url: string }>("/integrations/github/auth"),
  disconnectGithub: () =>
    post<Record<string, unknown>>("/integrations/github/disconnect"),
};

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

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
}
