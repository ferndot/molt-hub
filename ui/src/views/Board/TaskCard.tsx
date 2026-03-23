/**
 * TaskCard — individual task card with collapsed/expanded/focused states,
 * priority badge, status indicator, and HTML5 drag-and-drop support.
 */

import { Show, type Component } from "solid-js";
import { useNavigate } from "@solidjs/router";
import type { Priority } from "../../types/domain";
import type { BoardTask, BoardTaskStatus } from "./boardStore";
import { toggleCard } from "./boardStore";
import { PriorityBadge } from "../../components/PriorityBadge";
import { StatusIndicator } from "../../components/StatusIndicator";
import type { IndicatorStatus } from "../../components/StatusIndicator";
import type { PriorityLevel } from "../../components/PriorityBadge";
import styles from "./TaskCard.module.css";

// ---------------------------------------------------------------------------
// Status mapping — BoardTaskStatus → IndicatorStatus
// ---------------------------------------------------------------------------

function toIndicatorStatus(s: BoardTaskStatus): IndicatorStatus {
  switch (s) {
    case "running":
      return "running";
    case "waiting":
      return "paused";
    case "blocked":
      return "blocked";
    case "complete":
      return "completed";
  }
}

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface TaskCardProps {
  task: BoardTask;
  onDragStart?: (e: DragEvent, taskId: string, fromStage: string) => void;
  onApprove?: (taskId: string) => void;
  onReject?: (taskId: string) => void;
  focused?: boolean;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const TaskCard: Component<TaskCardProps> = (props) => {
  const navigate = useNavigate();

  const handleDragStart = (e: DragEvent) => {
    props.onDragStart?.(e, props.task.id, props.task.stage);
  };

  const handleClick = (e: MouseEvent) => {
    // Don't navigate/toggle if clicking action buttons
    const target = e.target as HTMLElement;
    if (target.closest("button")) return;
    // Double-click navigates to detail view
    if (e.detail === 2) {
      navigate(`/tasks/${props.task.id}`);
      return;
    }
    toggleCard(props.task.id);
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      navigate(`/tasks/${props.task.id}`);
      return;
    }
    if (e.key === " ") {
      e.preventDefault();
      toggleCard(props.task.id);
    }
  };

  const cardClass = () => {
    const classes = [styles.card];
    if (props.task.expanded) classes.push(styles.expanded);
    if (props.focused) classes.push(styles.focused);
    return classes.join(" ");
  };

  return (
    <div
      class={cardClass()}
      draggable="true"
      onDragStart={handleDragStart}
      onClick={handleClick}
      onKeyDown={handleKeyDown}
      tabIndex={0}
      role="button"
      aria-expanded={props.task.expanded}
      aria-label={`Task: ${props.task.name}`}
      data-task-id={props.task.id}
      data-task-stage={props.task.stage}
    >
      {/* Header row: task name + priority badge + status indicator */}
      <div class={styles.header}>
        <span class={styles.taskName}>{props.task.name}</span>
        <PriorityBadge
          priority={props.task.priority as PriorityLevel}
          size="sm"
        />
        <StatusIndicator
          status={toIndicatorStatus(props.task.status)}
          size="sm"
        />
      </div>

      {/* Meta row: agent name */}
      <div class={styles.meta}>
        <span class={styles.agentName}>{props.task.agentName}</span>
      </div>

      {/* Expanded content */}
      <Show when={props.task.expanded}>
        <div class={styles.expandedContent}>
          <Show when={props.task.summary}>
            <p class={styles.summary}>{props.task.summary}</p>
          </Show>
          <p class={styles.timeLabel}>
            Time in stage: {props.task.timeInStage}
          </p>
          <Show
            when={
              props.task.status === "waiting" ||
              props.task.status === "blocked"
            }
          >
            <div class={styles.actions}>
              <button
                class={`${styles.actionBtn} ${styles.approveBtn}`}
                onClick={(e) => {
                  e.stopPropagation();
                  props.onApprove?.(props.task.id);
                }}
              >
                Approve
              </button>
              <button
                class={`${styles.actionBtn} ${styles.rejectBtn}`}
                onClick={(e) => {
                  e.stopPropagation();
                  props.onReject?.(props.task.id);
                }}
              >
                Reject
              </button>
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
};

export default TaskCard;
