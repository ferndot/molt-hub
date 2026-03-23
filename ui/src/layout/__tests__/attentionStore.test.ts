/**
 * Tests for the attention store (badge count reactivity).
 * Pure Solid signals — no DOM needed, runs in node environment.
 */
import { describe, it, expect } from "vitest";
import { createRoot } from "solid-js";
import { p0Count, p1Count, attentionCount, setP0Count, setP1Count } from "../attentionStore";

describe("attentionStore", () => {
  it("attentionCount starts at 0 (or whatever it was reset to)", () => {
    createRoot((dispose) => {
      // Reset state
      setP0Count(0);
      setP1Count(0);
      expect(attentionCount()).toBe(0);
      dispose();
    });
  });

  it("attentionCount is sum of p0 and p1", () => {
    createRoot((dispose) => {
      setP0Count(0);
      setP1Count(0);
      expect(attentionCount()).toBe(0);

      setP0Count(2);
      expect(attentionCount()).toBe(2);

      setP1Count(3);
      expect(attentionCount()).toBe(5);

      setP0Count(0);
      expect(attentionCount()).toBe(3);

      // cleanup
      setP1Count(0);
      dispose();
    });
  });

  it("p0Count and p1Count are independent signals", () => {
    createRoot((dispose) => {
      setP0Count(10);
      setP1Count(5);

      expect(p0Count()).toBe(10);
      expect(p1Count()).toBe(5);

      // cleanup
      setP0Count(0);
      setP1Count(0);
      dispose();
    });
  });
});
