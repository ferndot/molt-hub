/**
 * InboxSidebar — collapsible right sidebar showing a heterogeneous
 * notification panel with filtering, inline actions, and read/unread state.
 * Resizable by dragging its left edge.
 */

import { For, Show, Switch, Match, createSignal, createMemo, type Component } from "solid-js";
import { useNavigate } from "@solidjs/router";
import {
  TbOutlineCheck,
  TbOutlineX,
  TbOutlineClock,
  TbOutlineAlertCircle,
  TbOutlineRobot,
  TbOutlineCircleCheck,
  TbOutlineCircleX,
  TbOutlineMessage,
  TbOutlineAlertTriangle,
  TbOutlineEye,
} from "solid-icons/tb";
import type { Notification, NotificationAction, FilterTab } from "./notificationStore";
import {
  filteredNotifications,
  unreadCount,
  countForTab,
  activeFilter,
  setActiveFilter,
  markRead,
  markAllRead,
  handleAction,
  newNotifId,
} from "./notificationStore";
import {
  settingsState,
  setInboxSidebarWidth,
  INBOX_SIDEBAR_MIN,
  INBOX_SIDEBAR_MAX,
} from "../Settings/settingsStore";
import { PriorityBadge } from "../../components/PriorityBadge";
import type { PriorityLevel } from "../../components/PriorityBadge";
import styles from "./InboxSidebar.module.css";

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface InboxSidebarProps {
  collapsed: boolean;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const priorityClass = (priority: string): string => {
  switch (priority) {
    case "p0": return styles.priorityP0;
    case "p1": return styles.priorityP1;
    case "p2": return styles.priorityP2;
    case "p3": return styles.priorityP3;
    default:   return styles.priorityP3;
  }
};

function relativeTime(isoTimestamp: string): string {
  const diff = Date.now() - new Date(isoTimestamp).getTime();
  const minutes = Math.floor(diff / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

const FILTER_TABS: { key: FilterTab; label: string }[] = [
  { key: "all", label: "All" },
  { key: "decisions", label: "Decisions" },
  { key: "updates", label: "Updates" },
  { key: "alerts", label: "Alerts" },
];

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

const NotificationIcon: Component<{ notif: Notification }> = (props) => (
  <Switch>
    <Match when={props.notif.type === "decision"}>
      <TbOutlineAlertCircle size={16} class={styles.iconDecision} />
    </Match>
    <Match when={props.notif.type === "agent_update"}>
      <TbOutlineRobot size={16} class={styles.iconAgent} />
    </Match>
    <Match when={props.notif.type === "build_status" && props.notif.title.toLowerCase().includes("passed")}>
      <TbOutlineCircleCheck size={16} class={styles.iconBuildPass} />
    </Match>
    <Match when={props.notif.type === "build_status"}>
      <TbOutlineCircleX size={16} class={styles.iconBuildFail} />
    </Match>
    <Match when={props.notif.type === "mention"}>
      <TbOutlineMessage size={16} class={styles.iconMention} />
    </Match>
    <Match when={props.notif.type === "system"}>
      <TbOutlineAlertTriangle size={16} class={styles.iconSystem} />
    </Match>
  </Switch>
);

const ActionButton: Component<{
  action: NotificationAction;
  notifId: string;
}> = (props) => {
  const kindClass = (): string => {
    switch (props.action.kind) {
      case "approve": return styles.approveBtn;
      case "reject":  return styles.rejectBtn;
      case "defer":   return styles.deferBtn;
      case "acknowledge": return styles.ackBtn;
      case "view":    return styles.viewBtn;
      case "dismiss": return styles.dismissBtn;
      default:        return "";
    }
  };

  const icon = () => {
    switch (props.action.kind) {
      case "approve": return <TbOutlineCheck size={12} />;
      case "reject":  return <TbOutlineX size={12} />;
      case "defer":   return <TbOutlineClock size={12} />;
      case "acknowledge": return <TbOutlineCheck size={12} />;
      case "view":    return <TbOutlineEye size={12} />;
      case "dismiss": return <TbOutlineX size={12} />;
      default:        return null;
    }
  };

  return (
    <button
      class={`${styles.actionBtn} ${kindClass()}`}
      onClick={(e) => {
        e.stopPropagation();
        handleAction(props.notifId, props.action);
      }}
      title={props.action.label}
    >
      {icon()}
      <span class={styles.actionLabel}>{props.action.label}</span>
    </button>
  );
};

const NotificationItem: Component<{ notif: Notification }> = (props) => {
  const isNew = createMemo(() => newNotifId() === props.notif.id);
  const navigate = useNavigate();

  return (
    <div
      class={styles.notifItem}
      classList={{
        [styles.unread]: !props.notif.read,
        [styles.notifPulse]: isNew(),
      }}
      onClick={() => {
        markRead(props.notif.id);
        if (props.notif.agentName) {
          navigate(`/agents/${props.notif.agentName}`);
        }
      }}
      role="listitem"
      data-notif-id={props.notif.id}
    >
      <div class={styles.notifRow}>
        <NotificationIcon notif={props.notif} />
        <div class={styles.notifContent}>
          <div class={styles.notifTitleRow}>
            <Show when={props.notif.type === "decision"}>
              <PriorityBadge
                priority={props.notif.priority as PriorityLevel}
                size="sm"
              />
            </Show>
            <span
              class={styles.notifTitle}
              classList={{ [styles.notifTitleUnread]: !props.notif.read }}
            >
              {props.notif.title}
            </span>
          </div>
          <Show when={props.notif.subtitle}>
            <div class={styles.notifSubtitle}>{props.notif.subtitle}</div>
          </Show>
          <div class={styles.notifMeta}>
            <Show when={props.notif.agentName}>
              <span class={styles.notifAgent}>{props.notif.agentName}</span>
            </Show>
            <span class={styles.notifTime}>{relativeTime(props.notif.timestamp)}</span>
          </div>
        </div>
      </div>
      <Show when={props.notif.actions && props.notif.actions.length > 0}>
        <div class={styles.notifActions}>
          <For each={props.notif.actions}>
            {(action) => <ActionButton action={action} notifId={props.notif.id} />}
          </For>
        </div>
      </Show>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const InboxSidebar: Component<InboxSidebarProps> = (props) => {
  const [isDragging, setIsDragging] = createSignal(false);

  // ---- Drag-to-resize logic -----------------------------------------------
  let sidebarRef: HTMLDivElement | undefined;

  const startResize = (e: MouseEvent) => {
    e.preventDefault();
    setIsDragging(true);
    const startX = e.clientX;
    const startWidth = settingsState.sidebarWidths.inboxSidebar;

    const onMove = (moveEvent: MouseEvent) => {
      const delta = startX - moveEvent.clientX;
      const clamped = Math.max(INBOX_SIDEBAR_MIN, Math.min(INBOX_SIDEBAR_MAX, startWidth + delta));
      if (sidebarRef) sidebarRef.style.width = `${clamped}px`;
    };

    const onUp = (upEvent: MouseEvent) => {
      setIsDragging(false);
      const delta = startX - upEvent.clientX;
      setInboxSidebarWidth(startWidth + delta);
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };

    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  const sidebarWidth = () =>
    props.collapsed ? 0 : settingsState.sidebarWidths.inboxSidebar;

  const counts = countForTab;
  const items = filteredNotifications;

  return (
    <div
      ref={sidebarRef}
      class={styles.sidebar}
      classList={{ [styles.collapsed]: props.collapsed, [styles.dragging]: isDragging() }}
      style={{ width: props.collapsed ? "0" : `${sidebarWidth()}px` }}
      aria-label="Inbox sidebar"
    >
      {/* Drag handle — left edge */}
      <Show when={!props.collapsed}>
        <div
          class={styles.resizeHandle}
          classList={{ [styles.dragging]: isDragging() }}
          onMouseDown={startResize}
          aria-hidden="true"
        />
      </Show>

      {/* Header */}
      <div class={styles.header}>
        <span class={styles.headerTitle}>INBOX</span>
        <Show when={unreadCount() > 0}>
          <span class={styles.countBadge}>{unreadCount()}</span>
        </Show>
        <button
          class={styles.markAllBtn}
          onClick={() => markAllRead()}
          title="Mark all as read"
        >
          <TbOutlineCheck size={12} />
          <span>Mark all read</span>
        </button>
      </div>

      {/* Filter tabs */}
      <div class={styles.filterTabs} role="tablist">
        <For each={FILTER_TABS}>
          {(tab) => (
            <button
              class={styles.filterTab}
              classList={{ [styles.filterTabActive]: activeFilter() === tab.key }}
              onClick={() => setActiveFilter(tab.key)}
              role="tab"
              aria-selected={activeFilter() === tab.key}
            >
              {tab.label}
              <span class={styles.tabCount}>{counts()[tab.key]}</span>
            </button>
          )}
        </For>
      </div>

      {/* Notification list */}
      <div class={styles.list} role="list">
        <For each={items()} fallback={
          <div class={styles.emptyState}>No notifications</div>
        }>
          {(notif) => <NotificationItem notif={notif} />}
        </For>
      </div>
    </div>
  );
};

export default InboxSidebar;
