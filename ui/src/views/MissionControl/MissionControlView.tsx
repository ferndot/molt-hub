/**
 * MissionControlView — the main unified view combining the board layout
 * with a collapsible triage sidebar. Board columns show attention-annotated
 * cards; the sidebar provides a flat priority-sorted action list.
 */

import { Component, For, Show, createSignal } from "solid-js";
import { useMissionControl } from "./missionControlStore";
import MissionColumn from "./MissionColumn";
import TriageSidebar from "./TriageSidebar";
import JiraImport from "../Settings/JiraImport";
import styles from "./MissionControlView.module.css";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const MissionControlView: Component = () => {
  const mc = useMissionControl();
  const [_focusZone, _setFocusZone] = createSignal<"board" | "sidebar">(
    "board",
  );
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

      {/* Body: board + sidebar */}
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
              />
            )}
          </For>
        </div>
        <Show when={mc.sidebarCollapsed()}>
          <button
            class={styles.inboxExpandTab}
            onClick={mc.toggleSidebar}
            title="Open inbox"
          >
            « Inbox
          </button>
        </Show>
        <TriageSidebar
          items={mc.attentionItems()}
          collapsed={mc.sidebarCollapsed()}
          hoveredItemId={mc.hoveredItemId()}
          onToggle={mc.toggleSidebar}
          onHoverItem={mc.setHoveredItemId}
          onApprove={(triageId) => mc.approve(triageId)}
          onReject={(triageId) => mc.reject(triageId, "")}
          onRedirect={(triageId, stage) => mc.redirect(triageId, stage)}
          onDefer={(triageId) => mc.defer(triageId)}
          onAcknowledge={(triageId) => mc.acknowledge(triageId)}
          onJiraImport={() => setJiraImportOpen(true)}
        />
      </div>

      {/* Jira import dialog (rendered in a Portal) */}
      <JiraImport
        isOpen={jiraImportOpen()}
        onClose={() => setJiraImportOpen(false)}
      />
    </div>
  );
};

export default MissionControlView;
