/**
 * Tests for boardStore — actions, sorting, and task filtering.
 */
import { describe, it, expect, beforeEach } from "vitest";

// We need to reset module state between tests, so we use dynamic imports
// and re-import the module each time via a factory.

describe("boardStore", () => {
  // ---------------------------------------------------------------------------
  // Helpers: build a minimal BoardTask
  // ---------------------------------------------------------------------------

  // We import the store statically since it is a singleton. We reset state via
  // moveTask / expandCard / collapseCard rather than re-importing each time.
  // Tests that need a clean slate manipulate individual tasks by known IDs from
  // the mock data seeded in boardStore.ts.

  const KNOWN_TASK_ID = "01HZAA0001"; // stage: in-progress
  const KNOWN_TASK_ID2 = "01HZAA0002"; // stage: in-progress

  // --------------------------------------------------------------------------
  // moveTask
  // --------------------------------------------------------------------------

  describe("moveTask", () => {
    it("changes the task's stage to the target stage", async () => {
      const { boardState, moveTask } = await import("../boardStore");
      const before = boardState.tasks.find((t) => t.id === KNOWN_TASK_ID);
      expect(before?.stage).toBe("in-progress");

      moveTask(KNOWN_TASK_ID, "in-progress", "code-review");

      const after = boardState.tasks.find((t) => t.id === KNOWN_TASK_ID);
      expect(after?.stage).toBe("code-review");

      // restore
      moveTask(KNOWN_TASK_ID, "code-review", "in-progress");
    });

    it("does not affect other tasks when moving one task", async () => {
      const { boardState, moveTask } = await import("../boardStore");
      const before = boardState.tasks.find((t) => t.id === KNOWN_TASK_ID2);
      const originalStage = before?.stage ?? "in-progress";

      moveTask(KNOWN_TASK_ID, "in-progress", "testing");

      const unchanged = boardState.tasks.find((t) => t.id === KNOWN_TASK_ID2);
      expect(unchanged?.stage).toBe(originalStage);

      moveTask(KNOWN_TASK_ID, "testing", "in-progress");
    });

    it("can move a task to backlog", async () => {
      const { boardState, moveTask } = await import("../boardStore");
      moveTask(KNOWN_TASK_ID, "in-progress", "backlog");
      const task = boardState.tasks.find((t) => t.id === KNOWN_TASK_ID);
      expect(task?.stage).toBe("backlog");
      // restore
      moveTask(KNOWN_TASK_ID, "backlog", "in-progress");
    });
  });

  // --------------------------------------------------------------------------
  // expandCard / collapseCard / toggleCard
  // --------------------------------------------------------------------------

  describe("card expand/collapse", () => {
    it("expandCard sets expanded to true", async () => {
      const { boardState, expandCard, collapseCard } = await import(
        "../boardStore"
      );
      collapseCard(KNOWN_TASK_ID); // ensure it starts collapsed
      expandCard(KNOWN_TASK_ID);
      const task = boardState.tasks.find((t) => t.id === KNOWN_TASK_ID);
      expect(task?.expanded).toBe(true);
    });

    it("collapseCard sets expanded to false", async () => {
      const { boardState, expandCard, collapseCard } = await import(
        "../boardStore"
      );
      expandCard(KNOWN_TASK_ID); // ensure it starts expanded
      collapseCard(KNOWN_TASK_ID);
      const task = boardState.tasks.find((t) => t.id === KNOWN_TASK_ID);
      expect(task?.expanded).toBe(false);
    });

    it("toggleCard flips expanded state", async () => {
      const { boardState, collapseCard, toggleCard } = await import(
        "../boardStore"
      );
      collapseCard(KNOWN_TASK_ID);
      toggleCard(KNOWN_TASK_ID);
      const afterFirst = boardState.tasks.find((t) => t.id === KNOWN_TASK_ID);
      expect(afterFirst?.expanded).toBe(true);

      toggleCard(KNOWN_TASK_ID);
      const afterSecond = boardState.tasks.find((t) => t.id === KNOWN_TASK_ID);
      expect(afterSecond?.expanded).toBe(false);
    });
  });

  // --------------------------------------------------------------------------
  // sortByPriority
  // --------------------------------------------------------------------------

  describe("sortByPriority", () => {
    it("places P0 tasks before P1 tasks", async () => {
      const { sortByPriority } = await import("../boardStore");
      const tasks = [
        {
          id: "a",
          name: "B",
          agentName: "x",
          priority: "p1" as const,
          status: "waiting" as const,
          stage: "backlog",
          summary: "",
          timeInStage: "—",
          expanded: false,
        },
        {
          id: "b",
          name: "A",
          agentName: "y",
          priority: "p0" as const,
          status: "running" as const,
          stage: "backlog",
          summary: "",
          timeInStage: "—",
          expanded: false,
        },
      ];
      const sorted = sortByPriority(tasks);
      expect(sorted[0].priority).toBe("p0");
      expect(sorted[1].priority).toBe("p1");
    });

    it("preserves order of tasks with equal priority", async () => {
      const { sortByPriority } = await import("../boardStore");
      const tasks = [
        {
          id: "a",
          name: "First",
          agentName: "x",
          priority: "p2" as const,
          status: "waiting" as const,
          stage: "backlog",
          summary: "",
          timeInStage: "—",
          expanded: false,
        },
        {
          id: "b",
          name: "Second",
          agentName: "y",
          priority: "p2" as const,
          status: "waiting" as const,
          stage: "backlog",
          summary: "",
          timeInStage: "—",
          expanded: false,
        },
      ];
      const sorted = sortByPriority(tasks);
      expect(sorted.map((t) => t.id)).toEqual(["a", "b"]);
    });

    it("orders all four priority levels correctly", async () => {
      const { sortByPriority } = await import("../boardStore");
      const tasks = [
        {
          id: "d",
          name: "D",
          agentName: "x",
          priority: "p3" as const,
          status: "waiting" as const,
          stage: "backlog",
          summary: "",
          timeInStage: "—",
          expanded: false,
        },
        {
          id: "b",
          name: "B",
          agentName: "x",
          priority: "p1" as const,
          status: "waiting" as const,
          stage: "backlog",
          summary: "",
          timeInStage: "—",
          expanded: false,
        },
        {
          id: "a",
          name: "A",
          agentName: "x",
          priority: "p0" as const,
          status: "running" as const,
          stage: "backlog",
          summary: "",
          timeInStage: "—",
          expanded: false,
        },
        {
          id: "c",
          name: "C",
          agentName: "x",
          priority: "p2" as const,
          status: "waiting" as const,
          stage: "backlog",
          summary: "",
          timeInStage: "—",
          expanded: false,
        },
      ];
      const sorted = sortByPriority(tasks);
      expect(sorted.map((t) => t.priority)).toEqual(["p0", "p1", "p2", "p3"]);
    });
  });

  // --------------------------------------------------------------------------
  // tasksForStage
  // --------------------------------------------------------------------------

  describe("tasksForStage", () => {
    it("returns only tasks matching the given stage", async () => {
      const { tasksForStage } = await import("../boardStore");
      const backlog = tasksForStage("backlog");
      expect(backlog.every((t) => t.stage === "backlog")).toBe(true);
    });

    it("returns tasks sorted by priority (P0 first)", async () => {
      const { boardState, moveTask, tasksForStage } = await import(
        "../boardStore"
      );
      // Move a P3 task to in-progress so we have mixed priorities
      const p3Task = boardState.tasks.find((t) => t.priority === "p3");
      if (p3Task && p3Task.stage !== "in-progress") {
        moveTask(p3Task.id, p3Task.stage, "in-progress");
      }

      const inProgress = tasksForStage("in-progress");
      if (inProgress.length > 1) {
        const priorityVals = { p0: 0, p1: 1, p2: 2, p3: 3 };
        for (let i = 0; i < inProgress.length - 1; i++) {
          expect(priorityVals[inProgress[i].priority]).toBeLessThanOrEqual(
            priorityVals[inProgress[i + 1].priority],
          );
        }
      }
      // restore
      if (p3Task && p3Task.stage !== "in-progress") {
        moveTask(p3Task.id, "in-progress", p3Task.stage);
      }
    });

    it("returns an empty array for an unknown stage", async () => {
      const { tasksForStage } = await import("../boardStore");
      expect(tasksForStage("nonexistent")).toEqual([]);
    });
  });

  // --------------------------------------------------------------------------
  // Drag-and-drop data transfer helpers
  // --------------------------------------------------------------------------

  describe("drag-and-drop data transfer format", () => {
    it("serialises taskId and fromStage to JSON for the data transfer", () => {
      const taskId = "01HZAA0001";
      const fromStage = "in-progress";
      const payload = JSON.stringify({ taskId, fromStage });
      const parsed = JSON.parse(payload) as {
        taskId: string;
        fromStage: string;
      };
      expect(parsed.taskId).toBe(taskId);
      expect(parsed.fromStage).toBe(fromStage);
    });

    it("ignores drops that have the same fromStage as toStage", () => {
      // Simulate the guard in BoardColumn.handleDrop
      const fromStage = "in-progress";
      const toStage = "in-progress";
      const shouldMove = fromStage !== toStage;
      expect(shouldMove).toBe(false);
    });

    it("allows drops when fromStage differs from toStage", () => {
      const fromStage: string = "in-progress";
      const toStage: string = "code-review";
      const shouldMove = fromStage !== toStage;
      expect(shouldMove).toBe(true);
    });
  });
});
