/**
 * chords.ts — chord key parsing utilities.
 *
 * A chord is two keys pressed in sequence within a time window (default 500ms).
 * Example: "g" then "t" = goto triage.
 *
 * Pure logic — no DOM, testable in node environment.
 */

export interface ChordState {
  pending: string | null;
  timestamp: number;
}

export type ChordAction = "goto-triage" | "goto-board" | "goto-agents" | "goto-code-chat";

export interface ChordResult {
  action: ChordAction;
  consumed: true;
}

const CHORD_WINDOW_MS = 500;

/** Chord map: first key -> second key -> action */
const CHORD_MAP: Record<string, Record<string, ChordAction>> = {
  g: {
    t: "goto-triage",
    b: "goto-board",
    a: "goto-agents",
    c: "goto-code-chat",
  },
};

/**
 * processChord takes the current chord state and a new key, and returns either:
 * - { next: ChordState, result: ChordResult } when a chord completes
 * - { next: ChordState, result: null } when waiting for second key or chord failed
 */
export function processChord(
  state: ChordState,
  key: string,
  now: number = Date.now(),
): { next: ChordState; result: ChordResult | null } {
  // If we have a pending first key and it's still within the window
  if (state.pending !== null && now - state.timestamp <= CHORD_WINDOW_MS) {
    const chordMap = CHORD_MAP[state.pending];
    if (chordMap) {
      const action = chordMap[key];
      if (action) {
        // Chord completed
        return {
          next: { pending: null, timestamp: 0 },
          result: { action, consumed: true },
        };
      }
    }
    // Chord failed — check if this key starts a new chord
  }

  // Check if key starts a chord
  if (CHORD_MAP[key]) {
    return {
      next: { pending: key, timestamp: now },
      result: null,
    };
  }

  // Not a chord starter
  return {
    next: { pending: null, timestamp: 0 },
    result: null,
  };
}

/** Create a fresh chord state. */
export function createChordState(): ChordState {
  return { pending: null, timestamp: 0 };
}
