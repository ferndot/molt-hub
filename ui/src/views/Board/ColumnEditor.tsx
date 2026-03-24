/**
 * ColumnEditor — inline panel for configuring kanban columns.
 * Rendered inside BoardView when the gear icon is clicked.
 *
 * Reads pipeline stages from boardStore and persists changes via
 * PATCH /api/pipeline/stages/:id.
 */

import { For, type Component } from "solid-js";
import { getSortedStages, patchStage } from "./boardStore";
import type { PipelineStage } from "../../lib/api";
import styles from "./ColumnEditor.module.css";

// ---------------------------------------------------------------------------
// Stage row
// ---------------------------------------------------------------------------

interface StageRowProps {
  stage: PipelineStage;
}

const StageRow: Component<StageRowProps> = (props) => {
  const stage = () => props.stage;

  return (
    <div class={styles.columnCard}>
      {/* Row 1: handle, label, color */}
      <div class={styles.columnMainRow}>
        <span class={styles.dragHandle} title="Drag to reorder">
          ⠿
        </span>
        <input
          class={styles.inputTitle}
          type="text"
          placeholder="Column title"
          value={stage().label}
          onBlur={(e) => {
            const value = e.currentTarget.value.trim();
            if (value && value !== stage().label) {
              patchStage(stage().id, { label: value });
            }
          }}
        />
        <input
          class={styles.inputColor}
          type="color"
          value={stage().color ?? "#6b7280"}
          title="Column accent color"
          onInput={(e) => patchStage(stage().id, { color: e.currentTarget.value })}
        />
      </div>

      {/* Row 2: behavior fields */}
      <div class={styles.behaviorRow}>
        <label class={styles.fieldLabel}>
          WIP Limit
          <input
            class={styles.inputSmall}
            type="number"
            min="0"
            placeholder="—"
            value={stage().wip_limit ?? ""}
            onBlur={(e) => {
              const raw = e.currentTarget.value;
              const wip_limit = raw === "" ? null : Number(raw);
              if (wip_limit !== stage().wip_limit) {
                patchStage(stage().id, { wip_limit });
              }
            }}
          />
        </label>

        <label class={styles.fieldCheckbox}>
          <input
            type="checkbox"
            checked={stage().requires_approval}
            onChange={(e) =>
              patchStage(stage().id, { requires_approval: e.currentTarget.checked })
            }
          />
          Require Approval
        </label>

        <label class={styles.fieldCheckbox}>
          <input
            type="checkbox"
            checked={stage().terminal}
            onChange={(e) =>
              patchStage(stage().id, { terminal: e.currentTarget.checked })
            }
          />
          Terminal
        </label>
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// ColumnEditor
// ---------------------------------------------------------------------------

export interface ColumnEditorProps {
  onClose: () => void;
}

const ColumnEditor: Component<ColumnEditorProps> = (props) => {
  const sortedStages = () => getSortedStages();

  return (
    <div class={styles.panel}>
      <div class={styles.panelHeader}>
        <h3 class={styles.panelTitle}>Column Configuration</h3>
        <button
          class={styles.btnClose}
          onClick={props.onClose}
          aria-label="Close column editor"
        >
          ✕
        </button>
      </div>

      <div class={styles.columnList}>
        <For each={sortedStages()}>
          {(stage: PipelineStage) => <StageRow stage={stage} />}
        </For>
      </div>
    </div>
  );
};

export default ColumnEditor;
