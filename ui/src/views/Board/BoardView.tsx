/**
 * BoardView — the Kanban board. Horizontal column layout, one column per
 * pipeline stage. Exported as the default component for the /board route.
 */

import { For, type Component } from "solid-js";
import { boardState, moveTask, tasksForStage } from "./boardStore";
import BoardColumn from "./BoardColumn";
import styles from "./BoardView.module.css";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const BoardView: Component = () => {
  const handleDrop = (
    taskId: string,
    fromStage: string,
    toStage: string,
  ) => {
    moveTask(taskId, fromStage, toStage);
  };

  const handleApprove = (taskId: string) => {
    // Stub: in a real implementation this would dispatch an approval event
    console.log("[board] approve", taskId);
  };

  const handleReject = (taskId: string) => {
    // Stub: in a real implementation this would dispatch a rejection event
    console.log("[board] reject", taskId);
  };

  return (
    <div class={styles.boardWrapper}>
      <h2 class={styles.boardTitle}>Kanban Board</h2>
      <div class={styles.columnsContainer}>
        <For each={boardState.stages}>
          {(stage) => (
            <BoardColumn
              stage={stage}
              tasks={tasksForStage(stage)}
              onDrop={handleDrop}
              onApprove={handleApprove}
              onReject={handleReject}
            />
          )}
        </For>
      </div>
    </div>
  );
};

export default BoardView;
