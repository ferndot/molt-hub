/**
 * Tests for agentListUtils API integration — fetchAgents, initAgents,
 * refreshAgents, and the mapApiAgent mapping function.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

describe("agentListUtils API integration", () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    globalThis.fetch = mockFetch;
    mockFetch.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // --------------------------------------------------------------------------
  // mapApiAgent
  // --------------------------------------------------------------------------

  describe("mapApiAgent", () => {
    it("maps a Running agent correctly", async () => {
      const { mapApiAgent } = await import("../agentListUtils");
      const result = mapApiAgent({
        agent_id: "01HZBB0001ABCDEF12345678",
        name: "test-agent",
        task_id: "task-001",
        status: "Running",
      });
      expect(result.id).toBe("01HZBB0001ABCDEF12345678");
      expect(result.status).toBe("running");
      expect(result.stage).toBe("in-progress");
      expect(result.name).toBe("test-agent");
    });

    it("falls back to agent_id prefix when name is empty", async () => {
      const { mapApiAgent } = await import("../agentListUtils");
      const result = mapApiAgent({
        agent_id: "01HZBB0001ABCDEF12345678",
        name: "",
        task_id: "task-001",
        status: "Running",
      });
      expect(result.name).toBe("01HZBB00");
    });

    it("maps unknown status to idle", async () => {
      const { mapApiAgent } = await import("../agentListUtils");
      const result = mapApiAgent({
        agent_id: "abc12345",
        name: "agent-abc",
        task_id: "t1",
        status: "SomeNewStatus",
      });
      expect(result.status).toBe("idle");
    });

    it("maps Waiting status to paused", async () => {
      const { mapApiAgent } = await import("../agentListUtils");
      const result = mapApiAgent({
        agent_id: "abc12345",
        name: "agent-abc",
        task_id: "t1",
        status: "Waiting",
      });
      expect(result.status).toBe("paused");
    });

    it("maps Failed status to terminated", async () => {
      const { mapApiAgent } = await import("../agentListUtils");
      const result = mapApiAgent({
        agent_id: "abc12345",
        name: "agent-abc",
        task_id: "t1",
        status: "Failed",
      });
      expect(result.status).toBe("terminated");
    });

    it("maps Stopped status to terminated", async () => {
      const { mapApiAgent } = await import("../agentListUtils");
      const result = mapApiAgent({
        agent_id: "abc12345",
        name: "agent-abc",
        task_id: "t1",
        status: "Stopped",
      });
      expect(result.status).toBe("terminated");
    });
  });

  // --------------------------------------------------------------------------
  // fetchAgents
  // --------------------------------------------------------------------------

  describe("fetchAgents", () => {
    it("returns mapped agents on success", async () => {
      const agents = [
        { agent_id: "a1", name: "agent-a1", task_id: "t1", status: "Running" },
        { agent_id: "a2", name: "agent-a2", task_id: "t2", status: "Idle" },
      ];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ agents, count: 2 }),
      });

      const { fetchAgents } = await import("../agentListUtils");
      const result = await fetchAgents();
      expect(result).toHaveLength(2);
      expect(result![0].id).toBe("a1");
      expect(result![0].status).toBe("running");
      expect(result![1].id).toBe("a2");
      expect(result![1].status).toBe("idle");
    });

    it("returns null on network error", async () => {
      mockFetch.mockRejectedValueOnce(new Error("Network error"));

      const { fetchAgents } = await import("../agentListUtils");
      const result = await fetchAgents();
      expect(result).toBeNull();
    });

    it("returns empty array when agents array is empty", async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ agents: [], count: 0 }),
      });

      const { fetchAgents } = await import("../agentListUtils");
      const result = await fetchAgents();
      expect(result).toEqual([]);
    });

    it("returns null on non-ok response", async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 500,
      });

      const { fetchAgents } = await import("../agentListUtils");
      const result = await fetchAgents();
      expect(result).toBeNull();
    });
  });

  // --------------------------------------------------------------------------
  // initAgents
  // --------------------------------------------------------------------------

  describe("initAgents", () => {
    it("populates the reactive store with API agents", async () => {
      const apiAgents = [
        { agent_id: "live-a1", name: "live-agent-1", task_id: "t1", status: "Running" },
      ];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ agents: apiAgents, count: 1 }),
      });

      const { initAgents, agents, agentsLoaded } = await import("../agentListUtils");
      await initAgents();

      expect(agentsLoaded()).toBe(true);
      const list = agents();
      expect(list.some((a) => a.id === "live-a1")).toBe(true);
    });

    it("sets empty array when API returns empty", async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ agents: [], count: 0 }),
      });

      const { initAgents, agents, agentsLoaded } = await import("../agentListUtils");
      await initAgents();

      expect(agentsLoaded()).toBe(true);
      expect(agents()).toHaveLength(0);
    });
  });

  // --------------------------------------------------------------------------
  // refreshAgents
  // --------------------------------------------------------------------------

  describe("refreshAgents", () => {
    it("updates agents when API returns data", async () => {
      const apiAgents = [
        { agent_id: "refresh-a1", name: "refresh-agent-1", task_id: "t1", status: "Running" },
        { agent_id: "refresh-a2", name: "refresh-agent-2", task_id: "t2", status: "Terminated" },
      ];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ agents: apiAgents, count: 2 }),
      });

      const { refreshAgents, agents } = await import("../agentListUtils");
      await refreshAgents();

      const list = agents();
      expect(list.some((a) => a.id === "refresh-a1")).toBe(true);
      expect(list.some((a) => a.id === "refresh-a2")).toBe(true);
    });

    it("keeps existing agents when API errors", async () => {
      mockFetch.mockRejectedValueOnce(new Error("fail"));

      const { refreshAgents, agents } = await import("../agentListUtils");
      const countBefore = agents().length;
      await refreshAgents();

      expect(agents().length).toBe(countBefore);
    });
  });

  // --------------------------------------------------------------------------
  // startAgentRefresh / stopAgentRefresh
  // --------------------------------------------------------------------------

  describe("startAgentRefresh / stopAgentRefresh", () => {
    it("starts and stops without error", async () => {
      const { startAgentRefresh, stopAgentRefresh } = await import("../agentListUtils");
      startAgentRefresh();
      startAgentRefresh(); // idempotent
      stopAgentRefresh();
      stopAgentRefresh(); // safe double stop
    });
  });
});
