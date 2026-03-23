/**
 * MissionControlView — the main unified view combining the board layout
 * with a collapsible triage sidebar. Board columns show attention-annotated
 * cards; the sidebar provides a flat priority-sorted action list.
 */

import { Component, For, createSignal } from "solid-js";
import { useMissionControl } from "./missionControlStore";
import MissionColumn from "./MissionColumn";
import TriageSidebar from "./TriageSidebar";
import styles from "./MissionControlView.module.css";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const MissionControlView: Component = () => {
  const mc = useMissionControl();
  const [_focusZone, _setFocusZone] = createSignal<"board" | "sidebar">(
    "board",
  );

  return (
    <div class={styles.container}>
      {/* Header bar */}
      <div class={styles.header}>
        <h2 class={styles.title}>Mission Control</h2>
        <span class={styles.attentionBadge}>
          {mc.totalAttentionCount()} need attention
        </span>
        <button class={styles.filterToggle} onClick={mc.toggleGlobalFilter}>
          {mc.globalFilterActive() ? "Show All" : "Attention Only"}
        </button>
        <button
          class={styles.sidebarToggle}
          onClick={mc.toggleSidebar}
          title="Toggle triage sidebar"
        >
          {mc.sidebarCollapsed() ? "\u25C0" : "\u25B6"}
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
        <TriageSidebar
          items={mc.attentionItems()}
          collapsed={mc.sidebarCollapsed()}
          hoveredItemId={mc.hoveredItemId()}
          onHoverItem={mc.setHoveredItemId}
          onApprove={(triageId) => mc.approve(triageId)}
          onReject={(triageId) => mc.reject(triageId, "")}
          onRedirect={(triageId, stage) => mc.redirect(triageId, stage)}
          onDefer={(triageId) => mc.defer(triageId)}
          onAcknowledge={(triageId) => mc.acknowledge(triageId)}
        />
      </div>
    </div>
  );
};

export default MissionControlView;
