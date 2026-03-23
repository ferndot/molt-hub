/**
 * PriorityBadge tests — validates type contracts, label mapping,
 * and exported API surface.
 *
 * Runs in node environment (consistent with project test conventions).
 * Imports from priorityTypes.ts (pure data) to avoid CSS module / JSX issues.
 */

import { describe, it, expect } from "vitest";
import {
  PRIORITY_LABELS,
  ALL_PRIORITIES,
  ALL_BADGE_SIZES,
  type PriorityLevel,
} from "../priorityTypes";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("PriorityBadge", () => {
  describe("PRIORITY_LABELS", () => {
    it("provides a human-readable label for every priority", () => {
      for (const priority of ALL_PRIORITIES) {
        const label = PRIORITY_LABELS[priority];
        expect(label).toBeDefined();
        expect(typeof label).toBe("string");
        expect(label.length).toBeGreaterThan(0);
      }
    });

    it("maps p0 → 'Critical'", () => {
      expect(PRIORITY_LABELS.p0).toBe("Critical");
    });

    it("maps p1 → 'High'", () => {
      expect(PRIORITY_LABELS.p1).toBe("High");
    });

    it("maps p2 → 'Medium'", () => {
      expect(PRIORITY_LABELS.p2).toBe("Medium");
    });

    it("maps p3 → 'Low'", () => {
      expect(PRIORITY_LABELS.p3).toBe("Low");
    });
  });

  describe("type coverage", () => {
    it("ALL_PRIORITIES contains exactly 4 variants", () => {
      expect(ALL_PRIORITIES).toHaveLength(4);
    });

    it("ALL_BADGE_SIZES contains exactly 3 variants (sm, md, lg)", () => {
      expect(ALL_BADGE_SIZES).toHaveLength(3);
      expect(ALL_BADGE_SIZES).toContain("sm");
      expect(ALL_BADGE_SIZES).toContain("md");
      expect(ALL_BADGE_SIZES).toContain("lg");
    });

    it("every priority has a unique label", () => {
      const labels = ALL_PRIORITIES.map((p) => PRIORITY_LABELS[p]);
      const unique = new Set(labels);
      expect(unique.size).toBe(ALL_PRIORITIES.length);
    });
  });

  describe("label exhaustiveness", () => {
    it("PRIORITY_LABELS has no extra keys beyond known priorities", () => {
      const keys = Object.keys(PRIORITY_LABELS);
      expect(keys.sort()).toEqual([...(ALL_PRIORITIES as readonly string[])].sort());
    });
  });

  describe("aria-label contract", () => {
    it("every label is non-empty and suitable for screen readers", () => {
      for (const priority of ALL_PRIORITIES) {
        const label = PRIORITY_LABELS[priority];
        // No raw underscores — should be human-readable
        expect(label).not.toContain("_");
        // At least 3 chars for meaningful a11y
        expect(label.length).toBeGreaterThanOrEqual(3);
      }
    });

    it("labels distinguish all priority levels", () => {
      const labels = ALL_PRIORITIES.map((p) => PRIORITY_LABELS[p]);
      // All four are unique
      expect(new Set(labels).size).toBe(4);
    });
  });

  describe("priority ordering contract", () => {
    it("ALL_PRIORITIES is ordered from highest to lowest priority", () => {
      expect(ALL_PRIORITIES[0]).toBe("p0");
      expect(ALL_PRIORITIES[1]).toBe("p1");
      expect(ALL_PRIORITIES[2]).toBe("p2");
      expect(ALL_PRIORITIES[3]).toBe("p3");
    });
  });
});
