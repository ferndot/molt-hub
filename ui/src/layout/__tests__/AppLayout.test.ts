/**
 * Tests for AppLayout — attention store integration.
 * Tests run in node environment — no DOM rendering.
 */
import { describe, it, expect } from "vitest";
import { p0Count, p1Count, attentionCount, setP0Count, setP1Count } from "../attentionStore";

describe("AppLayout attention badges", () => {
  it("attentionStore exports expected API shapes", () => {
    expect(typeof p0Count).toBe("function");
    expect(typeof p1Count).toBe("function");
    expect(typeof setP0Count).toBe("function");
    expect(typeof setP1Count).toBe("function");
    expect(typeof attentionCount).toBe("function");
  });

  it("badge count is reactive: updating signals updates attentionCount", () => {
    setP0Count(0);
    setP1Count(0);
    expect(attentionCount()).toBe(0);

    setP0Count(1);
    setP1Count(2);
    expect(attentionCount()).toBe(3);

    setP0Count(0);
    setP1Count(0);
    expect(attentionCount()).toBe(0);
  });

  it("badge shows 0 when no attention items", () => {
    setP0Count(0);
    setP1Count(0);
    const count = attentionCount();
    expect(count).toBe(0);
  });

  it("badge shows correct count for P0-only items", () => {
    setP0Count(5);
    setP1Count(0);
    expect(attentionCount()).toBe(5);
    setP0Count(0);
  });

  it("badge shows correct count for P1-only items", () => {
    setP0Count(0);
    setP1Count(7);
    expect(attentionCount()).toBe(7);
    setP1Count(0);
  });
});
