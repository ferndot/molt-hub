/**
 * MissionControlView — the main board view with attention-annotated cards.
 * The inbox sidebar is now in AppLayout (available across all views).
 */

import { Component, For, createSignal } from "solid-js";
import { useMissionControl } from "./missionControlStore";
import MissionColumn from "./MissionColumn";
import JiraImport from "../Settings/JiraImport";
import { settingsState } from "../Settings/settingsStore";
import styles from "./MissionControlView.module.css";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const MissionControlView: Component = () => {
  const mc = useMissionControl();
  const [jiraImportOpen, setJiraImportOpen] = createSignal(false);

  return (
    <div class={styles.container}>
      {/* Header bar */}
      <div class={styles.header}>
        <h2 class={styles.title}>Mission Control</h2>
        <span class={styles.attentionBadge}>
          {mc.totalAttentionCount() === 0
            ? "All clear"
            : `${mc.totalAttentionCount()} need attention`}
        </span>
        <button
          class={styles.filterToggle}
          classList={{ [styles.filterToggleActive]: mc.globalFilterActive() }}
          onClick={mc.toggleGlobalFilter}
        >
          {mc.globalFilterActive() ? "Show All" : "Focus"}
        </button>
      </div>

      {/* Board columns */}
      <div class={styles.body}>
        <div class={styles.boardRegion}>
          <For each={mc.stages()}>
            {(stage) => (
              <MissionColumn
                stage={stage}
                items={mc.visibleItemsForStage(stage)}
                attentionCount={mc.attentionCountForStage(stage)}
                filterActive={mc.globalFilterActive()}
                hiddenCount={mc.hiddenCountForStage(stage)}
                hoveredItemId={mc.hoveredItemId()}
                onHoverItem={mc.setHoveredItemId}
                onApprove={(triageId) => mc.approve(triageId)}
                onReject={(triageId) => mc.reject(triageId, "")}
                onRedirect={(triageId, stage) => mc.redirect(triageId, stage)}
                onDefer={(triageId) => mc.defer(triageId)}
                onAcknowledge={(triageId) => mc.acknowledge(triageId)}
                onDrop={(taskId, from, to) => mc.moveTask(taskId, from, to)}
                onToggle={(taskId) => mc.toggleCard(taskId)}
                footer={stage === "backlog" && settingsState.jiraConfig.connected
                  ? () => (
                    <button class={styles.importBtn} onClick={() => setJiraImportOpen(true)}>
                      ↓ Import from Jira
                    </button>
                  )
                  : undefined}
              />
            )}
          </For>
        </div>
      </div>

      <JiraImport
        isOpen={jiraImportOpen()}
        onClose={() => setJiraImportOpen(false)}
      />
    </div>
  );
};

export default MissionControlView;
