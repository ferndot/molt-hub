/**
 * TriageView — primary active surface of Molt Hub.
 *
 * Displays a priority-sorted list of items requiring human attention,
 * split into two visual tiers: "Needs Action" (P0+P1) and "Informational"
 * (P2+P3). Uses TanStack Virtual for the list.
 */

import type { Component } from "solid-js";
import { createMemo, createSignal, For, onMount, Show, onCleanup } from "solid-js";
import { createVirtualizer } from "../../lib/virtual";
import {
  useTriageStore,
  setFilterMode,
  setSortMode,
  setupTriageSubscription,
  getFilteredItems,
  initTriage,
} from "./triageStore";
import type { FilterMode, SortMode, TriageItem } from "./triageStore";
import TriageItemCard from "./TriageItem";
import ApprovalCard from "./ApprovalCard";
import type { ApprovalRequest } from "./ApprovalCard";
import styles from "./TriageView.module.css";

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

interface ToggleProps {
  label: string;
  active: boolean;
  onClick: () => void;
}

const ToggleButton: Component<ToggleProps> = (props) => (
  <button
    class={`${styles.toggleBtn} ${props.active ? styles.toggleBtnActive : ""}`}
    onClick={props.onClick}
    type="button"
  >
    {props.label}
  </button>
);

// ---------------------------------------------------------------------------
// Virtualised section — renders items via TanStack Virtual
// ---------------------------------------------------------------------------

interface VirtualSectionProps {
  items: TriageItem[];
}

const VirtualSection: Component<VirtualSectionProps> = (props) => {
  let containerRef: HTMLDivElement | undefined;

  const virtualizer = createVirtualizer({
    get count() {
      return props.items.length;
    },
    getScrollElement: () => containerRef ?? null,
    estimateSize: () => 80,
    overscan: 3,
  });

  return (
    <div class={styles.listContainer} ref={containerRef}>
      <div
        class={styles.listInner}
        style={{ height: `${virtualizer.getTotalSize()}px` }}
      >
        <For each={virtualizer.getVirtualItems()}>
          {(virtualRow) => {
            const item = props.items[virtualRow.index];
            return (
              <div
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  transform: `translateY(${virtualRow.start}px)`,
                }}
                data-index={virtualRow.index}
                ref={(el) => virtualizer.measureElement(el)}
              >
                <TriageItemCard item={item} />
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Main view
// ---------------------------------------------------------------------------

const TriageView: Component = () => {
  // Wire up WebSocket subscription for real-time updates
  const unsub = setupTriageSubscription();
  onCleanup(unsub);

  // Hydrate from HTTP on mount
  onMount(() => { void initTriage(); });

  const { state } = useTriageStore();

  // Approval requests — populated by WebSocket or API
  const [approvalRequests, setApprovalRequests] = createSignal<ApprovalRequest[]>([]);

  function handleApprovalResolved(id: string): void {
    setApprovalRequests((prev) => prev.filter((r) => r.id !== id));
  }

  /** Add an approval request (callable from WebSocket handler). */
  function addApprovalRequest(req: ApprovalRequest): void {
    setApprovalRequests((prev) => {
      if (prev.find((r) => r.id === req.id)) return prev;
      return [req, ...prev];
    });
  }

  // Expose addApprovalRequest for external wiring
  (window as unknown as Record<string, unknown>).__addApprovalRequest = addApprovalRequest;

  const filteredItems = createMemo(() =>
    getFilteredItems(state.items, state.filterMode, state.sortMode),
  );

  const needsActionItems = createMemo(() =>
    filteredItems().filter(
      (item) => item.priority === "p0" || item.priority === "p1",
    ),
  );

  const informationalItems = createMemo(() =>
    filteredItems().filter(
      (item) => item.priority === "p2" || item.priority === "p3",
    ),
  );

  // Raw attention count from store (unfiltered)
  const attentionCount = createMemo(
    () =>
      state.items.filter((i) => i.priority === "p0" || i.priority === "p1")
        .length,
  );

  const allItems = createMemo(() => filteredItems());

  // For the virtual list we flatten everything with a "divider" marker
  // We render the two tiers separately to keep the divider visible
  const showInformational = createMemo(
    () =>
      state.filterMode !== "needs-action" && informationalItems().length > 0,
  );

  return (
    <div class={styles.container}>
      {/* Header */}
      <div class={styles.header}>
        <div class={styles.titleRow}>
          <h2 class={styles.title}>Triage Queue</h2>
          <span class={styles.attentionCount}>
            <Show when={attentionCount() > 0} fallback="All clear">
              <span class={styles.attentionCountHighlight}>
                {attentionCount()}
              </span>{" "}
              {attentionCount() === 1 ? "item needs" : "items need"} attention
            </Show>
          </span>
        </div>

        <div class={styles.controls}>
          {/* Filter toggles */}
          <div class={styles.filterGroup}>
            <span class={styles.controlLabel}>Filter:</span>
            <ToggleButton
              label="Show All"
              active={state.filterMode === "all"}
              onClick={() => setFilterMode("all" as FilterMode)}
            />
            <ToggleButton
              label="Needs Action"
              active={state.filterMode === "needs-action"}
              onClick={() => setFilterMode("needs-action" as FilterMode)}
            />
            <ToggleButton
              label="By Agent"
              active={state.filterMode === "by-agent"}
              onClick={() => setFilterMode("by-agent" as FilterMode)}
            />
          </div>

          {/* Sort toggles */}
          <div class={styles.sortGroup}>
            <span class={styles.controlLabel}>Sort:</span>
            <ToggleButton
              label="Priority"
              active={state.sortMode === "priority"}
              onClick={() => setSortMode("priority" as SortMode)}
            />
            <ToggleButton
              label="Time Waiting"
              active={state.sortMode === "time-waiting"}
              onClick={() => setSortMode("time-waiting" as SortMode)}
            />
            <ToggleButton
              label="Agent"
              active={state.sortMode === "by-agent"}
              onClick={() => setSortMode("by-agent" as SortMode)}
            />
          </div>
        </div>
      </div>

      {/* Approval requests — top priority */}
      <Show when={approvalRequests().length > 0}>
        <div>
          <div class={styles.tierDivider}>
            <span class={styles.dividerLabel}>Approval Required</span>
            <div class={styles.dividerLine} />
          </div>
          <div>
            <For each={approvalRequests()}>
              {(req) => (
                <ApprovalCard
                  request={req}
                  onResolved={handleApprovalResolved}
                />
              )}
            </For>
          </div>
        </div>
      </Show>

      {/* Empty state */}
      <Show when={allItems().length === 0 && approvalRequests().length === 0}>
        <div class={styles.empty}>No items match the current filter.</div>
      </Show>

      {/* Needs Action tier */}
      <Show when={needsActionItems().length > 0}>
        <div>
          <div class={styles.tierDivider}>
            <span class={styles.dividerLabel}>Needs Action</span>
            <div class={styles.dividerLine} />
          </div>
          <VirtualSection items={needsActionItems()} />
        </div>
      </Show>

      {/* Informational tier */}
      <Show when={showInformational()}>
        <div>
          <div class={styles.tierDivider}>
            <span class={styles.dividerLabel}>Informational</span>
            <div class={styles.dividerLine} />
          </div>
          <VirtualSection items={informationalItems()} />
        </div>
      </Show>
    </div>
  );
};

export default TriageView;
