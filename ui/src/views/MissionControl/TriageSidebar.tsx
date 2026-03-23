/**
 * TriageSidebar — collapsible right sidebar showing a flat priority-sorted
 * list of attention items with inline actions and cross-reference hover.
 * Resizable by dragging its left edge.
 */

import { For, Show, createSignal, type Component } from "solid-js";
import type { MissionControlItem } from "./missionControlStore";
import {
  settingsState,
  setTriageSidebarWidth,
  TRIAGE_SIDEBAR_MIN,
  TRIAGE_SIDEBAR_MAX,
} from "../Settings/settingsStore";
import styles from "./TriageSidebar.module.css";

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface TriageSidebarProps {
  items: MissionControlItem[];
  collapsed: boolean;
  hoveredItemId: string | null;
  focusedIndex?: number;
  onHoverItem: (id: string | null) => void;
  onApprove: (triageId: string) => void;
  onReject: (triageId: string) => void;
  onRedirect: (triageId: string, stage: string) => void;
  onDefer: (triageId: string) => void;
  onAcknowledge: (triageId: string) => void;
  onToggle?: () => void;
  onJiraImport?: () => void;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const priorityClass = (priority: string): string => {
  switch (priority) {
    case "p0":
      return styles.priorityP0;
    case "p1":
      return styles.priorityP1;
    case "p2":
      return styles.priorityP2;
    case "p3":
      return styles.priorityP3;
    default:
      return styles.priorityP3;
  }
};

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const TriageSidebar: Component<TriageSidebarProps> = (props) => {
  const [isDragging, setIsDragging] = createSignal(false);

  const stopProp = (e: MouseEvent, fn: () => void) => {
    e.stopPropagation();
    fn();
  };

  // ---- Drag-to-resize logic -----------------------------------------------
  // Update the DOM directly during drag to avoid the store → effect →
  // localStorage chain on every mousemove. Commit to the store on mouseup.

  let sidebarRef: HTMLDivElement | undefined;

  const startResize = (e: MouseEvent) => {
    e.preventDefault();
    setIsDragging(true);
    const startX = e.clientX;
    const startWidth = settingsState.sidebarWidths.triageSidebar;

    const onMove = (moveEvent: MouseEvent) => {
      // Dragging left edge: moving left increases width
      const delta = startX - moveEvent.clientX;
      const clamped = Math.max(TRIAGE_SIDEBAR_MIN, Math.min(TRIAGE_SIDEBAR_MAX, startWidth + delta));
      if (sidebarRef) sidebarRef.style.width = `${clamped}px`;
    };

    const onUp = (upEvent: MouseEvent) => {
      setIsDragging(false);
      const delta = startX - upEvent.clientX;
      setTriageSidebarWidth(startWidth + delta);
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };

    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  const sidebarWidth = () =>
    props.collapsed ? 0 : settingsState.sidebarWidths.triageSidebar;

  return (
    <div
      ref={sidebarRef}
      class={styles.sidebar}
      classList={{ [styles.collapsed]: props.collapsed, [styles.dragging]: isDragging() }}
      style={{ width: props.collapsed ? "0" : `${sidebarWidth()}px` }}
      aria-label="Triage sidebar"
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

      <div class={styles.header}>
        <span class={styles.headerTitle}>INBOX</span>
        <span class={styles.countBadge}>{props.items.length}</span>
        <Show when={props.onToggle}>
          <button
            class={styles.collapseBtn}
            onClick={props.onToggle}
            title="Collapse inbox"
          >
            »
          </button>
        </Show>
      </div>

      <div class={styles.list} role="list">
        <For each={props.items}>
          {(item, idx) => (
            <div
              class={`${styles.sidebarItem}${props.hoveredItemId === item.id ? ` ${styles.itemHighlighted}` : ""}${props.focusedIndex === idx() ? ` ${styles.itemFocused}` : ""}`}
              onMouseEnter={() => props.onHoverItem(item.id)}
              onMouseLeave={() => props.onHoverItem(null)}
              data-task-id={item.id}
              role="listitem"
            >
              <div class={styles.itemHeader}>
                <span
                  class={`${styles.priorityBadge} ${priorityClass(item.priority)}`}
                >
                  {item.priority}
                </span>
                <span class={styles.itemName}>{item.name}</span>
              </div>
              <div class={styles.itemMeta}>
                <span class={styles.itemAgent}>{item.agentName}</span>
                <span class={styles.itemStage}>{item.stage}</span>
              </div>
              <Show when={item.attentionInfo}>
                <div class={styles.itemActions}>
                  <Show when={item.attentionInfo!.triageType === "decision"}>
                    <button
                      class={`${styles.actionBtn} ${styles.approveBtn}`}
                      onClick={(e) =>
                        stopProp(e, () =>
                          props.onApprove(item.attentionInfo!.triageId),
                        )
                      }
                    >
                      &#10003;
                    </button>
                    <button
                      class={`${styles.actionBtn} ${styles.rejectBtn}`}
                      onClick={(e) =>
                        stopProp(e, () =>
                          props.onReject(item.attentionInfo!.triageId),
                        )
                      }
                    >
                      &#10005;
                    </button>
                    <button
                      class={`${styles.actionBtn} ${styles.redirectBtn}`}
                      onClick={(e) =>
                        stopProp(e, () =>
                          props.onRedirect(
                            item.attentionInfo!.triageId,
                            item.stage,
                          ),
                        )
                      }
                    >
                      &rarr;
                    </button>
                    <button
                      class={`${styles.actionBtn} ${styles.deferBtn}`}
                      onClick={(e) =>
                        stopProp(e, () =>
                          props.onDefer(item.attentionInfo!.triageId),
                        )
                      }
                    >
                      &#9207;
                    </button>
                  </Show>
                  <Show when={item.attentionInfo!.triageType === "info"}>
                    <button
                      class={`${styles.actionBtn} ${styles.ackBtn}`}
                      onClick={(e) =>
                        stopProp(e, () =>
                          props.onAcknowledge(item.attentionInfo!.triageId),
                        )
                      }
                    >
                      &#10003;
                    </button>
                  </Show>
                </div>
              </Show>
            </div>
          )}
        </For>
      </div>

      {/* Footer: Jira import link */}
      <Show when={props.onJiraImport}>
        <div class={styles.footer}>
          <button
            class={styles.jiraImportBtn}
            onClick={props.onJiraImport}
            title="Import issues from Jira"
          >
            ↓ Import from Jira
          </button>
        </div>
      </Show>
    </div>
  );
};

export default TriageSidebar;
