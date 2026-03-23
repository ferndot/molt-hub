/**
 * useFocusManager — SolidJS primitive for tracking selected item index.
 *
 * Pure logic, no DOM — can be tested in node environment.
 */

import { createSignal } from "solid-js";

export interface FocusManager {
  selectedIndex: () => number;
  moveUp: () => void;
  moveDown: () => void;
  select: (index: number) => void;
  reset: () => void;
}

/**
 * createFocusManager returns a reactive focus manager for a list of `itemCount` items.
 * The `itemCount` parameter is a getter (signal or memo) so it stays reactive.
 */
export function createFocusManager(itemCount: () => number): FocusManager {
  const [selectedIndex, setSelectedIndex] = createSignal(-1);

  const clamp = (index: number) => {
    const count = itemCount();
    if (count === 0) return -1;
    return Math.max(0, Math.min(index, count - 1));
  };

  return {
    selectedIndex,
    moveUp() {
      setSelectedIndex((prev) => {
        if (prev <= 0) return 0;
        return clamp(prev - 1);
      });
    },
    moveDown() {
      setSelectedIndex((prev) => {
        const count = itemCount();
        if (count === 0) return -1;
        if (prev === -1) return 0;
        return clamp(prev + 1);
      });
    },
    select(index: number) {
      setSelectedIndex(clamp(index));
    },
    reset() {
      setSelectedIndex(-1);
    },
  };
}
