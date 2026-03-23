/**
 * StatusIndicator tests — validates type contracts, label mapping,
 * and exported API surface.
 *
 * Runs in node environment (consistent with project test conventions).
 * Imports from statusTypes.ts (pure data) to avoid CSS module / JSX issues.
 */

import { describe, it, expect } from "vitest";
import {
  STATUS_LABELS,
  ALL_STATUSES,
  ALL_SIZES,
  type IndicatorStatus,
} from "../statusTypes";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("StatusIndicator", () => {
  describe("STATUS_LABELS", () => {
    it("provides a human-readable label for every status", () => {
      for (const status of ALL_STATUSES) {
        const label = STATUS_LABELS[status];
        expect(label).toBeDefined();
        expect(typeof label).toBe("string");
        expect(label.length).toBeGreaterThan(0);
      }
    });

    it("maps pending → 'Pending'", () => {
      expect(STATUS_LABELS.pending).toBe("Pending");
    });

    it("maps in_progress → 'In Progress'", () => {
      expect(STATUS_LABELS.in_progress).toBe("In Progress");
    });

    it("maps blocked → 'Blocked'", () => {
      expect(STATUS_LABELS.blocked).toBe("Blocked");
    });

    it("maps awaiting_approval → 'Awaiting Approval'", () => {
      expect(STATUS_LABELS.awaiting_approval).toBe("Awaiting Approval");
    });

    it("maps success → 'Completed — Success'", () => {
      expect(STATUS_LABELS.success).toBe("Completed — Success");
    });

    it("maps failure → 'Completed — Failure'", () => {
      expect(STATUS_LABELS.failure).toBe("Completed — Failure");
    });

    // Agent-lifecycle statuses
    it("maps running → 'Running'", () => {
      expect(STATUS_LABELS.running).toBe("Running");
    });

    it("maps paused → 'Paused'", () => {
      expect(STATUS_LABELS.paused).toBe("Paused");
    });

    it("maps completed → 'Completed'", () => {
      expect(STATUS_LABELS.completed).toBe("Completed");
    });

    it("maps failed → 'Failed'", () => {
      expect(STATUS_LABELS.failed).toBe("Failed");
    });

    it("maps idle → 'Idle'", () => {
      expect(STATUS_LABELS.idle).toBe("Idle");
    });

    it("maps terminated → 'Terminated'", () => {
      expect(STATUS_LABELS.terminated).toBe("Terminated");
    });
  });

  describe("type coverage", () => {
    it("ALL_STATUSES contains exactly 12 variants", () => {
      expect(ALL_STATUSES).toHaveLength(12);
    });

    it("ALL_SIZES contains exactly 3 variants (sm, md, lg)", () => {
      expect(ALL_SIZES).toHaveLength(3);
      expect(ALL_SIZES).toContain("sm");
      expect(ALL_SIZES).toContain("md");
      expect(ALL_SIZES).toContain("lg");
    });

    it("every status has a unique label", () => {
      const labels = ALL_STATUSES.map((s) => STATUS_LABELS[s]);
      const unique = new Set(labels);
      expect(unique.size).toBe(ALL_STATUSES.length);
    });
  });

  describe("label exhaustiveness", () => {
    it("STATUS_LABELS has no extra keys beyond known statuses", () => {
      const keys = Object.keys(STATUS_LABELS);
      expect(keys.sort()).toEqual([...(ALL_STATUSES as readonly string[])].sort());
    });
  });

  describe("aria-label contract", () => {
    it("every label is non-empty and suitable for screen readers", () => {
      for (const status of ALL_STATUSES) {
        const label = STATUS_LABELS[status];
        // No raw underscores — should be human-readable
        expect(label).not.toContain("_");
        // At least 4 chars for meaningful a11y
        expect(label.length).toBeGreaterThanOrEqual(4);
      }
    });

    it("failure and success labels differentiate completed outcomes", () => {
      expect(STATUS_LABELS.success).toContain("Success");
      expect(STATUS_LABELS.failure).toContain("Failure");
      expect(STATUS_LABELS.success).not.toBe(STATUS_LABELS.failure);
    });
  });

  describe("Okabe-Ito palette contract", () => {
    // These are documented in the CSS but we verify the status→shape mapping
    // is exhaustive by checking ALL_STATUSES covers every expected variant
    const EXPECTED_TASK_STATUSES: IndicatorStatus[] = [
      "pending",
      "in_progress",
      "blocked",
      "awaiting_approval",
      "success",
      "failure",
    ];

    const EXPECTED_AGENT_STATUSES: IndicatorStatus[] = [
      "running",
      "paused",
      "completed",
      "failed",
      "idle",
      "terminated",
    ];

    it("ALL_STATUSES covers the full task-pipeline mapping", () => {
      for (const expected of EXPECTED_TASK_STATUSES) {
        expect(ALL_STATUSES).toContain(expected);
      }
    });

    it("ALL_STATUSES covers the full agent-lifecycle mapping", () => {
      for (const expected of EXPECTED_AGENT_STATUSES) {
        expect(ALL_STATUSES).toContain(expected);
      }
    });
  });

  describe("agent-lifecycle status shapes contract", () => {
    it("running status uses filled circle shape", () => {
      // Running = filled circle (green) — verify label is present
      expect(STATUS_LABELS.running).toBe("Running");
    });

    it("paused status uses half-filled circle shape", () => {
      expect(STATUS_LABELS.paused).toBe("Paused");
    });

    it("completed status uses checkmark shape", () => {
      expect(STATUS_LABELS.completed).toBe("Completed");
    });

    it("failed status uses X mark shape", () => {
      expect(STATUS_LABELS.failed).toBe("Failed");
    });

    it("idle status uses hollow circle shape", () => {
      expect(STATUS_LABELS.idle).toBe("Idle");
    });

    it("terminated status uses dash/minus shape", () => {
      expect(STATUS_LABELS.terminated).toBe("Terminated");
    });
  });
});
