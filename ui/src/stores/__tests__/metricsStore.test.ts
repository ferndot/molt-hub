/**
 * Tests for metricsStore — pure helpers and reactive signals.
 *
 * Uses createRoot to properly scope SolidJS reactive context.
 */

import { describe, it, expect, beforeEach } from "vitest";
import { createRoot } from "solid-js";

// ---------------------------------------------------------------------------
// formatMemory
// ---------------------------------------------------------------------------

describe("formatMemory", () => {
  it("formats bytes in the gigabyte range with one decimal", async () => {
    const { formatMemory } = await import("../metricsStore");
    expect(formatMemory(1_288_490_189)).toBe("1.2G");
    expect(formatMemory(2_147_483_648)).toBe("2.0G");
  });

  it("formats megabytes without decimals", async () => {
    const { formatMemory } = await import("../metricsStore");
    expect(formatMemory(524_288_000)).toBe("500M");
    expect(formatMemory(10_485_760)).toBe("10M");
  });

  it("formats kilobytes without decimals", async () => {
    const { formatMemory } = await import("../metricsStore");
    expect(formatMemory(512_000)).toBe("500K");
  });
});

// ---------------------------------------------------------------------------
// cpuLevel
// ---------------------------------------------------------------------------

describe("cpuLevel", () => {
  it("returns normal for usage below 70%", async () => {
    const { cpuLevel } = await import("../metricsStore");
    expect(cpuLevel(0)).toBe("normal");
    expect(cpuLevel(45)).toBe("normal");
    expect(cpuLevel(69)).toBe("normal");
  });

  it("returns warning for usage 70–89%", async () => {
    const { cpuLevel } = await import("../metricsStore");
    expect(cpuLevel(70)).toBe("warning");
    expect(cpuLevel(85)).toBe("warning");
    expect(cpuLevel(89)).toBe("warning");
  });

  it("returns critical for usage >= 90%", async () => {
    const { cpuLevel } = await import("../metricsStore");
    expect(cpuLevel(90)).toBe("critical");
    expect(cpuLevel(99)).toBe("critical");
    expect(cpuLevel(100)).toBe("critical");
  });
});

// ---------------------------------------------------------------------------
// updateMetrics
// ---------------------------------------------------------------------------

describe("updateMetrics", () => {
  it("updates active agent count", async () => {
    const { updateMetrics, activeAgentCount } = await import("../metricsStore");
    createRoot((dispose) => {
      updateMetrics({ activeAgentCount: 7 });
      expect(activeAgentCount()).toBe(7);
      // restore default
      updateMetrics({ activeAgentCount: 3 });
      dispose();
    });
  });

  it("updates cpu usage", async () => {
    const { updateMetrics, cpuUsage } = await import("../metricsStore");
    createRoot((dispose) => {
      updateMetrics({ cpuUsage: 88 });
      expect(cpuUsage()).toBe(88);
      // restore default
      updateMetrics({ cpuUsage: 45 });
      dispose();
    });
  });

  it("updates memory bytes", async () => {
    const { updateMetrics, memoryUsage } = await import("../metricsStore");
    createRoot((dispose) => {
      updateMetrics({ memoryBytes: 2_147_483_648 });
      expect(memoryUsage()).toBe(2_147_483_648);
      // restore default
      updateMetrics({ memoryBytes: 1_288_490_189 });
      dispose();
    });
  });

  it("ignores fields not present in update object", async () => {
    const { updateMetrics, activeAgentCount, cpuUsage } = await import("../metricsStore");
    createRoot((dispose) => {
      const before = activeAgentCount();
      updateMetrics({ cpuUsage: 55 });
      expect(activeAgentCount()).toBe(before);
      // restore
      updateMetrics({ cpuUsage: 45 });
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// pendingDecisionCount — derived from attentionStore
// ---------------------------------------------------------------------------

describe("pendingDecisionCount", () => {
  it("returns 0 when both attention counts are zero", async () => {
    const { pendingDecisionCount } = await import("../metricsStore");
    const { setP0Count, setP1Count } = await import("../../layout/attentionStore");
    createRoot((dispose) => {
      setP0Count(0);
      setP1Count(0);
      expect(pendingDecisionCount()).toBe(0);
      dispose();
    });
  });

  it("sums p0 and p1 counts", async () => {
    const { pendingDecisionCount } = await import("../metricsStore");
    const { setP0Count, setP1Count } = await import("../../layout/attentionStore");
    createRoot((dispose) => {
      setP0Count(2);
      setP1Count(3);
      expect(pendingDecisionCount()).toBe(5);
      // restore
      setP0Count(0);
      setP1Count(0);
      dispose();
    });
  });
});
