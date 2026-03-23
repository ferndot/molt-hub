/**
 * Task detail store — fetches and holds a single task's details and activity
 * timeline. Uses SolidJS createSignal for reactive loading/error/data states.
 */

import { createSignal } from "solid-js";
import { api } from "../../lib/api";
import type { TaskDetail, TaskEvent } from "../../lib/api";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const [task, setTask] = createSignal<TaskDetail | null>(null);
const [events, setEvents] = createSignal<TaskEvent[]>([]);
const [loading, setLoading] = createSignal(false);
const [error, setError] = createSignal<string | null>(null);

export { task, events, loading, error };

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/**
 * Fetch task details and its event timeline from the API.
 * Falls back gracefully on errors — sets the error signal.
 */
export async function loadTask(id: string): Promise<void> {
  setLoading(true);
  setError(null);
  setTask(null);
  setEvents([]);

  try {
    const [taskData, eventsData] = await Promise.all([
      api.getTask(id),
      api.getTaskEvents(id).catch(() => ({ events: [] })),
    ]);
    setTask(taskData);
    setEvents(eventsData.events);
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to load task";
    setError(message);
  } finally {
    setLoading(false);
  }
}

/**
 * Clear the store state (useful on unmount).
 */
export function clearTask(): void {
  setTask(null);
  setEvents([]);
  setLoading(false);
  setError(null);
}
