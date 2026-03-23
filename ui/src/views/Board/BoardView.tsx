/**
 * BoardView — the Kanban board. Horizontal column layout, one column per
 * pipeline stage. Exported as the default component for the /board route.
 */

import { For, Show, createSignal, onMount, type Component } from "solid-js";
import { TbOutlineSettings } from "solid-icons/tb";
import { moveTask, tasksForStage, initBoardStages } from "./boardStore";
import BoardColumn from "./BoardColumn";
import ColumnEditor from "./ColumnEditor";
import { settingsState, getSortedColumns, getStagesForColumn } from "../Settings/settingsStore";
import styles from "./BoardView.module.css";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const BoardView: Component = () => {
  const [editorOpen, setEditorOpen] = createSignal(false);

  // Fetch pipeline stages from server on mount (falls back to defaults)
  onMount(() => {
    initBoardStages();
  });

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

  // Derive columns from settingsStore; fall back to tasks for each stage name
  const sortedColumns = () => getSortedColumns(settingsState.kanbanColumns);

  return (
    <div class={styles.boardWrapper}>
      <div class={styles.boardHeader}>
        <h2 class={styles.boardTitle}>Kanban Board</h2>
        <button
          class={`${styles.settingsBtn}${editorOpen() ? ` ${styles.settingsBtnActive}` : ""}`}
          onClick={() => setEditorOpen((v) => !v)}
          title="Configure columns"
          aria-label="Configure board columns"
          aria-expanded={editorOpen()}
        >
          <TbOutlineSettings size={16} />
        </button>
      </div>

      <Show when={editorOpen()}>
        <ColumnEditor onClose={() => setEditorOpen(false)} />
      </Show>

      <div class={styles.columnsContainer}>
        <For each={sortedColumns()}>
          {(col) => {
            // A column may span multiple stage names (comma-separated)
            const stages = getStagesForColumn(col);
            const tasks = () =>
              stages.flatMap((s) => tasksForStage(s));
            // Use the first matched stage as the drop target stage
            const primaryStage = stages[0] ?? col.id;
            return (
              <BoardColumn
                stage={primaryStage}
                tasks={tasks()}
                onDrop={handleDrop}
                onApprove={handleApprove}
                onReject={handleReject}
              />
            );
          }}
        </For>
      </div>
    </div>
  );
};

export default BoardView;
