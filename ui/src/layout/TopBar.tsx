/**
 * TopBar — thin 36px bar at the top of the main content area.
 * Houses the inbox toggle + unread badge on the right; left side is
 * reserved for future breadcrumbs. Provides a macOS drag region.
 */

import { Show, type Component } from "solid-js";
import {
  TbOutlineLayoutSidebarLeftCollapse,
  TbOutlineLayoutSidebarLeftExpand,
  TbOutlineBell,
  TbOutlineBellOff,
} from "solid-icons/tb";
import styles from "./TopBar.module.css";
import { projectState } from "../stores/projectStore";

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface TopBarProps {
  sidebarCollapsed: boolean;
  onToggleSidebar: () => void;
  inboxOpen: boolean;
  onToggleInbox: () => void;
  inboxCount: number;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const TopBar: Component<TopBarProps> = (props) => {
  return (
    <div class={styles.topBar} data-testid="topbar">
      {/* Left: sidebar toggle */}
      <div class={styles.left}>
        <button
          class={styles.sidebarToggle}
          onClick={props.onToggleSidebar}
          title={props.sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
          aria-label={props.sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
        >
          {props.sidebarCollapsed
            ? <TbOutlineLayoutSidebarLeftExpand size={16} />
            : <TbOutlineLayoutSidebarLeftCollapse size={16} />
          }
        </button>
      </div>

      {/* Center: active project name (only shown when multiple projects exist) */}
      <Show when={projectState.projects.length > 1}>
        <span class={styles.projectBreadcrumb}>
          {projectState.projects.find(
            (p) => p.id === projectState.activeProjectId,
          )?.name ?? ""}
        </span>
      </Show>

      {/* Right: inbox toggle */}
      <div class={styles.right}>
        <button
          class={`${styles.inboxToggle}${props.inboxOpen ? ` ${styles.inboxToggleActive}` : ""}`}
          onClick={props.onToggleInbox}
          title={props.inboxOpen ? "Close inbox" : "Open inbox"}
          aria-label={props.inboxOpen ? "Close inbox" : "Open inbox"}
        >
          {props.inboxOpen
            ? <TbOutlineBellOff size={16} />
            : <TbOutlineBell size={16} />
          }
          <Show when={props.inboxCount > 0}>
            <span class={styles.inboxBadge} data-testid="inbox-badge">
              {props.inboxCount}
            </span>
          </Show>
        </button>
      </div>
    </div>
  );
};

export default TopBar;
