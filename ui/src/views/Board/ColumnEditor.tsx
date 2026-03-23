/**
 * ColumnEditor — inline panel for configuring kanban columns.
 * Rendered inside BoardView when the gear icon is clicked.
 */

import { For, type Component } from "solid-js";
import {
  settingsState,
  addColumn,
  removeColumn,
  updateColumn,
  updateColumnBehavior,
  updateColumnHooks,
  getSortedColumns,
  parseHookIds,
  serializeHookIds,
} from "../Settings/settingsStore";
import type { KanbanColumn } from "../Settings/settingsStore";
import styles from "./ColumnEditor.module.css";

// ---------------------------------------------------------------------------
// Column row
// ---------------------------------------------------------------------------

interface ColumnRowProps {
  col: KanbanColumn;
}

const ColumnRow: Component<ColumnRowProps> = (props) => {
  const col = () => props.col;

  return (
    <div class={styles.columnCard}>
      {/* Row 1: handle, title, stage, color, remove */}
      <div class={styles.columnMainRow}>
        <span class={styles.dragHandle} title="Drag to reorder">
          ⠿
        </span>
        <input
          class={styles.inputTitle}
          type="text"
          placeholder="Column title"
          value={col().title}
          onInput={(e) => updateColumn(col().id, { title: e.currentTarget.value })}
        />
        <input
          class={styles.inputStage}
          type="text"
          placeholder="stage-name (comma-separated)"
          value={col().stageMatch}
          title="Comma or space separated stage names"
          onInput={(e) => updateColumn(col().id, { stageMatch: e.currentTarget.value })}
        />
        <input
          class={styles.inputColor}
          type="color"
          value={col().color}
          title="Column accent color"
          onInput={(e) => updateColumn(col().id, { color: e.currentTarget.value })}
        />
        <button
          class={styles.btnRemove}
          onClick={() => removeColumn(col().id)}
          title="Remove column"
          aria-label={`Remove column ${col().title}`}
        >
          ✕
        </button>
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
            value={col().behavior.wipLimit ?? ""}
            onInput={(e) => {
              const raw = e.currentTarget.value;
              updateColumnBehavior(col().id, {
                wipLimit: raw === "" ? null : Number(raw),
              });
            }}
          />
        </label>

        <label class={styles.fieldCheckbox}>
          <input
            type="checkbox"
            checked={col().behavior.autoAssign}
            onChange={(e) =>
              updateColumnBehavior(col().id, { autoAssign: e.currentTarget.checked })
            }
          />
          Auto-assign
        </label>

        <label class={styles.fieldCheckbox}>
          <input
            type="checkbox"
            checked={col().behavior.requireApproval}
            onChange={(e) =>
              updateColumnBehavior(col().id, { requireApproval: e.currentTarget.checked })
            }
          />
          Require Approval
        </label>
      </div>

      {/* Row 3: hook fields */}
      <div class={styles.hooksRow}>
        <label class={styles.fieldLabel}>
          On Enter hooks
          <input
            class={styles.inputHooks}
            type="text"
            placeholder="hook-id-1, hook-id-2"
            value={serializeHookIds(col().hooks.onEnter)}
            onBlur={(e) =>
              updateColumnHooks(col().id, { onEnter: parseHookIds(e.currentTarget.value) })
            }
          />
        </label>
        <label class={styles.fieldLabel}>
          On Exit hooks
          <input
            class={styles.inputHooks}
            type="text"
            placeholder="hook-id-1, hook-id-2"
            value={serializeHookIds(col().hooks.onExit)}
            onBlur={(e) =>
              updateColumnHooks(col().id, { onExit: parseHookIds(e.currentTarget.value) })
            }
          />
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
  const sortedColumns = () => getSortedColumns(settingsState.kanbanColumns);

  const handleAddColumn = () => {
    const id = `col-custom-${Date.now()}`;
    addColumn({
      id,
      title: "New Column",
      stageMatch: "",
      color: "#6b7280",
      behavior: {
        wipLimit: null,
        autoAssign: false,
        autoTransition: null,
        requireApproval: false,
      },
      hooks: {
        onEnter: [],
        onExit: [],
        onStall: [],
      },
    });
  };

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
        <For each={sortedColumns()}>
          {(col: KanbanColumn) => <ColumnRow col={col} />}
        </For>
      </div>

      <button class={styles.btnAddColumn} onClick={handleAddColumn}>
        + Add Column
      </button>
    </div>
  );
};

export default ColumnEditor;
