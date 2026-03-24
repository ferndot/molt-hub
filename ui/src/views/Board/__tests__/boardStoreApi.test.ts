/**
 * Tests for boardStore API integration — verifying stages are loaded
 * from the server and pushStagesToApi sends correct data.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

describe("boardStore API integration", () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    globalThis.fetch = mockFetch;
    mockFetch.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("fetchBoardStages returns parsed stages on success", async () => {
    const stages = [
      { id: "backlog", label: "Backlog", wip_limit: null },
      { id: "in-progress", label: "In Progress", wip_limit: 3 },
    ];
    mockFetch.mockResolvedValueOnce({
      ok: true,
      headers: {
        get: (name: string) => (name === "content-type" ? "application/json" : null),
      },
      json: () => Promise.resolve({ stages }),
    });

    const { fetchBoardStages } = await import("../boardStore");
    const result = await fetchBoardStages("default");
    expect(mockFetch).toHaveBeenCalledWith(
      "/api/projects/default/boards/default/stages",
    );
    expect(result).toEqual(stages);
  });

  it("fetchBoardStages returns null on network error", async () => {
    mockFetch.mockRejectedValueOnce(new Error("Network error"));

    const { fetchBoardStages } = await import("../boardStore");
    const result = await fetchBoardStages("default");
    expect(result).toBeNull();
  });

  it("fetchBoardStages returns null on non-ok response", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 500,
    });

    const { fetchBoardStages } = await import("../boardStore");
    const result = await fetchBoardStages("default");
    expect(result).toBeNull();
  });

  it("fetchBoardStages returns null for empty stages array", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      headers: {
        get: (name: string) => (name === "content-type" ? "application/json" : null),
      },
      json: () => Promise.resolve({ stages: [] }),
    });

    const { fetchBoardStages } = await import("../boardStore");
    const result = await fetchBoardStages("default");
    expect(result).toBeNull();
  });

  it("pushStagesToApi calls PUT with current pipeline stages for active board", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve({ stages: [] }),
    });

    const { pushStagesToApi } = await import("../boardStore");
    await pushStagesToApi();

    expect(mockFetch).toHaveBeenCalled();
    const call = mockFetch.mock.calls[0];
    expect(call[0]).toBe("/api/projects/default/boards/default/stages");
    expect((call[1] as { method: string }).method).toBe("PUT");
    const body = JSON.parse((call[1] as { body: string }).body as string);
    expect(body).toHaveProperty("stages");
  });

  it("patchStage optimistically updates local state and calls PATCH API", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () =>
        Promise.resolve({
          id: "backlog",
          label: "Updated",
          color: "#ff0000",
          order: 0,
          wip_limit: null,
          requires_approval: false,
          timeout_seconds: null,
          terminal: false,
        }),
    });

    const { patchStage, boardState } = await import("../boardStore");
    await patchStage("backlog", { label: "Updated", color: "#ff0000" });

    const updated = boardState.pipelineStages.find((s) => s.id === "backlog");
    expect(updated?.label).toBe("Updated");
    expect(updated?.color).toBe("#ff0000");

    expect(mockFetch).toHaveBeenCalled();
    const call = mockFetch.mock.calls[0];
    expect(call[0]).toBe("/api/projects/default/boards/default/stages/backlog");
    expect((call[1] as { method: string }).method).toBe("PATCH");
    const body = JSON.parse((call[1] as { body: string }).body as string);
    expect(body.label).toBe("Updated");
    expect(body.color).toBe("#ff0000");
  });

  it("getSortedStages returns stages sorted by order", async () => {
    const { getSortedStages } = await import("../boardStore");
    const sorted = getSortedStages();
    for (let i = 0; i < sorted.length - 1; i++) {
      expect(sorted[i].order).toBeLessThanOrEqual(sorted[i + 1].order);
    }
  });

  it("initBoardStages sorts fetched stages by order", async () => {
    const stages = [
      { id: "deployed", label: "Deployed", wip_limit: null, requires_approval: false, timeout_seconds: null, terminal: true, color: "#10b981", order: 4 },
      { id: "backlog", label: "Backlog", wip_limit: null, requires_approval: false, timeout_seconds: null, terminal: false, color: "#6b7280", order: 0 },
      { id: "in-progress", label: "In Progress", wip_limit: 3, requires_approval: false, timeout_seconds: null, terminal: false, color: "#3b82f6", order: 1 },
    ];
    mockFetch.mockResolvedValueOnce({
      ok: true,
      headers: {
        get: (name: string) => (name === "content-type" ? "application/json" : null),
      },
      json: () => Promise.resolve({ boards: [{ id: "default", name: "Default" }] }),
    });
    mockFetch.mockResolvedValueOnce({
      ok: true,
      headers: {
        get: (name: string) => (name === "content-type" ? "application/json" : null),
      },
      json: () => Promise.resolve({ stages }),
    });

    const { initBoardStages, boardState } = await import("../boardStore");
    await initBoardStages();

    expect(boardState.stagesLoaded).toBe(true);
    expect(boardState.stages[0]).toBe("backlog");
    expect(boardState.stages[1]).toBe("in-progress");
    expect(boardState.stages[2]).toBe("deployed");
    expect(boardState.pipelineStages[0].color).toBe("#6b7280");
    expect(boardState.pipelineStages[0].order).toBe(0);
  });

  it("default pipeline stages include color and order fields", async () => {
    const { boardState } = await import("../boardStore");
    for (const stage of boardState.pipelineStages) {
      expect(stage).toHaveProperty("color");
      expect(stage).toHaveProperty("order");
      expect(typeof stage.order).toBe("number");
    }
  });
});
