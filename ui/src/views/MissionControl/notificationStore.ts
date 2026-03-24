/**
 * Notification store — heterogeneous notification model for the inbox sidebar.
 * Supports decision, agent_update, build_status, mention, and system notification types.
 */

import { createSignal, createMemo } from "solid-js";
import { subscribe } from "../../lib/ws";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type NotificationType =
  | "decision"       // Needs human approval (existing triage items)
  | "agent_update"   // Agent completed a stage, needs review
  | "build_status"   // CI/CD result
  | "mention"        // Someone mentioned you in a comment
  | "system";        // System alerts (cost threshold, health degradation)

export type NotificationPriority = "p0" | "p1" | "p2" | "p3";

export type ActionKind =
  | "approve"
  | "reject"
  | "redirect"
  | "defer"
  | "acknowledge"
  | "dismiss"
  | "view";

export interface NotificationAction {
  label: string;
  kind: ActionKind;
  handler: string; // action identifier
}

export interface Notification {
  id: string;
  type: NotificationType;
  priority: NotificationPriority;
  title: string;
  subtitle?: string;
  agentName?: string;
  timestamp: string;
  read: boolean;
  actions?: NotificationAction[];
}

export type FilterTab = "all" | "decisions" | "updates" | "alerts";

// ---------------------------------------------------------------------------
// Store signals
// ---------------------------------------------------------------------------

const [notifications, setNotifications] = createSignal<Notification[]>([]);
const [activeFilter, setActiveFilter] = createSignal<FilterTab>("all");
const [newNotifId, setNewNotifId] = createSignal<string | null>(null);

// ---------------------------------------------------------------------------
// Derived state
// ---------------------------------------------------------------------------

const PRIORITY_ORDER: Record<string, number> = { p0: 0, p1: 1, p2: 2, p3: 3 };

function sortByPriorityAndTime(items: Notification[]): Notification[] {
  return [...items].sort((a, b) => {
    const pd = PRIORITY_ORDER[a.priority] - PRIORITY_ORDER[b.priority];
    if (pd !== 0) return pd;
    return new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime();
  });
}

const unreadCount = createMemo(() =>
  notifications().filter((n) => !n.read).length,
);

const filteredNotifications = createMemo(() => {
  const filter = activeFilter();
  const all = notifications();
  let filtered: Notification[];

  switch (filter) {
    case "decisions":
      filtered = all.filter((n) => n.type === "decision");
      break;
    case "updates":
      filtered = all.filter((n) => n.type === "agent_update" || n.type === "build_status");
      break;
    case "alerts":
      filtered = all.filter((n) => n.type === "mention" || n.type === "system");
      break;
    default:
      filtered = all;
  }

  return sortByPriorityAndTime(filtered);
});

const countForTab = createMemo(() => {
  const all = notifications();
  return {
    all: all.length,
    decisions: all.filter((n) => n.type === "decision").length,
    updates: all.filter((n) => n.type === "agent_update" || n.type === "build_status").length,
    alerts: all.filter((n) => n.type === "mention" || n.type === "system").length,
  };
});

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

function markRead(id: string): void {
  setNotifications((prev) =>
    prev.map((n) => (n.id === id ? { ...n, read: true } : n)),
  );
}

function markAllRead(): void {
  setNotifications((prev) => prev.map((n) => ({ ...n, read: true })));
}

function dismissNotification(id: string): void {
  setNotifications((prev) => prev.filter((n) => n.id !== id));
}

function handleAction(notifId: string, action: NotificationAction): void {
  // Mark as read on any action
  markRead(notifId);
  if (action.kind === "dismiss") {
    dismissNotification(notifId);
  }
  // In a real app, dispatch to the appropriate handler via the action.handler string
  // For now this is a no-op beyond marking read
}

function addNotification(notif: Notification): void {
  setNotifications((prev) => [notif, ...prev]);
  setNewNotifId(notif.id);
  // Clear the pulse animation marker after the animation completes
  setTimeout(() => setNewNotifId(null), 600);
}

// ---------------------------------------------------------------------------
// WebSocket integration
// ---------------------------------------------------------------------------

const VALID_TYPES = new Set<NotificationType>(["decision", "agent_update", "build_status", "mention", "system"]);
const VALID_PRIORITIES = new Set<NotificationPriority>(["p0", "p1", "p2", "p3"]);

/**
 * Parse a raw WebSocket payload into a Notification, returning null if invalid.
 */
function parseWsNotification(payload: unknown): Notification | null {
  if (payload == null || typeof payload !== "object") return null;
  const p = payload as Record<string, unknown>;
  const title = typeof p.title === "string" ? p.title.trim() : "";
  if (!title) return null;

  return {
    id: typeof p.id === "string" ? p.id : `ws-notif-${Date.now()}-${Math.random().toString(36).slice(2)}`,
    type: VALID_TYPES.has(p.type as NotificationType) ? (p.type as NotificationType) : "system",
    priority: VALID_PRIORITIES.has(p.priority as NotificationPriority) ? (p.priority as NotificationPriority) : "p1",
    title,
    subtitle: typeof p.subtitle === "string" ? p.subtitle : undefined,
    agentName: typeof p.agentName === "string" ? p.agentName : undefined,
    timestamp: typeof p.timestamp === "string" ? p.timestamp : new Date().toISOString(),
    read: false,
    actions: Array.isArray(p.actions) ? (p.actions as NotificationAction[]) : undefined,
  };
}

/**
 * Subscribe to the "notification:*" WebSocket topic.
 * Returns an unsubscribe function for cleanup.
 */
function connectNotificationsWs(): () => void {
  return subscribe("notification:*", (msg) => {
    if (msg.type !== "event") return;
    const notif = parseWsNotification(msg.payload);
    if (notif) addNotification(notif);
  });
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

export {
  notifications,
  filteredNotifications,
  unreadCount,
  countForTab,
  activeFilter,
  setActiveFilter,
  newNotifId,
  markRead,
  markAllRead,
  dismissNotification,
  handleAction,
  addNotification,
  connectNotificationsWs,
};

export type { FilterTab as NotificationFilterTab };
