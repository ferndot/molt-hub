/**
 * MissionColumn — one pipeline stage column with drag-drop support,
 * attention count badge, and filter-aware rendering.
 */

import { createSignal, For, Show, type Component } from "solid-js";
import type { MissionControlItem } from "./missionControlStore";
import UnifiedCard from "./UnifiedCard";
import styles from "./MissionColumn.module.css";

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface MissionColumnProps {
  stage: string;
  items: MissionControlItem[];
  attentionCount: number;
  filterActive: boolean;
  hiddenCount: number;
  hoveredItemId: string | null;
  focusedRow?: number;
  onHoverItem: (id: string | null) => void;
  onApprove: (triageId: string) => void;
  onReject: (triageId: string) => void;
  onRedirect: (triageId: string, stage: string) => void;
  onDefer: (triageId: string) => void;
  onAcknowledge: (triageId: string) => void;
  onDrop: (taskId: string, fromStage: string, toStage: string) => void;
  onToggle?: (taskId: string) => void;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const MissionColumn: Component<MissionColumnProps> = (props) => {
  const [isDragOver, setIsDragOver] = createSignal(false);

  const handleDragOver = (e: DragEvent) => {
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = "move";
    }
    setIsDragOver(true);
  };

  const handleDragLeave = (e: DragEvent) => {
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

  return (
    <div
      class={`${styles.column}${isDragOver() ? ` ${styles.dropZone}` : ""}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      data-stage={props.stage}
      aria-label={`Stage: ${props.stage}`}
    >
      {/* Header */}
      <div class={styles.columnHeader}>
        <span class={styles.stageName}>{props.stage.replace(/-/g, " ")}</span>
        <span class={styles.countBadge}>{props.items.length}</span>
        <Show when={props.attentionCount > 0}>
          <span class={styles.attentionBadge}>{props.attentionCount}</span>
        </Show>
      </div>

      {/* Card list */}
      <div class={styles.cardList} role="list">
        <Show
          when={props.items.length > 0}
          fallback={<div class={styles.emptyHint}>Drop cards here</div>}
        >
          <For each={props.items}>
            {(item, idx) => (
              <UnifiedCard
                item={item}
                highlighted={props.hoveredItemId === item.id}
                focused={props.focusedRow === idx()}
                onToggle={props.onToggle}
                onHoverEnter={(id) => props.onHoverItem(id)}
                onHoverLeave={() => props.onHoverItem(null)}
                onApprove={props.onApprove}
                onReject={props.onReject}
                onRedirect={props.onRedirect}
                onDefer={props.onDefer}
                onAcknowledge={props.onAcknowledge}
              />
            )}
          </For>
        </Show>
      </div>

      {/* Hidden count when filter is active */}
      <Show when={props.filterActive && props.hiddenCount > 0}>
        <div class={styles.hiddenCount}>
          and {props.hiddenCount} more&hellip;
        </div>
      </Show>
    </div>
  );
};

export default MissionColumn;
