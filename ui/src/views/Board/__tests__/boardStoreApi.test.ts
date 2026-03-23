/**
 * Tests for boardStore API integration — verifying stages are loaded
 * from the server and pushStagesToApi sends correct data.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

describe("boardStore API integration", () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    // override global fetch for tests
    globalThis.fetch = mockFetch;
    mockFetch.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("fetchPipelineStages returns parsed stages on success", async () => {
    const stages = [
      { id: "backlog", label: "Backlog", wip_limit: null },
      { id: "in-progress", label: "In Progress", wip_limit: 3 },
    ];
    mockFetch.mockResolvedValueOnce({
      ok: true,
      headers: new Map([["content-type", "application/json"]]) as unknown as Headers,
      json: () => Promise.resolve({ stages }),
    });

    // Provide a real Headers-like object for the test
    mockFetch.mockReset();
    mockFetch.mockResolvedValueOnce({
      ok: true,
      headers: {
        get: (name: string) => name === "content-type" ? "application/json" : null,
      },
      json: () => Promise.resolve({ stages }),
    });

    const { fetchPipelineStages } = await import("../boardStore");
    const result = await fetchPipelineStages();
    expect(result).toEqual(stages);
  });

  it("fetchPipelineStages returns null on network error", async () => {
    mockFetch.mockRejectedValueOnce(new Error("Network error"));

    const { fetchPipelineStages } = await import("../boardStore");
    const result = await fetchPipelineStages();
    expect(result).toBeNull();
  });

  it("fetchPipelineStages returns null on non-ok response", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 500,
    });

    const { fetchPipelineStages } = await import("../boardStore");
    const result = await fetchPipelineStages();
    expect(result).toBeNull();
  });

  it("fetchPipelineStages returns null for empty stages array", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      headers: {
        get: (name: string) => name === "content-type" ? "application/json" : null,
      },
      json: () => Promise.resolve({ stages: [] }),
    });

    const { fetchPipelineStages } = await import("../boardStore");
    const result = await fetchPipelineStages();
    expect(result).toBeNull();
  });

  it("pushStagesToApi calls api.updateStages with current pipeline stages", async () => {
    // Mock the fetch for the updateStages call
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve({ stages: [] }),
    });

    const { pushStagesToApi, boardState } = await import("../boardStore");
    await pushStagesToApi();

    // Verify fetch was called with PUT and pipeline stages
    expect(mockFetch).toHaveBeenCalled();
    const call = mockFetch.mock.calls[0];
    expect(call[0]).toBe("/api/pipeline/stages");
    expect(call[1].method).toBe("PUT");
    const body = JSON.parse(call[1].body as string);
    expect(body).toHaveProperty("stages");
  });
});
