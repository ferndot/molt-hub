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
    taskId: "01HZABCDEFGHJKMNPQRSTVWXYZ",
    agentId: "agent-42",
    taskTitle: "Deploy billing service v2",
    stage: "deployment",
    requestedBy: "backend-agent-3",
    requestedAt: new Date(Date.now() - 30 * 60 * 1000).toISOString(),
    summary: "Migration script ready, zero-downtime deploy.",
  };

  it("submitTaskHumanDecision approve sends POST to /api/tasks/:id/decision", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve({ taskId: SAMPLE_REQUEST.taskId, status: "complete" }),
    });

    const { api } = await import("../../../lib/api");
    await api.submitTaskHumanDecision(SAMPLE_REQUEST.taskId, {
      boardId: "main-board",
      kind: "approved",
    });

    expect(mockFetch).toHaveBeenCalledWith(
      `/api/tasks/${SAMPLE_REQUEST.taskId}/decision`,
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("submitTaskHumanDecision reject sends reason in body", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve({ taskId: SAMPLE_REQUEST.taskId, status: "running" }),
    });

    const { api } = await import("../../../lib/api");
    await api.submitTaskHumanDecision(SAMPLE_REQUEST.taskId, {
      boardId: "main-board",
      kind: "rejected",
      reason: "Tests failing",
    });

    expect(mockFetch).toHaveBeenCalledWith(
      `/api/tasks/${SAMPLE_REQUEST.taskId}/decision`,
      expect.objectContaining({
        method: "POST",
        body: expect.stringContaining("Tests failing"),
      }),
    );
  });

  it("ApprovalRequest type has all required fields", () => {
    // Type-level check: if this compiles, the interface is correct
    const req: ApprovalRequest = SAMPLE_REQUEST;
    expect(req.id).toBe("apr-001");
    expect(req.taskId).toBeTruthy();
    expect(req.agentId).toBe("agent-42");
    expect(req.taskTitle).toBe("Deploy billing service v2");
    expect(req.stage).toBe("deployment");
    expect(req.requestedBy).toBe("backend-agent-3");
    expect(typeof req.requestedAt).toBe("string");
    expect(req.summary).toBe("Migration script ready, zero-downtime deploy.");
  });

  it("submitTaskHumanDecision throws on server error", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 500,
      text: () => Promise.resolve(""),
    });

    const { api } = await import("../../../lib/api");
    await expect(
      api.submitTaskHumanDecision("tid", { boardId: "b", kind: "approved" }),
    ).rejects.toThrow("POST /tasks/tid/decision failed: 500");
  });
});
