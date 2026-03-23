/**
 * useGridFocusManager — SolidJS primitive for tracking focus in a 2D column grid.
 *
 * Each column can have a different number of rows. Navigation wraps/clamps
 * appropriately and skips empty columns when moving left/right.
 *
 * Pure logic, no DOM — can be tested in node environment.
 */

import { createSignal } from "solid-js";

export interface GridFocusManager {
  colIndex: () => number;
  rowIndex: () => number;
  moveLeft: () => void;
  moveRight: () => void;
  moveUp: () => void;
  moveDown: () => void;
  select: (col: number, row: number) => void;
  reset: () => void;
}

/**
 * createGridFocusManager returns a reactive focus manager for a 2D grid
 * defined by `columnCounts` — an array where each element is the number
 * of rows in that column. The getter is reactive (signal or memo).
 */
export function createGridFocusManager(
  columnCounts: () => number[],
): GridFocusManager {
  const [colIndex, setColIndex] = createSignal(-1);
  const [rowIndex, setRowIndex] = createSignal(-1);

  const clampRow = (col: number, row: number): number => {
    const counts = columnCounts();
    if (col < 0 || col >= counts.length) return -1;
    const len = counts[col];
    if (len === 0) return -1;
    return Math.max(0, Math.min(row, len - 1));
  };

  return {
    colIndex,
    rowIndex,

    moveDown() {
      const counts = columnCounts();
      if (counts.length === 0) return;

      const col = colIndex();
      const row = rowIndex();

      // From initial state, go to (0, 0) — but find first non-empty column
      if (col === -1 && row === -1) {
        for (let c = 0; c < counts.length; c++) {
          if (counts[c] > 0) {
            setColIndex(c);
            setRowIndex(0);
            return;
          }
        }
        return; // all columns empty
      }

      if (col < 0 || col >= counts.length) return;
      const len = counts[col];
      if (len === 0) return;
      const next = Math.min(row + 1, len - 1);
      setRowIndex(next);
    },

    moveUp() {
      const col = colIndex();
      const row = rowIndex();
      if (col === -1 && row === -1) return;

      const counts = columnCounts();
      if (col < 0 || col >= counts.length) return;
      if (counts[col] === 0) return;

      setRowIndex(Math.max(0, row - 1));
    },

    moveRight() {
      const counts = columnCounts();
      if (counts.length === 0) return;

      const col = colIndex();
      const row = rowIndex();
      if (col === -1 && row === -1) return;

      // Find next non-empty column to the right
      for (let c = col + 1; c < counts.length; c++) {
        if (counts[c] > 0) {
          setColIndex(c);
          setRowIndex(clampRow(c, row));
          return;
        }
      }
      // No non-empty column found — stay put
    },

    moveLeft() {
      const counts = columnCounts();
      if (counts.length === 0) return;

      const col = colIndex();
      const row = rowIndex();
      if (col === -1 && row === -1) return;

      // Find next non-empty column to the left
      for (let c = col - 1; c >= 0; c--) {
        if (counts[c] > 0) {
          setColIndex(c);
          setRowIndex(clampRow(c, row));
          return;
        }
      }
      // No non-empty column found — stay put
    },

    select(col: number, row: number) {
      const counts = columnCounts();
      if (counts.length === 0) {
        setColIndex(-1);
        setRowIndex(-1);
        return;
      }
      const clampedCol = Math.max(0, Math.min(col, counts.length - 1));
      const clampedRow = clampRow(clampedCol, row);
      setColIndex(clampedCol);
      setRowIndex(clampedRow);
    },

    reset() {
      setColIndex(-1);
      setRowIndex(-1);
    },
  };
}
