/**
 * UnifiedCard — a board card that optionally shows triage attention info
 * with a glowing left-border accent and inline action buttons.
 */

import { Show, type Component } from "solid-js";
import { TbOutlineCheck, TbOutlineX, TbOutlineArrowRight, TbOutlineClock } from "solid-icons/tb";
import type { MissionControlItem } from "./missionControlStore";
import styles from "./UnifiedCard.module.css";

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface UnifiedCardProps {
  item: MissionControlItem;
  highlighted?: boolean;
  focused?: boolean;
  onToggle?: (taskId: string) => void;
  onHoverEnter?: (taskId: string) => void;
  onHoverLeave?: () => void;
  onApprove?: (taskId: string, triageId: string) => void;
  onReject?: (taskId: string, triageId: string, reason: string) => void;
  onRedirect?: (taskId: string, triageId: string, stage: string) => void;
  onDefer?: (triageId: string) => void;
  onAcknowledge?: (triageId: string) => void;
  onDragStart?: (e: DragEvent, taskId: string, fromStage: string) => void;
  onDragEnd?: (e: DragEvent) => void;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const priorityBadgeClass = (priority: string): string => {
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

const attentionBorderClass = (priority: string): string => {
  switch (priority) {
    case "p1":
      return styles.attentionP1;
    case "p2":
      return styles.attentionP2;
    case "p3":
      return styles.attentionP3;
    default:
      return "";
  }
};

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const UnifiedCard: Component<UnifiedCardProps> = (props) => {
  const cardClass = () => {
    const classes = [styles.card];
    if (props.item.attentionInfo) {
      classes.push(styles.attention);
      const borderCls = attentionBorderClass(props.item.priority);
      if (borderCls) classes.push(borderCls);
    }
    if (props.highlighted) classes.push(styles.highlighted);
    if (props.focused) classes.push(styles.focused);
    return classes.join(" ");
  };

  const handleDragStart = (e: DragEvent) => {
    props.onDragStart?.(e, props.item.id, props.item.stage);
  };

  const handleDragEnd = (e: DragEvent) => {
    props.onDragEnd?.(e);
  };

  const stopProp = (e: MouseEvent, fn: () => void) => {
    e.stopPropagation();
    fn();
  };

  return (
    <div
      class={cardClass()}
      draggable="true"
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onClick={() => props.onToggle?.(props.item.id)}
      onMouseEnter={() => props.onHoverEnter?.(props.item.id)}
      onMouseLeave={() => props.onHoverLeave?.()}
      data-task-id={props.item.id}
      role="listitem"
    >
      {/* Header */}
      <div class={styles.header}>
        <span class={styles.taskName}>{props.item.name}</span>
        <span
          class={`${styles.priorityBadge} ${priorityBadgeClass(props.item.priority)}`}
        >
          {props.item.priority}
        </span>
      </div>

      {/* Meta — stage chip omitted; column header already shows the stage */}
      <div class={styles.meta}>
        <span class={styles.agentName}>{props.item.agentName}</span>
      </div>

      {/* Attention actions — always visible when attention info present */}
      <Show when={props.item.attentionInfo}>
        <div class={styles.actions}>
          <Show when={props.item.attentionInfo!.triageType === "decision"}>
            <button
              class={`${styles.actionBtn} ${styles.approveBtn}`}
              onClick={(e) =>
                stopProp(e, () =>
                  props.onApprove?.(
                    props.item.id,
                    props.item.attentionInfo!.triageId,
                  ),
                )
              }
            >
              <TbOutlineCheck size={12} /> Approve
            </button>
            <button
              class={`${styles.actionBtn} ${styles.rejectBtn}`}
              onClick={(e) =>
                stopProp(e, () => {
                  const reason =
                    typeof window !== "undefined"
                      ? window.prompt("Reason for rejection (optional):") ?? ""
                      : "";
                  props.onReject?.(
                    props.item.id,
                    props.item.attentionInfo!.triageId,
                    reason,
                  );
                })
              }
            >
              <TbOutlineX size={12} /> Reject
            </button>
            <button
              class={`${styles.actionBtn} ${styles.redirectBtn}`}
              onClick={(e) =>
                stopProp(e, () => {
                  const to =
                    typeof window !== "undefined"
                      ? window.prompt(
                          "Pipeline stage id to redirect to:",
                          "",
                        )?.trim() ?? ""
                      : "";
                  if (to) {
                    props.onRedirect?.(
                      props.item.id,
                      props.item.attentionInfo!.triageId,
                      to,
                    );
                  }
                })
              }
            >
              <TbOutlineArrowRight size={12} /> Redirect
            </button>
            <button
              class={`${styles.actionBtn} ${styles.deferBtn}`}
              onClick={(e) =>
                stopProp(e, () =>
                  props.onDefer?.(props.item.attentionInfo!.triageId),
                )
              }
            >
              <TbOutlineClock size={12} /> Defer
            </button>
          </Show>
          <Show when={props.item.attentionInfo!.triageType === "info"}>
            <button
              class={`${styles.actionBtn} ${styles.ackBtn}`}
              onClick={(e) =>
                stopProp(e, () =>
                  props.onAcknowledge?.(props.item.attentionInfo!.triageId),
                )
              }
            >
              <TbOutlineCheck size={12} /> Acknowledge
            </button>
          </Show>
        </div>
      </Show>

      {/* Expanded detail */}
      <Show when={props.item.expanded && props.item.summary}>
        <div class={styles.detail}>{props.item.summary}</div>
        <div class={styles.timeInStage}>{props.item.timeInStage}</div>
      </Show>
    </div>
  );
};

export default UnifiedCard;
