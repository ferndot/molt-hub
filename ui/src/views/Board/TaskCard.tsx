/**
 * TaskCard — individual task card with collapsed/expanded/focused states,
 * priority badge, status dot, and HTML5 drag-and-drop support.
 */

import { Show, type Component } from "solid-js";
import type { Priority } from "../../types/domain";
import type { BoardTask, BoardTaskStatus } from "./boardStore";
import { toggleCard } from "./boardStore";
import styles from "./TaskCard.module.css";

// ---------------------------------------------------------------------------
// Color maps
// ---------------------------------------------------------------------------

export const PRIORITY_COLORS: Record<Priority, string> = {
  p0: "#e63946",
  p1: "#f4a261",
  p2: "#2a9d8f",
  p3: "#6c757d",
};

export const STATUS_COLORS: Record<BoardTaskStatus, string> = {
  running: "#2a9d8f",
  waiting: "#f4a261",
  blocked: "#e63946",
  complete: "#6c757d",
};

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
  const handleDragStart = (e: DragEvent) => {
    props.onDragStart?.(e, props.task.id, props.task.stage);
  };

  const handleClick = (e: MouseEvent) => {
    // Don't toggle if clicking action buttons
    const target = e.target as HTMLElement;
    if (target.closest("button")) return;
    toggleCard(props.task.id);
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
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
      {/* Header row: task name + priority badge + status dot */}
      <div class={styles.header}>
        <span class={styles.taskName}>{props.task.name}</span>
        <span
          class={styles.priorityBadge}
          style={{ background: PRIORITY_COLORS[props.task.priority] }}
        >
          {props.task.priority.toUpperCase()}
        </span>
        <span
          class={styles.statusDot}
          style={{ background: STATUS_COLORS[props.task.status] }}
          title={props.task.status}
          aria-label={`Status: ${props.task.status}`}
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
