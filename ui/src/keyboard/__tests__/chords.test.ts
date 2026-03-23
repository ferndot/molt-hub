/**
 * Tests for chord key parsing logic.
 * Runs in node environment — no DOM.
 */

import { describe, it, expect } from "vitest";
import { processChord, createChordState } from "../chords";

const T0 = 1000; // reference timestamp
const WITHIN = T0 + 400; // 400ms later — within 500ms window
const EXPIRED = T0 + 600; // 600ms later — outside window

describe("chord key parsing", () => {
  describe("first key handling", () => {
    it("returns pending state when first chord key is pressed", () => {
      const state = createChordState();
      const { next, result } = processChord(state, "g", T0);
      expect(next.pending).toBe("g");
      expect(next.timestamp).toBe(T0);
      expect(result).toBeNull();
    });

    it("returns null pending for non-chord keys", () => {
      const state = createChordState();
      const { next, result } = processChord(state, "j", T0);
      expect(next.pending).toBeNull();
      expect(result).toBeNull();
    });
  });

  describe("chord completion within window", () => {
    it("g then t = goto-triage", () => {
      let state = createChordState();
      ({ next: state } = processChord(state, "g", T0));
      const { next, result } = processChord(state, "t", WITHIN);
      expect(result).not.toBeNull();
      expect(result?.action).toBe("goto-triage");
      expect(result?.consumed).toBe(true);
      expect(next.pending).toBeNull();
    });

    it("g then b = goto-board", () => {
      let state = createChordState();
      ({ next: state } = processChord(state, "g", T0));
      const { result } = processChord(state, "b", WITHIN);
      expect(result?.action).toBe("goto-board");
    });

    it("g then a = goto-agents", () => {
      let state = createChordState();
      ({ next: state } = processChord(state, "g", T0));
      const { result } = processChord(state, "a", WITHIN);
      expect(result?.action).toBe("goto-agents");
    });
  });

  describe("chord expiry", () => {
    it("does not complete a chord after timeout window expires", () => {
      let state = createChordState();
      ({ next: state } = processChord(state, "g", T0));
      const { result, next } = processChord(state, "t", EXPIRED);
      // "t" is not a chord starter, so pending should be cleared
      expect(result).toBeNull();
      expect(next.pending).toBeNull();
    });

    it("expired chord followed by new chord starter resets correctly", () => {
      let state = createChordState();
      ({ next: state } = processChord(state, "g", T0));
      // "g" again after expiry — should start a new chord
      ({ next: state } = processChord(state, "g", EXPIRED));
      expect(state.pending).toBe("g");
    });
  });

  describe("invalid chord sequences", () => {
    it("g then x (unknown second key) returns no result", () => {
      let state = createChordState();
      ({ next: state } = processChord(state, "g", T0));
      const { result, next } = processChord(state, "x", WITHIN);
      expect(result).toBeNull();
      // "x" is not a chord starter either
      expect(next.pending).toBeNull();
    });

    it("fresh state, pressing second key alone returns no result", () => {
      const state = createChordState();
      const { result } = processChord(state, "t", T0);
      expect(result).toBeNull();
    });
  });

  describe("createChordState", () => {
    it("returns a state with null pending and zero timestamp", () => {
      const state = createChordState();
      expect(state.pending).toBeNull();
      expect(state.timestamp).toBe(0);
    });
  });
});
