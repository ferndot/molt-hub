/**
 * Shared reactive store for attention badge counts.
 * Derives counts from the notification store for the status bar badge.
 * Components read from here; never create their own local copies.
 */
import { createSignal } from "solid-js";
import { unreadCount } from "../views/MissionControl/notificationStore";

// P0 count (critical — shown in red)
export const [p0Count, setP0Count] = createSignal<number>(0);

// P1 count (high — shown in orange)
export const [p1Count, setP1Count] = createSignal<number>(0);

/** Total urgent items shown on the Triage badge */
export const attentionCount = () => p0Count() + p1Count();

/**
 * Unread notification count — used by the inbox toggle badge in StatusBar.
 * Re-exported from notificationStore for convenience.
 */
export { unreadCount };
