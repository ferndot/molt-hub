/**
 * Tests for AgentList grouping logic.
 * Pure functions — no DOM needed, runs in node environment.
 */
import { describe, it, expect } from "vitest";
import { groupAgentsByStatus, type Agent, type AgentStatus } from "../agentListUtils";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeAgent(overrides: Partial<Agent> & { id: string; status: AgentStatus }): Agent {
  return {
    name: overrides.name ?? `agent-${overrides.id}`,
    stage: overrides.stage ?? "Working",
    taskId: overrides.taskId ?? "",
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("groupAgentsByStatus", () => {
  it("returns empty array for empty input", () => {
    expect(groupAgentsByStatus([])).toEqual([]);
  });

  it("groups agents by status in priority order: running, paused, idle, terminated", () => {
    const agents: Agent[] = [
      makeAgent({ id: "1", status: "idle" }),
      makeAgent({ id: "2", status: "running" }),
      makeAgent({ id: "3", status: "terminated" }),
      makeAgent({ id: "4", status: "paused" }),
      makeAgent({ id: "5", status: "running" }),
    ];

    const groups = groupAgentsByStatus(agents);

    expect(groups.map((g) => g.status)).toEqual(["running", "paused", "idle", "terminated"]);
  });

  it("provides correct labels for each group", () => {
    const agents: Agent[] = [
      makeAgent({ id: "1", status: "running" }),
      makeAgent({ id: "2", status: "paused" }),
      makeAgent({ id: "3", status: "idle" }),
      makeAgent({ id: "4", status: "terminated" }),
    ];

    const groups = groupAgentsByStatus(agents);

    expect(groups.map((g) => g.label)).toEqual(["Running", "Paused", "Idle", "Terminated"]);
  });

  it("counts agents per group correctly", () => {
    const agents: Agent[] = [
      makeAgent({ id: "1", status: "running" }),
      makeAgent({ id: "2", status: "running" }),
      makeAgent({ id: "3", status: "running" }),
      makeAgent({ id: "4", status: "paused" }),
      makeAgent({ id: "5", status: "idle" }),
    ];

    const groups = groupAgentsByStatus(agents);

    expect(groups.find((g) => g.status === "running")?.agents.length).toBe(3);
    expect(groups.find((g) => g.status === "paused")?.agents.length).toBe(1);
    expect(groups.find((g) => g.status === "idle")?.agents.length).toBe(1);
  });

  it("hides empty groups", () => {
    const agents: Agent[] = [
      makeAgent({ id: "1", status: "running" }),
      makeAgent({ id: "2", status: "terminated" }),
    ];

    const groups = groupAgentsByStatus(agents);

    expect(groups.length).toBe(2);
    expect(groups.map((g) => g.status)).toEqual(["running", "terminated"]);
  });

  it("places each agent in its correct group", () => {
    const agents: Agent[] = [
      makeAgent({ id: "a", status: "paused", name: "alpha" }),
      makeAgent({ id: "b", status: "paused", name: "beta" }),
      makeAgent({ id: "c", status: "running", name: "gamma" }),
    ];

    const groups = groupAgentsByStatus(agents);
    const pausedGroup = groups.find((g) => g.status === "paused")!;

    expect(pausedGroup.agents.map((a) => a.name)).toEqual(["alpha", "beta"]);
  });

  it("handles all agents in a single status", () => {
    const agents: Agent[] = [
      makeAgent({ id: "1", status: "idle" }),
      makeAgent({ id: "2", status: "idle" }),
    ];

    const groups = groupAgentsByStatus(agents);

    expect(groups.length).toBe(1);
    expect(groups[0].status).toBe("idle");
    expect(groups[0].agents.length).toBe(2);
  });
});
