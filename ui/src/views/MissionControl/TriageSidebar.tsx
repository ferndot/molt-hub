/**
 * TriageSidebar — collapsible right sidebar showing a flat priority-sorted
 * list of attention items with inline actions and cross-reference hover.
 */

import { For, Show, type Component } from "solid-js";
import type { MissionControlItem } from "./missionControlStore";
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
  const stopProp = (e: MouseEvent, fn: () => void) => {
    e.stopPropagation();
    fn();
  };

  return (
    <div
      class={`${styles.sidebar}${props.collapsed ? ` ${styles.collapsed}` : ""}`}
      aria-label="Triage sidebar"
    >
      <div class={styles.header}>
        <span>TRIAGE</span>
        <span class={styles.countBadge}>{props.items.length}</span>
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
    </div>
  );
};

export default TriageSidebar;
