/**
 * Tests for notificationStore — tab filtering, unread count, and actions.
 *
 * NOTE: passesAttentionLevel and parseWsNotification were removed from the
 * store. Tests for those helpers were deleted as the functionality no longer
 * exists in notificationStore.
 */

import { describe, it, expect } from "vitest";
import {
  notifications,
  unreadCount,
  addNotification,
  markRead,
  markAllRead,
  dismissNotification,
} from "../notificationStore";
import type { Notification } from "../notificationStore";

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

function makeNotif(overrides: Partial<Notification> = {}): Notification {
  return {
    id: `test-${Math.random().toString(36).slice(2)}`,
    type: "system",
    priority: "p1",
    title: "Test notification",
    timestamp: new Date().toISOString(),
    read: false,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// addNotification / markRead / markAllRead / dismissNotification
// ---------------------------------------------------------------------------

describe("notificationStore actions", () => {
  it("addNotification adds to the list", () => {
    const before = notifications().length;
    addNotification(makeNotif({ id: "add-test" }));
    expect(notifications().length).toBe(before + 1);
    expect(notifications().some((n) => n.id === "add-test")).toBe(true);
  });

  it("markRead marks a single notification as read", () => {
    const n = makeNotif({ id: "mark-read-test" });
    addNotification(n);
    markRead("mark-read-test");
    expect(notifications().find((x) => x.id === "mark-read-test")?.read).toBe(true);
  });

  it("markAllRead marks every notification as read", () => {
    addNotification(makeNotif());
    addNotification(makeNotif());
    markAllRead();
    for (const n of notifications()) {
      expect(n.read).toBe(true);
    }
  });

  it("dismissNotification removes the notification", () => {
    const n = makeNotif({ id: "dismiss-test" });
    addNotification(n);
    dismissNotification("dismiss-test");
    expect(notifications().some((x) => x.id === "dismiss-test")).toBe(false);
  });

  it("unreadCount reflects current unread notifications", () => {
    markAllRead();
    const before = unreadCount();
    addNotification(makeNotif({ id: "unread-count-test" }));
    expect(unreadCount()).toBe(before + 1);
    markRead("unread-count-test");
    expect(unreadCount()).toBe(before);
  });
});
