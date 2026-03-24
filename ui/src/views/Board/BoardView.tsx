/**
 * BoardView — the Kanban board. Horizontal column layout, one column per
 * pipeline stage. Exported as the default component for the /board route.
 */

import { For, Show, createSignal, onMount, type Component } from "solid-js";
import { TbOutlineSettings } from "solid-icons/tb";
import { moveTask, tasksForStage, initBoardStages, getSortedStages } from "./boardStore";
import BoardColumn from "./BoardColumn";
import ColumnEditor from "./ColumnEditor";
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

  // Derive columns from boardStore pipeline stages (server-driven)
  const sortedStages = () => getSortedStages();

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
        <For each={sortedStages()}>
          {(stage) => (
            <BoardColumn
              stage={stage.id}
              label={stage.label}
              color={stage.color}
              tasks={tasksForStage(stage.id)}
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
