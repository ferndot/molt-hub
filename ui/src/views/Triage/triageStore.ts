/**
 * Triage queue store — holds items requiring human attention.
 *
 * Actions emit decisions locally (removing items from the queue).
 * WebSocket integration with topic "triage:*" will be wired in a later task.
 */

import { createStore, produce } from "solid-js/store";
import { onCleanup } from "solid-js";
import { subscribe } from "../../lib/ws";
import type { Priority } from "../../types/domain";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface TriageItem {
  id: string;
  taskId: string;
  taskName: string;
  agentName: string;
  /** Current pipeline stage */
  stage: string;
  priority: Priority;
  /** "decision" = needs approval/rejection; "info" = informational acknowledgement */
  type: "decision" | "info";
  createdAt: string;
  /** Agent output summary */
  summary: string;
}

// ---------------------------------------------------------------------------
// Sort helpers
// ---------------------------------------------------------------------------

const PRIORITY_ORDER: Record<Priority, number> = {
  p0: 0,
  p1: 1,
  p2: 2,
  p3: 3,
};

function sortItems(items: TriageItem[]): TriageItem[] {
  return [...items].sort((a, b) => {
    const pd = PRIORITY_ORDER[a.priority] - PRIORITY_ORDER[b.priority];
    if (pd !== 0) return pd;
    return new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime();
  });
}

// ---------------------------------------------------------------------------
// Mock data
// ---------------------------------------------------------------------------

const MOCK_ITEMS: TriageItem[] = [
  {
    id: "ti-001",
    taskId: "01HTASK0000000000000000001",
    taskName: "Add OAuth2 login flow",
    agentName: "frontend-agent-1",
    stage: "code-review",
    priority: "p0",
    type: "decision",
    createdAt: new Date(Date.now() - 45 * 60 * 1000).toISOString(),
    summary:
      "Implemented OAuth2 PKCE flow with refresh token rotation. 3 new endpoints, 12 unit tests passing. Ready for human code review before merging to main.",
  },
  {
    id: "ti-002",
    taskId: "01HTASK0000000000000000002",
    taskName: "Migrate user table to Postgres",
    agentName: "backend-agent-3",
    stage: "deployment",
    priority: "p0",
    type: "decision",
    createdAt: new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString(),
    summary:
      "Schema migration script ready. Will add 3 indexes. Estimated downtime: 0s (online migration). Requires approval before applying to production.",
  },
  {
    id: "ti-003",
    taskId: "01HTASK0000000000000000003",
    taskName: "Refactor billing service",
    agentName: "backend-agent-1",
    stage: "testing",
    priority: "p1",
    type: "decision",
    createdAt: new Date(Date.now() - 30 * 60 * 1000).toISOString(),
    summary:
      "Extracted billing logic into separate module. Test coverage at 84%. 2 integration tests failing due to Stripe sandbox — awaiting human decision on whether to proceed.",
  },
  {
    id: "ti-004",
    taskId: "01HTASK0000000000000000004",
    taskName: "Update API rate limiting",
    agentName: "backend-agent-2",
    stage: "code-review",
    priority: "p1",
    type: "decision",
    createdAt: new Date(Date.now() - 15 * 60 * 1000).toISOString(),
    summary:
      "Switched from token bucket to sliding window algorithm. Limits: 100 req/min per user, 10k req/min global. Config change requires approval.",
  },
  {
    id: "ti-005",
    taskId: "01HTASK0000000000000000005",
    taskName: "Bump lodash to 4.17.21",
    agentName: "deps-agent-1",
    stage: "dependency-update",
    priority: "p2",
    type: "info",
    createdAt: new Date(Date.now() - 90 * 60 * 1000).toISOString(),
    summary:
      "Automated dependency update. No breaking changes detected. All tests pass. Changelog reviewed.",
  },
  {
    id: "ti-006",
    taskId: "01HTASK0000000000000000006",
    taskName: "Generate API documentation",
    agentName: "docs-agent-1",
    stage: "documentation",
    priority: "p3",
    type: "info",
    createdAt: new Date(Date.now() - 3 * 60 * 60 * 1000).toISOString(),
    summary:
      "OpenAPI spec generated from code annotations. 47 endpoints documented. Published to /docs.",
  },
  {
    id: "ti-007",
    taskId: "01HTASK0000000000000000007",
    taskName: "Add dark mode support",
    agentName: "frontend-agent-2",
    stage: "testing",
    priority: "p2",
    type: "info",
    createdAt: new Date(Date.now() - 20 * 60 * 1000).toISOString(),
    summary:
      "CSS variables refactored for theme support. Light/dark toggle implemented. Visual regression tests added.",
  },
];

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export type FilterMode = "all" | "needs-action" | "by-agent";
export type SortMode = "priority" | "time-waiting" | "by-agent";

