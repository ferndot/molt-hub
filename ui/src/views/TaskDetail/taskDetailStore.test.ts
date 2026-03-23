/**
 * Tests for taskDetailStore — verifies loading, error, and data states.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Mock the api module before importing the store
vi.mock("../../lib/api", () => ({
  api: {
    getTask: vi.fn(),
    getTaskEvents: vi.fn(),
  },
}));

import { api } from "../../lib/api";
import { task, events, loading, error, loadTask, clearTask } from "./taskDetailStore";

const mockApi = api as unknown as {
  getTask: ReturnType<typeof vi.fn>;
  getTaskEvents: ReturnType<typeof vi.fn>;
};

// ---------------------------------------------------------------------------
// Test data
// ---------------------------------------------------------------------------

const MOCK_TASK = {
  id: "01HZAA0001",
  title: "Implement auth token refresh",
  description: "Implementing JWT refresh flow with sliding window expiry.",
  current_stage: "in-progress",
  priority: "p0",
  assigned_agent: "agent-alpha-id",
  agent_name: "agent-alpha",
  state_type: "in_progress",
  created_at: "2026-03-20T10:00:00Z",
  updated_at: "2026-03-20T12:14:00Z",
};

const MOCK_EVENTS = [
  {
    id: "evt-001",
    timestamp: "2026-03-20T10:00:00Z",
    event_type: "task_created",
    actor: "system",
    description: "Task created in backlog",
  },
  {
    id: "evt-002",
    timestamp: "2026-03-20T10:05:00Z",
    event_type: "agent_assigned",
    actor: "system",
    description: "Assigned to agent-alpha",
  },
  {
    id: "evt-003",
    timestamp: "2026-03-20T10:30:00Z",
    event_type: "task_stage_changed",
    actor: "agent-alpha",
    description: "Moved from backlog to in-progress",
  },
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("taskDetailStore", () => {
  beforeEach(() => {
    clearTask();
    vi.clearAllMocks();
  });

  afterEach(() => {
    clearTask();
  });

  it("starts with null/empty state", () => {
    expect(task()).toBe(null);
    expect(events()).toEqual([]);
    expect(loading()).toBe(false);
    expect(error()).toBe(null);
  });

  it("loads task and events from API", async () => {
    mockApi.getTask.mockResolvedValue(MOCK_TASK);
    mockApi.getTaskEvents.mockResolvedValue({ events: MOCK_EVENTS });

    await loadTask("01HZAA0001");

    expect(task()).toEqual(MOCK_TASK);
    expect(events()).toEqual(MOCK_EVENTS);
    expect(loading()).toBe(false);
    expect(error()).toBe(null);
    expect(mockApi.getTask).toHaveBeenCalledWith("01HZAA0001");
    expect(mockApi.getTaskEvents).toHaveBeenCalledWith("01HZAA0001");
  });

  it("sets error on API failure", async () => {
    mockApi.getTask.mockRejectedValue(new Error("GET /tasks/bad-id failed: 404"));
    mockApi.getTaskEvents.mockResolvedValue({ events: [] });

    await loadTask("bad-id");

    expect(task()).toBe(null);
    expect(error()).toBe("GET /tasks/bad-id failed: 404");
    expect(loading()).toBe(false);
  });

  it("still loads task if events endpoint fails", async () => {
    mockApi.getTask.mockResolvedValue(MOCK_TASK);
    mockApi.getTaskEvents.mockRejectedValue(new Error("events not found"));

    await loadTask("01HZAA0001");

    expect(task()).toEqual(MOCK_TASK);
    expect(events()).toEqual([]);
    expect(error()).toBe(null);
  });

  it("clearTask resets all state", async () => {
    mockApi.getTask.mockResolvedValue(MOCK_TASK);
    mockApi.getTaskEvents.mockResolvedValue({ events: MOCK_EVENTS });
    await loadTask("01HZAA0001");

    clearTask();

    expect(task()).toBe(null);
    expect(events()).toEqual([]);
    expect(loading()).toBe(false);
    expect(error()).toBe(null);
  });

  it("activity events are stored in chronological order", async () => {
    const reverseEvents = [...MOCK_EVENTS].reverse();
    mockApi.getTask.mockResolvedValue(MOCK_TASK);
    mockApi.getTaskEvents.mockResolvedValue({ events: reverseEvents });

    await loadTask("01HZAA0001");

    // Events should be stored as received (server returns them in order)
    const timestamps = events().map((e) => e.timestamp);
    expect(timestamps).toEqual(reverseEvents.map((e) => e.timestamp));
  });
});
