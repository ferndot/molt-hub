/**
 * BoardColumn — one pipeline stage column with drop-zone support and a
 * vertically scrollable list of TaskCards.
 */

import { createSignal, For, Show, type Component } from "solid-js";
import type { BoardTask } from "./boardStore";
import TaskCard from "./TaskCard";
import styles from "./BoardColumn.module.css";

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface BoardColumnProps {
  stage: string;
  /** Display label for the column header. Falls back to stage id. */
  label?: string;
  /** Accent color (hex) for the column header indicator. */
  color?: string | null;
  tasks: BoardTask[];
  onDrop: (taskId: string, fromStage: string, toStage: string) => void;
  onApprove?: (taskId: string) => void;
  onReject?: (taskId: string) => void;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const BoardColumn: Component<BoardColumnProps> = (props) => {
  const [isDragOver, setIsDragOver] = createSignal(false);

  // -------------------------------------------------------------------------
  // Drag-and-drop handlers
  // -------------------------------------------------------------------------

  const handleDragStart = (
    e: DragEvent,
    taskId: string,
    fromStage: string,
  ) => {
    if (!e.dataTransfer) return;
    e.dataTransfer.setData("text/plain", JSON.stringify({ taskId, fromStage }));
    e.dataTransfer.effectAllowed = "move";
    // Make the dragged element semi-transparent via a class added to target
    const target = e.target as HTMLElement;
    target.style.opacity = "0.5";
  };

  const handleDragOver = (e: DragEvent) => {
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = "move";
    }
    setIsDragOver(true);
  };

  const handleDragLeave = (e: DragEvent) => {
    // Only clear if leaving the column entirely (not entering a child)
    const related = e.relatedTarget as HTMLElement | null;
    if (related && (e.currentTarget as HTMLElement).contains(related)) return;
    setIsDragOver(false);
  };

  const handleDrop = (e: DragEvent) => {
    e.preventDefault();
    setIsDragOver(false);
    if (!e.dataTransfer) return;
    const raw = e.dataTransfer.getData("text/plain");
    if (!raw) return;
    try {
      const { taskId, fromStage } = JSON.parse(raw) as {
        taskId: string;
        fromStage: string;
      };
      if (fromStage !== props.stage) {
        props.onDrop(taskId, fromStage, props.stage);
      }
    } catch {
      // ignore malformed data
    }
  };

  const handleDragEnd = (e: DragEvent) => {
    // Restore opacity on the element that was dragged
    const target = e.target as HTMLElement;
    target.style.opacity = "";
  };

  // -------------------------------------------------------------------------
  // Render
  // -------------------------------------------------------------------------

  return (
    <div
      class={`${styles.column}${isDragOver() ? ` ${styles.dragOver}` : ""}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      data-stage={props.stage}
      aria-label={`Stage: ${props.stage}`}
    >
      {/* Header */}
      <div
        class={styles.header}
        style={props.color ? { "border-bottom": `2px solid ${props.color}` } : undefined}
      >
        <span class={styles.stageName}>{props.label ?? props.stage.replace(/-/g, " ")}</span>
        <span class={styles.countBadge}>{props.tasks.length}</span>
      </div>

      {/* Card list */}
      <div class={styles.cardList}>
        <Show
          when={props.tasks.length > 0}
          fallback={<div class={styles.emptyHint}>Drop cards here</div>}
        >
          <For each={props.tasks}>
            {(task) => (
              <div onDragEnd={handleDragEnd}>
                <TaskCard
                  task={task}
                  onDragStart={handleDragStart}
                  onApprove={props.onApprove}
                  onReject={props.onReject}
                />
              </div>
            )}
          </For>
        </Show>
      </div>
    </div>
  );
};

export default BoardColumn;
