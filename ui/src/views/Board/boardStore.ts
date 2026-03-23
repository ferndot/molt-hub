/**
 * Board store — SolidJS createStore holding board state: stages and tasks
 * grouped by stage. Subscribes to WebSocket topic "board:*" for real-time
 * updates.
 */

import { createStore } from "solid-js/store";
import { subscribe } from "../../lib/ws";
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

export interface BoardState {
  stages: string[];
  tasks: BoardTask[];
}

// ---------------------------------------------------------------------------
// Mock data
// ---------------------------------------------------------------------------

const STAGES: string[] = [
  "backlog",
  "in-progress",
  "code-review",
  "testing",
  "deployed",
];

const MOCK_TASKS: Omit<BoardTask, "expanded">[] = [
  {
    id: "01HZAA0001",
    name: "Implement auth token refresh",
    agentName: "agent-alpha",
    priority: "p0",
    status: "running",
    stage: "in-progress",
    summary: "Implementing JWT refresh flow with sliding window expiry.",
    timeInStage: "2h 14m",
  },
  {
    id: "01HZAA0002",
    name: "Fix null pointer in pipeline executor",
    agentName: "agent-beta",
    priority: "p0",
    status: "blocked",
    stage: "in-progress",
    summary: "Awaiting upstream fix in core crate before proceeding.",
    timeInStage: "45m",
  },
  {
    id: "01HZAA0003",
    name: "Add retry logic to agent runner",
    agentName: "agent-gamma",
    priority: "p1",
    status: "waiting",
    stage: "code-review",
    summary: "PR opened — waiting for human review.",
    timeInStage: "1h 30m",
  },
  {
    id: "01HZAA0004",
    name: "Database migration for events table",
    agentName: "agent-delta",
    priority: "p1",
    status: "running",
    stage: "testing",
    summary: "Running integration tests against migration scripts.",
    timeInStage: "3h 02m",
  },
  {
    id: "01HZAA0005",
    name: "UI triage queue skeleton",
    agentName: "agent-epsilon",
    priority: "p2",
    status: "waiting",
    stage: "backlog",
    summary: "",
    timeInStage: "—",
  },
  {
    id: "01HZAA0006",
    name: "WebSocket reconnect hardening",
    agentName: "agent-zeta",
    priority: "p2",
    status: "complete",
    stage: "deployed",
    summary: "Exponential backoff + jitter implemented and deployed.",
    timeInStage: "8h 15m",
  },
  {
    id: "01HZAA0007",
    name: "Refactor instruction templating",
    agentName: "agent-eta",
    priority: "p3",
    status: "waiting",
    stage: "backlog",
    summary: "",
    timeInStage: "—",
  },
  {
    id: "01HZAA0008",
    name: "Transition rules engine tests",
    agentName: "agent-theta",
    priority: "p1",
    status: "running",
    stage: "code-review",
    summary: "Expanding edge case coverage for state machine transitions.",
    timeInStage: "55m",
  },
  {
    id: "01HZAA0009",
    name: "Deploy process supervisor v2",
    agentName: "agent-iota",
    priority: "p0",
    status: "complete",
    stage: "deployed",
    summary: "Supervisor v2 with health checks deployed to production.",
    timeInStage: "12h 40m",
  },
  {
    id: "01HZAA0010",
    name: "Add ULID serde support",
    agentName: "agent-kappa",
    priority: "p3",
    status: "waiting",
    stage: "backlog",
    summary: "",
    timeInStage: "—",
  },
];

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

const initialState: BoardState = {
  stages: STAGES,
  tasks: MOCK_TASKS.map((t) => ({ ...t, expanded: false })),
};

export const [boardState, setBoardState] =
  createStore<BoardState>(initialState);

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

export function sortByPriority(tasks: BoardTask[]): BoardTask[] {
  return [...tasks].sort(
    (a, b) => PRIORITY_ORDER[a.priority] - PRIORITY_ORDER[b.priority],
  );
}

export function tasksForStage(stage: string): BoardTask[] {
  return sortByPriority(boardState.tasks.filter((t) => t.stage === stage));
}

// ---------------------------------------------------------------------------
// WebSocket subscription (stub — real handler wired when server sends board events)
// ---------------------------------------------------------------------------

subscribe("board:*", (_msg) => {
  // TODO: handle real-time board update events from the server
  // e.g. task stage changes, new tasks, priority updates
});
