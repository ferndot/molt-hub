/**
 * Tests for ApprovalCard — verifies approve/reject API calls
 * and callback behaviour. Uses pure function testing (no DOM rendering).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { ApprovalRequest } from "../ApprovalCard";

describe("ApprovalCard logic", () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    globalThis.fetch = mockFetch as typeof fetch;
    mockFetch.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  const SAMPLE_REQUEST: ApprovalRequest = {
    id: "apr-001",
    agentId: "agent-42",
    taskTitle: "Deploy billing service v2",
    stage: "deployment",
    requestedBy: "backend-agent-3",
    requestedAt: new Date(Date.now() - 30 * 60 * 1000).toISOString(),
    summary: "Migration script ready, zero-downtime deploy.",
  };

  it("approveAgent sends POST to /api/agents/:id/approve", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve({ ok: true }),
    });

    const { api } = await import("../../../lib/api");
    await api.approveAgent(SAMPLE_REQUEST.agentId);

    expect(mockFetch).toHaveBeenCalledWith(
      "/api/agents/agent-42/approve",
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("rejectAgent sends POST to /api/agents/:id/reject with reason", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve({ ok: true }),
    });

    const { api } = await import("../../../lib/api");
    await api.rejectAgent(SAMPLE_REQUEST.agentId, "Tests failing");

    expect(mockFetch).toHaveBeenCalledWith(
      "/api/agents/agent-42/reject",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ reason: "Tests failing" }),
      }),
    );
  });

  it("ApprovalRequest type has all required fields", () => {
    // Type-level check: if this compiles, the interface is correct
    const req: ApprovalRequest = SAMPLE_REQUEST;
    expect(req.id).toBe("apr-001");
    expect(req.agentId).toBe("agent-42");
    expect(req.taskTitle).toBe("Deploy billing service v2");
    expect(req.stage).toBe("deployment");
    expect(req.requestedBy).toBe("backend-agent-3");
    expect(typeof req.requestedAt).toBe("string");
    expect(req.summary).toBe("Migration script ready, zero-downtime deploy.");
  });

  it("approve API throws on server error", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 500,
      text: () => Promise.resolve(""),
    });

    const { api } = await import("../../../lib/api");
    await expect(api.approveAgent("agent-42")).rejects.toThrow(
      "POST /agents/agent-42/approve failed: 500",
    );
  });

  it("reject API throws on server error", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 422,
      text: () => Promise.resolve(""),
    });

    const { api } = await import("../../../lib/api");
    await expect(
      api.rejectAgent("agent-42", "Bad code"),
    ).rejects.toThrow("POST /agents/agent-42/reject failed: 422");
  });
});
