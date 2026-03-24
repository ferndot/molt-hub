/**
 * BoardView — the Kanban board. Horizontal column layout, one column per
 * pipeline stage. Exported as the default component for the /board route.
 */

import { For, Show, createSignal, type Component } from "solid-js";
import { TbOutlineSettings } from "solid-icons/tb";
import { moveTask, tasksForStage, getSortedStages, type BoardTask } from "./boardStore";
import { activeRepoPath } from "../../stores/projectStore";
import BoardColumn from "./BoardColumn";
import ColumnEditor from "./ColumnEditor";
import styles from "./BoardView.module.css";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const BoardView: Component = () => {
  const [editorOpen, setEditorOpen] = createSignal(false);

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

  const handleRunAgent = async (task: BoardTask) => {
    const repo = activeRepoPath();
    if (!repo) {
      window.alert(
        "Choose a project with a repository path in Settings before running an agent.",
      );
      return;
    }
    const instructions = [
      `Board task ${task.id} (stage: ${task.stage}): ${task.name}`,
      task.summary ? task.summary : "",
    ]
      .filter(Boolean)
      .join("\n\n");
    try {
      const res = await fetch("/api/agents/spawn", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          instructions,
          workingDir: repo,
          adapterType: "claude",
        }),
      });
      const data = (await res.json().catch(() => ({}))) as {
        message?: string;
        agentId?: string;
      };
      if (!res.ok) {
        window.alert(
          data.message ?? `Spawn failed (HTTP ${res.status})`,
        );
        return;
      }
      if (data.agentId) {
        window.alert(`Agent started: ${data.agentId}`);
      }
    } catch (e) {
      window.alert(e instanceof Error ? e.message : "Network error");
    }
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
              onRunAgent={handleRunAgent}
            />
          )}
        </For>
      </div>
    </div>
  );
};

export default BoardView;
