/**
 * Mission Control store — merges board tasks with triage attention info
 * into a unified view. Provides filtering, sorting, and cross-reference
 * hover state for the board + sidebar layout.
 */

import { createSignal, createMemo } from "solid-js";
import {
  boardState,
  moveTask,
  toggleCard,
  sortByPriority,
} from "../Board/boardStore";
import {
  useTriageStore,
  approve as triageApprove,
  reject as triageReject,
  redirect as triageRedirect,
  defer,
  acknowledge,
} from "../Triage/triageStore";
import type { BoardTask } from "../Board/boardStore";
import type { TriageItem } from "../Triage/triageStore";
import type { Priority } from "../../types/domain";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface AttentionInfo {
  triageId: string;
  triageType: "decision" | "info";
  createdAt: string;
}

export interface MissionControlItem extends BoardTask {
  attentionInfo?: AttentionInfo;
}

// ---------------------------------------------------------------------------
// UI state signals
// ---------------------------------------------------------------------------

const [globalFilterActive, setGlobalFilterActive] = createSignal(false);
const [hoveredItemId, setHoveredItemId] = createSignal<string | null>(null);
const [sidebarCollapsed, setSidebarCollapsed] = createSignal(false);

// ---------------------------------------------------------------------------
// Merge logic
// ---------------------------------------------------------------------------

function mergeItems(): MissionControlItem[] {
  const allowedStages = new Set(boardState.stages);
  const activeBoardId = boardState.activeBoardId;
  const { state: triageState } = useTriageStore();
  const items: MissionControlItem[] = boardState.tasks
    .filter((task) => allowedStages.has(task.stage))
    .filter((task) => !task.boardId || task.boardId === activeBoardId)
    .map((task) => {
    const triageMatch = triageState.items.find((ti) => ti.taskId === task.id);
    if (triageMatch) {
      return {
        ...task,
        attentionInfo: {
          triageId: triageMatch.id,
          triageType: triageMatch.type,
          createdAt: triageMatch.createdAt,
        },
      };
    }
    return { ...task };
  });
  return items;
}

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

export function useMissionControl() {
  const items = createMemo(() => mergeItems());

  const stages = () => boardState.stages;

  const itemsForStage = (stage: string) => {
    const stageItems = items().filter((item) => item.stage === stage);
    // Attention items first, then sort by priority within each group
    const attention = stageItems.filter((i) => i.attentionInfo);
    const rest = stageItems.filter((i) => !i.attentionInfo);
    return [...sortByPriority(attention), ...sortByPriority(rest)];
  };

  const visibleItemsForStage = (stage: string) => {
    const all = itemsForStage(stage);
    if (globalFilterActive()) {
      return all.filter((i) => i.attentionInfo);
    }
    return all;
  };

  const hiddenCountForStage = (stage: string) => {
    if (!globalFilterActive()) return 0;
    const all = itemsForStage(stage);
    return all.filter((i) => !i.attentionInfo).length;
  };

  const attentionCountForStage = (stage: string) => {
    return itemsForStage(stage).filter((i) => i.attentionInfo).length;
  };

  // Flat priority-sorted attention items for the sidebar
  const attentionItems = createMemo(() => {
    const all = items().filter((i) => i.attentionInfo);
    return sortByPriority(all);
  });

  const totalAttentionCount = createMemo(() => attentionItems().length);

  async function approveAttention(taskId: string, triageId: string): Promise<void> {
    try {
      await triageApprove(triageId, taskId);
    } catch (e) {
      console.error(e);
    }
  }

  async function rejectAttention(
    taskId: string,
    triageId: string,
    reason: string,
  ): Promise<void> {
    try {
      await triageReject(triageId, taskId, reason);
    } catch (e) {
      console.error(e);
    }
  }

  async function redirectAttention(
    taskId: string,
    triageId: string,
    stage: string,
  ): Promise<void> {
    try {
      await triageRedirect(triageId, taskId, stage);
    } catch (e) {
      console.error(e);
    }
  }

  return {
    stages,
    items,
    itemsForStage,
    visibleItemsForStage,
    hiddenCountForStage,
    attentionCountForStage,
    attentionItems,
    totalAttentionCount,
    globalFilterActive,
    toggleGlobalFilter: () => setGlobalFilterActive((v) => !v),
    hoveredItemId,
    setHoveredItemId,
    sidebarCollapsed,
    toggleSidebar: () => setSidebarCollapsed((v) => !v),
    approveAttention,
    rejectAttention,
    redirectAttention,
    defer,
    acknowledge,
    moveTask,
    toggleCard,
  };
}