interface TriageStoreState {
  items: TriageItem[];
  filterMode: FilterMode;
  sortMode: SortMode;
}

const [state, setState] = createStore<TriageStoreState>({
  items: sortItems(MOCK_ITEMS),
  filterMode: "all",
  sortMode: "priority",
});

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

function removeItem(id: string): void {
  setState(
    produce((s) => {
      s.items = s.items.filter((item) => item.id !== id);
    }),
  );
}

export function approve(id: string): void {
  removeItem(id);
}

export function reject(id: string, _reason: string): void {
  removeItem(id);
}

export function redirect(id: string, _targetStage: string): void {
  removeItem(id);
}

export function defer(id: string): void {
  setState(
    produce((s) => {
      const idx = s.items.findIndex((item) => item.id === id);
      if (idx === -1) return;
      const [item] = s.items.splice(idx, 1);
      s.items.push(item);
    }),
  );
}

export function acknowledge(id: string): void {
  removeItem(id);
}

export function setFilterMode(mode: FilterMode): void {
  setState("filterMode", mode);
}

export function setSortMode(mode: SortMode): void {
  setState("sortMode", mode);
}

// ---------------------------------------------------------------------------
// WebSocket subscription (stub — wired for future real-time updates)
// ---------------------------------------------------------------------------

export function setupTriageSubscription(): () => void {
  const unsubscribe = subscribe("triage:*", (msg) => {
    if (msg.type !== "event") return;
    const payload = msg.payload as Record<string, unknown>;

    // Determine the sub-topic from the full topic string (e.g. "triage:new")
    const topic = (msg as { topic?: string }).topic ?? "";

    if (topic === "triage:new" || topic === "triage:item") {
      // Add a new triage item from the server
      const item: TriageItem = {
        id: (payload.id as string) ?? "",
        taskId: (payload.task_id as string) ?? "",
        taskName: (payload.task_name as string) ?? "",
        agentName: (payload.agent_name as string) ?? "",
        stage: (payload.stage as string) ?? "",
        priority: (payload.priority as Priority) ?? "p2",
        type: (payload.type as "decision" | "info") ?? "info",
        createdAt: (payload.created_at as string) ?? new Date().toISOString(),
        summary: (payload.summary as string) ?? "",
      };
      // Only add if not already present
      if (!state.items.find((i) => i.id === item.id)) {
        setState(
          produce((s) => {
            s.items = sortItems([...s.items, item]);
          }),
        );
      }
    } else if (topic === "triage:resolved" || topic === "triage:removed") {
      // Remove a triage item that was resolved elsewhere
      const id = payload.id as string | undefined;
      if (id) {
        removeItem(id);
      }
    }
  });
  return unsubscribe;
}

// ---------------------------------------------------------------------------
// Derived / selectors
// ---------------------------------------------------------------------------

export function getFilteredItems(
  items: TriageItem[],
  filterMode: FilterMode,
  sortMode: SortMode,
): TriageItem[] {
  let filtered = items;

  if (filterMode === "needs-action") {
    filtered = items.filter(
      (item) => item.priority === "p0" || item.priority === "p1",
    );
  }

  let sorted = [...filtered];

  if (sortMode === "priority") {
    sorted = sorted.sort((a, b) => {
      const pd = PRIORITY_ORDER[a.priority] - PRIORITY_ORDER[b.priority];
      if (pd !== 0) return pd;
      return (
        new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime()
      );
    });
  } else if (sortMode === "time-waiting") {
    sorted = sorted.sort(
      (a, b) =>
        new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime(),
    );
  } else if (sortMode === "by-agent") {
    sorted = sorted.sort((a, b) => a.agentName.localeCompare(b.agentName));
  }

  return sorted;
}

/** Read-only access to the store state. */
export function useTriageStore() {
  return { state };
}
