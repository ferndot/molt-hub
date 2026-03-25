/**
 * Triage queue store — holds items requiring human attention.
 *
 * Actions emit decisions locally (removing items from the queue).
 * WebSocket integration with topic "triage:*" will be wired in a later task.
 */

import { createStore, produce } from "solid-js/store";
import { onCleanup } from "solid-js";
import { subscribe } from "../../lib/ws";
import { api } from "../../lib/api";
import { boardState } from "../Board/boardStore";
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
  items: [],
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

function requireActiveBoardId(): string {
  const id = boardState.activeBoardId?.trim() ?? "";
  if (!id) {
    throw new Error("Select a board on the workboard before acting on triage items.");
  }
  return id;
}

/** Approve a task awaiting human sign-off (persists `HumanDecision`, removes triage row). */
export async function approve(
  triageId: string,
  taskId: string,
): Promise<void> {
  await api.submitTaskHumanDecision(taskId, {
    boardId: requireActiveBoardId(),
    kind: "approved",
  });
  removeItem(triageId);
}

/** Reject back to in-progress (optional reason). */
export async function reject(
  triageId: string,
  taskId: string,
  reason: string,
): Promise<void> {
  await api.submitTaskHumanDecision(taskId, {
    boardId: requireActiveBoardId(),
    kind: "rejected",
    reason: reason.trim() || undefined,
  });
  removeItem(triageId);
}

/** Redirect to another pipeline stage while resolving the approval wait. */
export async function redirect(
  triageId: string,
  taskId: string,
  targetStage: string,
  reason?: string,
): Promise<void> {
  const to = targetStage.trim();
  if (!to) {
    throw new Error("Target stage is required");
  }
  await api.submitTaskHumanDecision(taskId, {
    boardId: requireActiveBoardId(),
    kind: "redirected",
    toStage: to,
    reason: reason?.trim() || undefined,
  });
  removeItem(triageId);
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
// HTTP hydration — populate triage on mount
// ---------------------------------------------------------------------------

export async function initTriage(): Promise<void> {
  try {
    const data = await api.getTriage();
    if (Array.isArray(data.items) && data.items.length > 0) {
      setState(produce((s) => {
        // Only add items not already present
        const existing = new Set(s.items.map(i => i.id));
        const newItems = data.items
          .filter(raw => !existing.has(raw.id))
          .map(raw => ({
            id: raw.id,
            taskId: raw.task_id,
            taskName: raw.task_name,
            agentName: raw.agent_name,
            stage: raw.stage,
            priority: raw.priority as Priority,
            type: raw.type as "decision" | "info",
            createdAt: raw.created_at,
            summary: raw.summary,
          }));
        s.items = sortItems([...s.items, ...newItems]);
      }));
    }
  } catch {
    // Silently ignore — triage still works via WebSocket
  }
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
      // Server sends `id` as task_id for some resolutions; match either key.
      const id = payload.id as string | undefined;
      const taskId = payload.task_id as string | undefined;
      if (id || taskId) {
        setState(
          produce((s) => {
            s.items = s.items.filter(
              (item) =>
                (id ? item.id !== id : true) &&
                (taskId ? item.taskId !== taskId : true),
            );
          }),
        );
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
