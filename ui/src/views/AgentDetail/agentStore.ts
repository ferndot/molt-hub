/**
 * Agent detail store — holds state for a single agent's detail view.
 *
 * Provides mock data for 4-5 agents, a getAgent() selector, and a
 * WebSocket subscription stub for topic `agent:{id}`.
 */

import { createStore } from "solid-js/store";
import { subscribe } from "../../lib/ws";
import { api } from "../../lib/api";
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
// Mock data helpers
// ---------------------------------------------------------------------------

function ts(offsetMs: number): string {
  return new Date(Date.now() - offsetMs).toISOString();
}

function outputTs(offsetMs: number): string {
  return new Date(Date.now() - offsetMs).toLocaleTimeString("en-US", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

// ---------------------------------------------------------------------------
// Mock agents
// ---------------------------------------------------------------------------

const MOCK_AGENTS: AgentDetail[] = [
  {
    id: "agent-001",
    name: "frontend-agent-1",
    taskName: "Add OAuth2 login flow",
    taskDescription:
      "Implement PKCE-based OAuth2 flow with refresh token rotation. Integrate with existing session management. Add unit and integration tests.",
    currentStage: "code-review",
    stageHistory: [
      { stage: "planning", enteredAt: ts(3 * 60 * 60 * 1000) },
      { stage: "testing", enteredAt: ts(90 * 60 * 1000) },
      { stage: "code-review", enteredAt: ts(45 * 60 * 1000) },
    ],
    status: "running",
    priority: "p0",
    assignedAt: ts(3 * 60 * 60 * 1000),
    outputLines: [
      { timestamp: outputTs(44 * 60 * 1000), text: "Starting OAuth2 implementation analysis..." },
      { timestamp: outputTs(43 * 60 * 1000), text: "Reading existing auth middleware at src/middleware/auth.ts" },
      { timestamp: outputTs(42 * 60 * 1000), text: "Identified 3 integration points for PKCE flow" },
      { timestamp: outputTs(40 * 60 * 1000), text: "Writing OAuthProvider class..." },
      { timestamp: outputTs(38 * 60 * 1000), text: "Implementing generateCodeVerifier() and generateCodeChallenge()" },
      { timestamp: outputTs(35 * 60 * 1000), text: "Running unit tests: pkce.test.ts..." },
      { timestamp: outputTs(34 * 60 * 1000), text: "  PASS pkce.test.ts (12 tests, 0 failures)" },
      { timestamp: outputTs(30 * 60 * 1000), text: "Implementing token refresh rotation..." },
      { timestamp: outputTs(28 * 60 * 1000), text: "Writing integration tests for /auth/callback endpoint" },
      { timestamp: outputTs(25 * 60 * 1000), text: "  PASS auth-callback.test.ts (8 tests, 0 failures)" },
      { timestamp: outputTs(20 * 60 * 1000), text: "Running full test suite..." },
      { timestamp: outputTs(19 * 60 * 1000), text: "  PASS — 3 new endpoints, 12 unit tests" },
      { timestamp: outputTs(15 * 60 * 1000), text: "Transitioning to code-review stage" },
      { timestamp: outputTs(14 * 60 * 1000), text: "Ready for human review. Diff: +342 -18 lines" },
    ],
  },
  {
    id: "agent-002",
    name: "backend-agent-3",
    taskName: "Migrate user table to Postgres",
    taskDescription:
      "Online schema migration for the users table. Add 3 new indexes, backfill denormalized columns. Zero downtime required.",
    currentStage: "deployment",
    stageHistory: [
      { stage: "planning", enteredAt: ts(5 * 60 * 60 * 1000) },
      { stage: "testing", enteredAt: ts(3 * 60 * 60 * 1000) },
      { stage: "integration", enteredAt: ts(2.5 * 60 * 60 * 1000) },
      { stage: "deployment", enteredAt: ts(2 * 60 * 60 * 1000) },
    ],
    status: "paused",
    priority: "p0",
    assignedAt: ts(5 * 60 * 60 * 1000),
    outputLines: [
      { timestamp: outputTs(4 * 60 * 60 * 1000), text: "Analyzing users table schema..." },
      { timestamp: outputTs(3.5 * 60 * 60 * 1000), text: "Generating migration script via pg-migrate..." },
      { timestamp: outputTs(3 * 60 * 60 * 1000), text: "Dry-run on staging DB: estimated 0s downtime" },
      { timestamp: outputTs(2.8 * 60 * 60 * 1000), text: "Adding index: users_email_idx (CONCURRENTLY)" },
      { timestamp: outputTs(2.5 * 60 * 60 * 1000), text: "Adding index: users_created_at_idx (CONCURRENTLY)" },
      { timestamp: outputTs(2.2 * 60 * 60 * 1000), text: "Backfill: users.display_name from first_name + last_name" },
      { timestamp: outputTs(2 * 60 * 60 * 1000), text: "Migration ready. Awaiting human approval for production." },
      { timestamp: outputTs(2 * 60 * 60 * 1000 - 1000), text: "[PAUSED] Waiting for operator decision..." },
    ],
  },
  {
    id: "agent-003",
    name: "backend-agent-1",
    taskName: "Refactor billing service",
    taskDescription:
      "Extract billing logic from the monolith into a dedicated BillingService module. Maintain backwards compatibility with existing API contracts.",
    currentStage: "testing",
    stageHistory: [
      { stage: "planning", enteredAt: ts(2 * 60 * 60 * 1000) },
      { stage: "testing", enteredAt: ts(30 * 60 * 1000) },
    ],
    status: "running",
    priority: "p1",
    assignedAt: ts(2 * 60 * 60 * 1000),
    outputLines: [
      { timestamp: outputTs(90 * 60 * 1000), text: "Starting billing service extraction..." },
      { timestamp: outputTs(85 * 60 * 1000), text: "Mapping 23 billing-related functions across 5 files" },
      { timestamp: outputTs(80 * 60 * 1000), text: "Creating BillingService module skeleton" },
      { timestamp: outputTs(70 * 60 * 1000), text: "Migrating invoiceGenerator.ts..." },
      { timestamp: outputTs(60 * 60 * 1000), text: "Migrating subscriptionManager.ts..." },
      { timestamp: outputTs(50 * 60 * 1000), text: "Migrating paymentProcessor.ts..." },
      { timestamp: outputTs(40 * 60 * 1000), text: "Running unit tests: coverage at 84%" },
      { timestamp: outputTs(35 * 60 * 1000), text: "  2 integration tests failing (Stripe sandbox unavailable)" },
      { timestamp: outputTs(30 * 60 * 1000), text: "Awaiting decision: proceed with failing Stripe tests?" },
    ],
  },
  {
    id: "agent-004",
    name: "backend-agent-2",
    taskName: "Update API rate limiting",
    taskDescription:
      "Switch rate limiting algorithm from token bucket to sliding window. Configure per-user and global limits.",
    currentStage: "code-review",
    stageHistory: [
      { stage: "planning", enteredAt: ts(60 * 60 * 1000) },
      { stage: "code-review", enteredAt: ts(15 * 60 * 1000) },
    ],
    status: "running",
    priority: "p1",
    assignedAt: ts(60 * 60 * 1000),
    outputLines: [
      { timestamp: outputTs(55 * 60 * 1000), text: "Analyzing current rate limiter implementation..." },
      { timestamp: outputTs(50 * 60 * 1000), text: "Token bucket found in src/middleware/rateLimit.ts" },
      { timestamp: outputTs(45 * 60 * 1000), text: "Implementing SlidingWindowLimiter class..." },
      { timestamp: outputTs(40 * 60 * 1000), text: "Configuring: 100 req/min per user, 10k req/min global" },
      { timestamp: outputTs(30 * 60 * 1000), text: "Writing benchmarks: sliding window vs token bucket" },
      { timestamp: outputTs(25 * 60 * 1000), text: "  Benchmark result: sliding window 12% more accurate" },
      { timestamp: outputTs(20 * 60 * 1000), text: "All tests passing (24 tests)" },
      { timestamp: outputTs(15 * 60 * 1000), text: "Ready for code review. Config change needs approval." },
    ],
  },
  {
    id: "agent-005",
    name: "docs-agent-1",
    taskName: "Generate API documentation",
    taskDescription:
      "Auto-generate OpenAPI 3.0 spec from code annotations. Publish to /docs endpoint with Swagger UI.",
    currentStage: "documentation",
    stageHistory: [
      { stage: "planning", enteredAt: ts(4 * 60 * 60 * 1000) },
      { stage: "documentation", enteredAt: ts(3 * 60 * 60 * 1000) },
    ],
    status: "idle",
    priority: "p3",
    assignedAt: ts(4 * 60 * 60 * 1000),
    outputLines: [
      { timestamp: outputTs(3.5 * 60 * 60 * 1000), text: "Scanning codebase for JSDoc/OpenAPI annotations..." },
      { timestamp: outputTs(3.3 * 60 * 60 * 1000), text: "Found 47 annotated endpoints across 8 route files" },
      { timestamp: outputTs(3.1 * 60 * 60 * 1000), text: "Generating openapi.json..." },
      { timestamp: outputTs(3 * 60 * 60 * 1000), text: "Validating spec against OpenAPI 3.0 schema..." },
      { timestamp: outputTs(2.9 * 60 * 60 * 1000), text: "  Validation: PASSED (0 errors, 3 warnings)" },
      { timestamp: outputTs(2.8 * 60 * 60 * 1000), text: "Publishing to /docs with Swagger UI 5.x" },
      { timestamp: outputTs(2.7 * 60 * 60 * 1000), text: "Done. 47 endpoints documented. Published to /docs" },
    ],
  },
];

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export interface AgentDetailState {
  agents: AgentDetail[];
  activeId: string | null;
}

const [state, setState] = createStore<AgentDetailState>({
  agents: MOCK_AGENTS,
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

interface ApiAgent {
  id: string;
  name: string;
  task_name?: string;
  task_description?: string;
  current_stage?: string;
  status?: string;
  priority?: string;
  assigned_at?: string;
}

function mapApiAgent(a: ApiAgent): AgentDetail {
  return {
    id: a.id,
    name: a.name,
    taskName: a.task_name ?? "",
    taskDescription: a.task_description ?? "",
    currentStage: a.current_stage ?? "unknown",
    stageHistory: [],
    status: (a.status as AgentDetail["status"]) ?? "idle",
    priority: (a.priority as Priority) ?? "p2",
    assignedAt: a.assigned_at ?? new Date().toISOString(),
    outputLines: [],
  };
}

/**
 * Fetch agents from the backend and merge into the store.
 * Falls back silently to existing mock data if the API is unreachable.
 */
export async function fetchAgents(): Promise<void> {
  try {
    const data = await api.getAgents();
    const agents = (data.agents as ApiAgent[]) ?? [];
    if (agents.length > 0) {
      setState("agents", agents.map(mapApiAgent));
    }
  } catch {
    // Keep existing mock data
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
