/**
 * TanStack Virtual helpers for SolidJS.
 *
 * Re-exports `createVirtualizer` from @tanstack/solid-virtual and provides
 * a typed wrapper that will be used by the triage queue and activity feed.
 */

export { createVirtualizer } from "@tanstack/solid-virtual";
export type {
  VirtualizerOptions,
  Virtualizer,
} from "@tanstack/solid-virtual";

// ---------------------------------------------------------------------------
// Convenience types for list virtualisation
// ---------------------------------------------------------------------------

/** Options for a vertical virtual list. */
export interface VirtualListConfig {
  /** Total number of items. */
  count: number;
  /** Estimated row height in pixels (used before measurement). */
  estimateSize: () => number;
  /** Ref to the scroll container element. */
  getScrollElement: () => HTMLElement | null;
  /** Overscan rows above/below visible area (default 3). */
  overscan?: number;
}
