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

// ---------------------------------------------------------------------------
// Public API surface
// ---------------------------------------------------------------------------

export const api = {
  // Settings
  getSettings: () => get<Record<string, unknown>>("/settings"),
  updateSettings: (settings: unknown) =>
    put<Record<string, unknown>>("/settings", settings),

  // Pipeline
  getStages: () => get<{ stages: unknown[] }>("/pipeline/stages"),
  updateStages: (stages: unknown) =>
    put<{ stages: unknown[] }>("/pipeline/stages", stages),

  // Agents
  getAgents: () => get<{ agents: unknown[] }>("/agents"),
  spawnAgent: (req: unknown) =>
    post<Record<string, unknown>>("/agents/spawn", req),
  terminateAgent: (id: string) =>
    post<Record<string, unknown>>(`/agents/${id}/terminate`),
  getAgentOutput: (id: string) =>
    get<{ lines: unknown[] }>(`/agents/${id}/output`),

  // Approval
  approveAgent: (id: string) =>
    post<Record<string, unknown>>(`/agents/${id}/approve`),
  rejectAgent: (id: string, reason: string) =>
    post<Record<string, unknown>>(`/agents/${id}/reject`, { reason }),

  // Audit
  getAuditLog: (limit = 100) =>
    get<{ entries: AuditEntry[] }>(`/audit?limit=${limit}`),
};

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

export interface AuditEntry {
  id: string;
  timestamp: string;
  action: string;
  actor: string;
  details: string;
}
