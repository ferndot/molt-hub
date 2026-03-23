/**
 * Tests for missionControlStore — merge logic, sorting, filtering.
 */
import { describe, it, expect } from "vitest";

describe("missionControlStore", () => {
  // -------------------------------------------------------------------------
  // mergeItems joins board tasks with triage attention info
  // -------------------------------------------------------------------------

  describe("merge and attention", () => {
    it("returns all board tasks as MissionControlItems", async () => {
      const { useMissionControl } = await import("../missionControlStore");
      const { boardState } = await import("../../Board/boardStore");
      const mc = useMissionControl();
      expect(mc.items().length).toBe(boardState.tasks.length);
    });

    it("attaches attentionInfo when a triage item matches a board task", async () => {
      const { useMissionControl } = await import("../missionControlStore");
      const mc = useMissionControl();
      // attentionItems should only include items that have triage matches
      const attItems = mc.attentionItems();
      for (const item of attItems) {
        expect(item.attentionInfo).toBeDefined();
        expect(item.attentionInfo!.triageId).toBeTruthy();
        expect(["decision", "info"]).toContain(item.attentionInfo!.triageType);
      }
    });
  });

  // -------------------------------------------------------------------------
  // Attention items sorted to top within stage
  // -------------------------------------------------------------------------

  describe("itemsForStage sorting", () => {
    it("places attention items before non-attention items in the same stage", async () => {
      const { useMissionControl } = await import("../missionControlStore");
      const mc = useMissionControl();
      const stages = mc.stages();
      for (const stage of stages) {
        const stageItems = mc.itemsForStage(stage);
        let foundNonAttention = false;
        for (const item of stageItems) {
          if (!item.attentionInfo) {
            foundNonAttention = true;
          }
          if (foundNonAttention && item.attentionInfo) {
            // An attention item after a non-attention item is a failure
            expect(item.attentionInfo).toBeUndefined();
          }
        }
      }
    });
  });

  // -------------------------------------------------------------------------
  // attentionItems is flat priority-sorted
  // -------------------------------------------------------------------------

  describe("attentionItems", () => {
    it("returns items sorted by priority (p0 first)", async () => {
      const { useMissionControl } = await import("../missionControlStore");
      const mc = useMissionControl();
      const att = mc.attentionItems();
      const priorityOrder: Record<string, number> = {
        p0: 0,
        p1: 1,
        p2: 2,
        p3: 3,
      };
      for (let i = 0; i < att.length - 1; i++) {
        expect(priorityOrder[att[i].priority]).toBeLessThanOrEqual(
          priorityOrder[att[i + 1].priority],
        );
      }
    });
  });

  // -------------------------------------------------------------------------
  // Filter hides non-attention items
  // -------------------------------------------------------------------------

  describe("global filter", () => {
    it("visibleItemsForStage returns all items when filter is off", async () => {
      const { useMissionControl } = await import("../missionControlStore");
      const mc = useMissionControl();
      // Ensure filter is off
      if (mc.globalFilterActive()) {
        mc.toggleGlobalFilter();
      }
      const stages = mc.stages();
      for (const stage of stages) {
        const visible = mc.visibleItemsForStage(stage);
        const all = mc.itemsForStage(stage);
        expect(visible.length).toBe(all.length);
      }
    });

    it("visibleItemsForStage returns only attention items when filter is on", async () => {
      const { useMissionControl } = await import("../missionControlStore");
      const mc = useMissionControl();
      // Ensure filter is on
      if (!mc.globalFilterActive()) {
        mc.toggleGlobalFilter();
      }
      const stages = mc.stages();
      for (const stage of stages) {
        const visible = mc.visibleItemsForStage(stage);
        for (const item of visible) {
          expect(item.attentionInfo).toBeDefined();
        }
      }
      // Restore filter state
      mc.toggleGlobalFilter();
    });
  });

  // -------------------------------------------------------------------------
  // hiddenCount correct
  // -------------------------------------------------------------------------

  describe("hiddenCountForStage", () => {
    it("returns 0 when filter is off", async () => {
      const { useMissionControl } = await import("../missionControlStore");
      const mc = useMissionControl();
      if (mc.globalFilterActive()) {
        mc.toggleGlobalFilter();
      }
      const stages = mc.stages();
      for (const stage of stages) {
        expect(mc.hiddenCountForStage(stage)).toBe(0);
      }
    });

    it("returns count of non-attention items when filter is on", async () => {
      const { useMissionControl } = await import("../missionControlStore");
      const mc = useMissionControl();
      if (!mc.globalFilterActive()) {
        mc.toggleGlobalFilter();
      }
      const stages = mc.stages();
      for (const stage of stages) {
        const all = mc.itemsForStage(stage);
        const nonAttention = all.filter((i) => !i.attentionInfo).length;
        expect(mc.hiddenCountForStage(stage)).toBe(nonAttention);
      }
      // Restore filter state
      mc.toggleGlobalFilter();
    });
  });
});
