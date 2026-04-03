/**
 * Notification store — heterogeneous notification model for the inbox sidebar.
 * Supports decision, agent_update, build_status, mention, and system notification types.
 */

import { createSignal, createMemo } from "solid-js";
import { subscribe } from "../../lib/ws";

// ---------------------------------------------------------------------------
// Tauri native push — only active when running inside the desktop app.
// Gracefully no-ops in browser/dev mode.
// ---------------------------------------------------------------------------

let _nativePushReady = false;

export async function initNativePush(): Promise<void> {
  try {
    const { isPermissionGranted, requestPermission } = await import("@tauri-apps/plugin-notification");
    let granted = await isPermissionGranted();
    if (!granted) {
      const permission = await requestPermission();
      granted = permission === "granted";
    }
    _nativePushReady = granted;
  } catch {
    // Not running in Tauri desktop — silently skip
  }
}

async function sendNativePush(title: string, body: string): Promise<void> {
  if (!_nativePushReady) return;
  try {
    const { sendNotification } = await import("@tauri-apps/plugin-notification");
    sendNotification({ title, body });
  } catch {
    // ignore
  }
}

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
// localStorage persistence for read/dismissed state
// ---------------------------------------------------------------------------

const NOTIF_READ_KEY = "molt:notif-read";
const NOTIF_DISMISSED_KEY = "molt:notif-dismissed";

function loadSet(key: string): Set<string> {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return new Set();
    return new Set(JSON.parse(raw) as string[]);
  } catch {
    return new Set();
  }
}

function saveSet(key: string, set: Set<string>): void {
  try {
    localStorage.setItem(key, JSON.stringify([...set]));
  } catch {
    // localStorage unavailable; silently ignore
  }
}

const _readIds = loadSet(NOTIF_READ_KEY);
const _dismissedIds = loadSet(NOTIF_DISMISSED_KEY);

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
  _readIds.add(id);
  saveSet(NOTIF_READ_KEY, _readIds);
}

function markAllRead(): void {
  setNotifications((prev) => prev.map((n) => ({ ...n, read: true })));
  notifications().forEach((n) => _readIds.add(n.id));
  saveSet(NOTIF_READ_KEY, _readIds);
}

function dismissNotification(id: string): void {
  setNotifications((prev) => prev.filter((n) => n.id !== id));
  _dismissedIds.add(id);
  saveSet(NOTIF_DISMISSED_KEY, _dismissedIds);
  // Also clean up read tracking for dismissed items
  _readIds.delete(id);
  saveSet(NOTIF_READ_KEY, _readIds);
}

// ---------------------------------------------------------------------------
// Navigate singleton — set by AppLayout via registerNavigate()
// ---------------------------------------------------------------------------

let _navigateFn: ((path: string) => void) | null = null;

export function registerNavigate(fn: (path: string) => void): void {
  _navigateFn = fn;
}

function handleAction(notifId: string, action: NotificationAction): void {
  // Mark as read on any action
  markRead(notifId);
  if (action.kind === "dismiss") {
    dismissNotification(notifId);
    return;
  }
  if (action.handler.startsWith("navigate:")) {
    const path = action.handler.slice("navigate:".length);
    _navigateFn?.(path);
    return;
  }
  // approve/reject handlers: wiring to approval API is a separate task
}

function addNotification(notif: Notification): void {
  setNotifications((prev) => [notif, ...prev]);
  setNewNotifId(notif.id);
  // Clear the pulse animation marker after the animation completes
  setTimeout(() => setNewNotifId(null), 600);
  // Native push for high-priority notifications
  if (notif.priority === "p0" || notif.priority === "p1") {
    void sendNativePush(notif.title, notif.subtitle ?? "");
  }
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
    if (!notif) return;
    if (_dismissedIds.has(notif.id)) return;
    if (_readIds.has(notif.id)) notif.read = true;
    addNotification(notif);
  });
}

// ---------------------------------------------------------------------------
// HTTP initialization
// ---------------------------------------------------------------------------

/**
 * Fetch triage items from the API and seed the notification store.
 * Call once at startup so the inbox is populated before any WS events arrive.
 */
export async function initNotificationsFromTriage(): Promise<void> {
  try {
    const res = await fetch("/api/tasks/triage");
    if (!res.ok) return;
    const data = await res.json() as { items: Array<{
      id: string;
      task_id: string;
      task_name: string;
      agent_name: string;
      stage: string;
      priority: string;
      type: string;
      created_at: string;
      summary: string;
    }> };
    if (!Array.isArray(data.items)) return;

    const existing = new Set(notifications().map((n) => n.id));
    for (const item of data.items) {
      if (existing.has(item.id)) continue;
      if (_dismissedIds.has(item.id)) continue;
      const notif: Notification = {
        id: item.id,
        type: item.type === "decision" ? "decision" : "agent_update",
        priority: (["p0","p1","p2","p3"].includes(item.priority) ? item.priority : "p2") as NotificationPriority,
        title: item.task_name,
        subtitle: item.summary || (item.stage ? `Stage: ${item.stage}` : undefined),
        agentName: item.agent_name || undefined,
        timestamp: item.created_at,
        read: _readIds.has(item.id),
        actions: item.type === "decision"
          ? [
              { label: "Approve", kind: "approve" as ActionKind, handler: `approve:${item.task_id}` },
              { label: "Defer",   kind: "defer"   as ActionKind, handler: `defer:${item.task_id}` },
            ]
          : [
              { label: "Acknowledge", kind: "acknowledge" as ActionKind, handler: `ack:${item.task_id}` },
            ],
      };
      setNotifications((prev) => sortByPriorityAndTime([...prev, notif]));
    }
  } catch {
    // silently ignore
  }
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
  registerNavigate,
};

export type { FilterTab as NotificationFilterTab };
