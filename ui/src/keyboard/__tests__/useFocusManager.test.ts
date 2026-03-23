/**
 * Tests for useFocusManager — list focus/selection primitive.
 * Runs in node environment via SolidJS reactive primitives.
 */

import { describe, it, expect } from "vitest";
import { createRoot } from "solid-js";
import { createFocusManager } from "../useFocusManager";

describe("createFocusManager", () => {
  describe("initial state", () => {
    it("starts with selectedIndex -1 (nothing selected)", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 5);
        expect(fm.selectedIndex()).toBe(-1);
        dispose();
      });
    });
  });

  describe("moveDown", () => {
    it("moves from -1 to 0 on first moveDown", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 5);
        fm.moveDown();
        expect(fm.selectedIndex()).toBe(0);
        dispose();
      });
    });

    it("increments index on subsequent calls", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 5);
        fm.moveDown();
        fm.moveDown();
        expect(fm.selectedIndex()).toBe(1);
        dispose();
      });
    });

    it("clamps at last item (itemCount - 1)", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 3);
        fm.moveDown();
        fm.moveDown();
        fm.moveDown();
        fm.moveDown(); // would be 3, but max is 2
        expect(fm.selectedIndex()).toBe(2);
        dispose();
      });
    });

    it("stays at -1 when itemCount is 0", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 0);
        fm.moveDown();
        expect(fm.selectedIndex()).toBe(-1);
        dispose();
      });
    });
  });

  describe("moveUp", () => {
    it("clamps at 0 when already at first item", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 5);
        fm.select(0);
        fm.moveUp();
        expect(fm.selectedIndex()).toBe(0);
        dispose();
      });
    });

    it("decrements when above first item", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 5);
        fm.select(3);
        fm.moveUp();
        expect(fm.selectedIndex()).toBe(2);
        dispose();
      });
    });

    it("clamps to 0 when selectedIndex is -1", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 5);
        // -1 -> moveUp -> should be 0 (clamped)
        fm.moveUp();
        expect(fm.selectedIndex()).toBe(0);
        dispose();
      });
    });
  });

  describe("select", () => {
    it("sets selectedIndex to given value", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 10);
        fm.select(7);
        expect(fm.selectedIndex()).toBe(7);
        dispose();
      });
    });

    it("clamps to last item if out of bounds", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 5);
        fm.select(99);
        expect(fm.selectedIndex()).toBe(4);
        dispose();
      });
    });

    it("clamps to 0 if negative index provided", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 5);
        fm.select(-5);
        expect(fm.selectedIndex()).toBe(0);
        dispose();
      });
    });
  });

  describe("reset", () => {
    it("returns selectedIndex to -1", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 5);
        fm.select(3);
        fm.reset();
        expect(fm.selectedIndex()).toBe(-1);
        dispose();
      });
    });
  });

  describe("boundary conditions", () => {
    it("handles single item list correctly", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 1);
        fm.moveDown();
        expect(fm.selectedIndex()).toBe(0);
        fm.moveDown(); // can't go further
        expect(fm.selectedIndex()).toBe(0);
        fm.moveUp();
        expect(fm.selectedIndex()).toBe(0);
        dispose();
      });
    });

    it("moveDown then moveUp returns to previous position", () => {
      createRoot((dispose) => {
        const fm = createFocusManager(() => 5);
        fm.select(2);
        fm.moveDown();
        expect(fm.selectedIndex()).toBe(3);
        fm.moveUp();
        expect(fm.selectedIndex()).toBe(2);
        dispose();
      });
    });
  });
});
