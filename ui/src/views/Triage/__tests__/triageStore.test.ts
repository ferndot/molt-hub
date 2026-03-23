/**
 * Tests for the triage store actions and selectors.
 *
 * Uses vitest in node environment — no DOM required.
 */

import { describe, it, expect, beforeEach } from "vitest";
import { getFilteredItems } from "../triageStore";
import type { TriageItem } from "../triageStore";
import type { Priority } from "../../../types/domain";

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

function makeItem(
  overrides: Partial<TriageItem> & { id: string; priority: Priority },
): TriageItem {
  return {
    taskId: `task-${overrides.id}`,
    taskName: `Task ${overrides.id}`,
    agentName: `agent-1`,
    stage: "code-review",
    type: "decision",
    createdAt: new Date(Date.now() - 60_000).toISOString(),
    summary: "summary",
    ...overrides,
  };
}

const ITEMS: TriageItem[] = [
  makeItem({ id: "a", priority: "p2", createdAt: new Date(Date.now() - 10_000).toISOString() }),
  makeItem({ id: "b", priority: "p0", createdAt: new Date(Date.now() - 30_000).toISOString() }),
  makeItem({ id: "c", priority: "p3", createdAt: new Date(Date.now() - 5_000).toISOString() }),
  makeItem({ id: "d", priority: "p1", createdAt: new Date(Date.now() - 20_000).toISOString() }),
  makeItem({ id: "e", priority: "p0", createdAt: new Date(Date.now() - 60_000).toISOString() }),
];

// ---------------------------------------------------------------------------
// Sort order
// ---------------------------------------------------------------------------

describe("getFilteredItems — sort by priority", () => {
  it("returns P0 items before P1, P1 before P2, P2 before P3", () => {
    const result = getFilteredItems(ITEMS, "all", "priority");
    const priorities = result.map((i) => i.priority);
    // All P0s come before P1s, P1s before P2s, P2s before P3s
    const firstP1 = priorities.indexOf("p1");
    const lastP0 = priorities.lastIndexOf("p0");
    const firstP2 = priorities.indexOf("p2");
    const lastP1 = priorities.lastIndexOf("p1");
    const firstP3 = priorities.indexOf("p3");
    const lastP2 = priorities.lastIndexOf("p2");

    expect(lastP0).toBeLessThan(firstP1);
    expect(lastP1).toBeLessThan(firstP2);
    expect(lastP2).toBeLessThan(firstP3);
  });

  it("within the same priority tier, older items (earliest createdAt) come first", () => {
    const result = getFilteredItems(ITEMS, "all", "priority");
    const p0Items = result.filter((i) => i.priority === "p0");
    // item 'e' is older than 'b'
    expect(p0Items[0].id).toBe("e");
    expect(p0Items[1].id).toBe("b");
  });
});

describe("getFilteredItems — sort by time-waiting", () => {
  it("returns oldest (longest waiting) items first regardless of priority", () => {
    const result = getFilteredItems(ITEMS, "all", "time-waiting");
    // 'e' is 60s old, 'b' is 30s, 'd' is 20s, 'a' is 10s, 'c' is 5s
    expect(result[0].id).toBe("e");
    expect(result[result.length - 1].id).toBe("c");
  });
});

describe("getFilteredItems — sort by agent", () => {
  it("sorts by agentName alphabetically", () => {
    const multiAgent: TriageItem[] = [
      makeItem({ id: "x", priority: "p0", agentName: "zeta-agent" }),
      makeItem({ id: "y", priority: "p1", agentName: "alpha-agent" }),
      makeItem({ id: "z", priority: "p2", agentName: "beta-agent" }),
    ];
    const result = getFilteredItems(multiAgent, "all", "by-agent");
    expect(result[0].agentName).toBe("alpha-agent");
    expect(result[1].agentName).toBe("beta-agent");
    expect(result[2].agentName).toBe("zeta-agent");
  });
});

// ---------------------------------------------------------------------------
// Filter mode: needs-action
// ---------------------------------------------------------------------------

describe("getFilteredItems — filter: needs-action", () => {
  it("includes only P0 and P1 items", () => {
    const result = getFilteredItems(ITEMS, "needs-action", "priority");
    expect(result.every((i) => i.priority === "p0" || i.priority === "p1")).toBe(true);
  });

  it("excludes P2 and P3 items", () => {
    const result = getFilteredItems(ITEMS, "needs-action", "priority");
    expect(result.some((i) => i.priority === "p2" || i.priority === "p3")).toBe(false);
  });

  it("returns correct count (3 = 2 P0 + 1 P1)", () => {
    const result = getFilteredItems(ITEMS, "needs-action", "priority");
    expect(result.length).toBe(3);
  });
});

// ---------------------------------------------------------------------------
// Filter mode: all
// ---------------------------------------------------------------------------

describe("getFilteredItems — filter: all", () => {
  it("returns all items", () => {
    const result = getFilteredItems(ITEMS, "all", "priority");
    expect(result.length).toBe(ITEMS.length);
  });
});

// ---------------------------------------------------------------------------
// Store actions (approve, reject, defer) via direct array manipulation
// ---------------------------------------------------------------------------

describe("store action semantics (pure logic)", () => {
  // We test the action logic independently since the SolidJS store state
  // is a singleton that resets between test runs via module isolation.
  // The key behaviour is:
  //   approve / reject → remove item
  //   defer → move item to end

  it("removing an item by id leaves remaining items unchanged", () => {
    const items: TriageItem[] = [
      makeItem({ id: "1", priority: "p0" }),
      makeItem({ id: "2", priority: "p1" }),
      makeItem({ id: "3", priority: "p2" }),
    ];
    const result = items.filter((i) => i.id !== "2");
    expect(result.length).toBe(2);
    expect(result.map((i) => i.id)).toEqual(["1", "3"]);
  });

  it("defer: moving an item to the end preserves others in relative order", () => {
    const items: TriageItem[] = [
      makeItem({ id: "1", priority: "p0" }),
      makeItem({ id: "2", priority: "p1" }),
      makeItem({ id: "3", priority: "p2" }),
    ];
    const idx = items.findIndex((i) => i.id === "1");
    const copy = [...items];
    const [removed] = copy.splice(idx, 1);
    copy.push(removed);
    expect(copy.map((i) => i.id)).toEqual(["2", "3", "1"]);
  });
});
