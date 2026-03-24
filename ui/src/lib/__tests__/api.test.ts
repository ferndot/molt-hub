/**
 * Tests for the API client module.
 *
 * Mocks global fetch to verify correct URLs, methods, and bodies.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// We use dynamic import to avoid module-level side effects
let api: typeof import("../api")["api"];

describe("api client", () => {
  const mockFetch = vi.fn();

  beforeEach(async () => {
    // override global fetch for tests
    globalThis.fetch = mockFetch;
    mockFetch.mockReset();
    // Re-import to get fresh module
    const mod = await import("../api");
    api = mod.api;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  function mockJsonResponse(data: unknown, status = 200) {
    return mockFetch.mockResolvedValueOnce({
      ok: status >= 200 && status < 300,
      status,
      json: () => Promise.resolve(data),
    });
  }

  // ---- Settings ----

  it("getSettings sends GET /api/settings", async () => {
    mockJsonResponse({ appearance: { theme: "dark" } });
    const result = await api.getSettings();
    expect(mockFetch).toHaveBeenCalledWith("/api/settings");
    expect(result).toEqual({ appearance: { theme: "dark" } });
  });

  it("updateSettings sends PUT /api/settings with JSON body", async () => {
    const payload = { appearance: { theme: "light" } };
    mockJsonResponse(payload);
    await api.updateSettings(payload);
    expect(mockFetch).toHaveBeenCalledWith("/api/settings", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
  });

  // ---- Pipeline ----

  it("getStages sends GET /api/pipeline/stages", async () => {
    const data = { stages: [{ id: "backlog", label: "Backlog", wip_limit: null }] };
    mockJsonResponse(data);
    const result = await api.getStages();
    expect(mockFetch).toHaveBeenCalledWith("/api/pipeline/stages");
    expect(result.stages).toHaveLength(1);
  });

  it("updateStages sends PUT /api/pipeline/stages", async () => {
    const payload = { stages: [] };
    mockJsonResponse(payload);
    await api.updateStages(payload);
    expect(mockFetch).toHaveBeenCalledWith("/api/pipeline/stages", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
  });

  // ---- Agents ----

  it("getAgents sends GET /api/agents", async () => {
    mockJsonResponse({ agents: [] });
    const result = await api.getAgents();
    expect(mockFetch).toHaveBeenCalledWith("/api/agents");
    expect(result.agents).toEqual([]);
  });

  it("spawnAgent sends POST /api/agents/spawn with body", async () => {
    const req = { task: "test" };
    mockJsonResponse({ id: "agent-1" });
    await api.spawnAgent(req);
    expect(mockFetch).toHaveBeenCalledWith("/api/agents/spawn", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(req),
    });
  });

  it("terminateAgent sends POST /api/agents/:id/terminate", async () => {
    mockJsonResponse({ ok: true });
    await api.terminateAgent("agent-42");
    expect(mockFetch).toHaveBeenCalledWith("/api/agents/agent-42/terminate", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: undefined,
    });
  });

  it("getAgentOutput sends GET /api/agents/:id/output", async () => {
    mockJsonResponse({ lines: ["hello"] });
    const result = await api.getAgentOutput("agent-7");
    expect(mockFetch).toHaveBeenCalledWith("/api/agents/agent-7/output");
    expect(result.lines).toEqual(["hello"]);
  });

  // ---- Approval ----

  it("approveAgent sends POST /api/agents/:id/approve", async () => {
    mockJsonResponse({ ok: true });
    await api.approveAgent("agent-99");
    expect(mockFetch).toHaveBeenCalledWith("/api/agents/agent-99/approve", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: undefined,
    });
  });

  it("rejectAgent sends POST /api/agents/:id/reject with reason", async () => {
    mockJsonResponse({ ok: true });
    await api.rejectAgent("agent-99", "Not ready");
    expect(mockFetch).toHaveBeenCalledWith("/api/agents/agent-99/reject", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ reason: "Not ready" }),
    });
  });

  // ---- Audit ----

  it("getAuditLog sends GET /api/audit with limit", async () => {
    mockJsonResponse({ entries: [] });
    await api.getAuditLog(50);
    expect(mockFetch).toHaveBeenCalledWith("/api/audit?limit=50");
  });

  it("getAuditLog defaults limit to 100", async () => {
    mockJsonResponse({ entries: [] });
    await api.getAuditLog();
    expect(mockFetch).toHaveBeenCalledWith("/api/audit?limit=100");
  });

  // ---- GitHub Integration ----

  it("getGithubStatus sends GET /api/integrations/github/status", async () => {
    mockJsonResponse({ connected: true, owner: "my-org" });
    const result = await api.getGithubStatus();
    expect(mockFetch).toHaveBeenCalledWith("/api/integrations/github/status");
    expect(result.connected).toBe(true);
    expect(result.owner).toBe("my-org");
  });

  it("getGithubAuthUrl sends GET /api/integrations/github/auth", async () => {
    mockJsonResponse({ url: "https://github.com/login/oauth/authorize?..." });
    const result = await api.getGithubAuthUrl();
    expect(mockFetch).toHaveBeenCalledWith("/api/integrations/github/auth");
    expect(result.url).toContain("github.com");
  });

  it("disconnectGithub sends POST /api/integrations/github/disconnect", async () => {
    mockJsonResponse({ ok: true });
    await api.disconnectGithub();
    expect(mockFetch).toHaveBeenCalledWith("/api/integrations/github/disconnect", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: undefined,
    });
  });

  // ---- Error handling ----

  it("throws on non-ok response", async () => {
    mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });
    await expect(api.getSettings()).rejects.toThrow("GET /settings failed: 500");
  });
});
