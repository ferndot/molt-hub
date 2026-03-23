/**
 * Notification store — heterogeneous notification model for the inbox sidebar.
 * Supports decision, agent_update, build_status, mention, and system notification types.
 */

import { createSignal, createMemo } from "solid-js";

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
// Mock data
// ---------------------------------------------------------------------------

function minutesAgo(n: number): string {
  return new Date(Date.now() - n * 60_000).toISOString();
}

const MOCK_NOTIFICATIONS: Notification[] = [
  {
    id: "notif-1",
    type: "decision",
    priority: "p0",
    title: "Approve deployment to production",
    subtitle: "v2.4.1 release candidate",
    agentName: "deploy-bot",
    timestamp: minutesAgo(2),
    read: false,
    actions: [
      { label: "Approve", kind: "approve", handler: "approve:notif-1" },
      { label: "Reject", kind: "reject", handler: "reject:notif-1" },
      { label: "Defer", kind: "defer", handler: "defer:notif-1" },
    ],
  },
  {
    id: "notif-2",
    type: "agent_update",
    priority: "p1",
    title: "frontend completed code-review",
    subtitle: "3 issues found, 1 auto-fixed",
    agentName: "frontend",
    timestamp: minutesAgo(8),
    read: false,
    actions: [
      { label: "View", kind: "view", handler: "view:notif-2" },
    ],
  },
  {
    id: "notif-3",
    type: "build_status",
    priority: "p1",
    title: "CI failed on feature/auth-flow",
    subtitle: "2 test failures in auth.test.ts",
    timestamp: minutesAgo(15),
    read: false,
    actions: [
      { label: "View", kind: "view", handler: "view:notif-3" },
    ],
  },
  {
    id: "notif-4",
    type: "mention",
    priority: "p2",
    title: "Alice mentioned you in PR #42",
    subtitle: "\"Can you review the API changes?\"",
    timestamp: minutesAgo(32),
    read: false,
    actions: [
      { label: "View", kind: "view", handler: "view:notif-4" },
    ],
  },
  {
    id: "notif-5",
    type: "system",
    priority: "p1",
    title: "Cost threshold 80% reached",
    subtitle: "$847 of $1,000 monthly budget",
    timestamp: minutesAgo(45),
    read: true,
    actions: [
      { label: "Acknowledge", kind: "acknowledge", handler: "ack:notif-5" },
    ],
  },
  {
    id: "notif-6",
    type: "decision",
    priority: "p2",
    title: "Approve schema migration",
    subtitle: "Add users.avatar_url column",
    agentName: "db-agent",
    timestamp: minutesAgo(60),
    read: true,
    actions: [
      { label: "Approve", kind: "approve", handler: "approve:notif-6" },
      { label: "Reject", kind: "reject", handler: "reject:notif-6" },
      { label: "Defer", kind: "defer", handler: "defer:notif-6" },
    ],
  },
  {
    id: "notif-7",
    type: "agent_update",
    priority: "p3",
    title: "backend completed testing",
    subtitle: "All 142 tests passing",
    agentName: "backend",
    timestamp: minutesAgo(90),
    read: true,
    actions: [
      { label: "View", kind: "view", handler: "view:notif-7" },
    ],
  },
  {
    id: "notif-8",
    type: "build_status",
    priority: "p3",
    title: "CI passed on main",
    subtitle: "Build #1847 — 4m 12s",
    timestamp: minutesAgo(120),
    read: true,
    actions: [
      { label: "View", kind: "view", handler: "view:notif-8" },
    ],
  },
  {
    id: "notif-9",
    type: "system",
    priority: "p0",
    title: "Agent health degraded",
    subtitle: "frontend agent response time >30s",
    timestamp: minutesAgo(5),
    read: false,
    actions: [
      { label: "Acknowledge", kind: "acknowledge", handler: "ack:notif-9" },
    ],
  },
  {
    id: "notif-10",
    type: "mention",
    priority: "p3",
    title: "Bob commented on MOLT-128",
    subtitle: "\"Looks good, merging now\"",
    timestamp: minutesAgo(180),
    read: true,
    actions: [
      { label: "View", kind: "view", handler: "view:notif-10" },
    ],
  },
];

// ---------------------------------------------------------------------------
// Store signals
// ---------------------------------------------------------------------------

const [notifications, setNotifications] = createSignal<Notification[]>(MOCK_NOTIFICATIONS);
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
};

export type { FilterTab as NotificationFilterTab };
