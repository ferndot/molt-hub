/**
 * ColumnEditor — modal panel for configuring kanban columns.
 *
 * Reads pipeline stages from boardStore and persists changes via
 * PATCH /api/projects/…/boards/:boardId/stages/:stageId.
 */

import { For, createEffect, createSignal, type Component } from "solid-js";
import { Dialog } from "@kobalte/core/dialog";
import { getSortedStages, patchStage } from "./boardStore";
import type { HookDefinitionJson, PipelineStage } from "../../lib/api";
import styles from "./ColumnEditor.module.css";

// ---------------------------------------------------------------------------
// Stage row
// ---------------------------------------------------------------------------

interface StageRowProps {
  stage: PipelineStage;
}

const StageRow: Component<StageRowProps> = (props) => {
  const stage = () => props.stage;
  const [hooksText, setHooksText] = createSignal("[]");

  createEffect(() => {
    const h = stage().hooks;
    setHooksText(JSON.stringify(h ?? [], null, 2));
  });

  return (
    <div class={styles.columnCard}>
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

      <div class={styles.hooksBlock}>
        <label class={styles.hooksBlockLabel} for={`hooks-${stage().id}`}>
          Hooks (JSON array)
        </label>
        <textarea
          id={`hooks-${stage().id}`}
          class={styles.hooksTextarea}
          rows={5}
          spellcheck={false}
          value={hooksText()}
          onInput={(e) => setHooksText(e.currentTarget.value)}
          onBlur={() => {
            try {
              const parsed = JSON.parse(hooksText()) as unknown;
              if (!Array.isArray(parsed)) {
                throw new Error("hooks must be a JSON array");
              }
              void patchStage(stage().id, {
                hooks: parsed as HookDefinitionJson[],
              });
            } catch {
              setHooksText(JSON.stringify(stage().hooks ?? [], null, 2));
            }
          }}
        />
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// ColumnEditor
// ---------------------------------------------------------------------------

export interface ColumnEditorProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const ColumnEditor: Component<ColumnEditorProps> = (props) => {
  const sortedStages = () => getSortedStages();

  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay class={styles.overlay} />
        <Dialog.Content class={styles.dialogContent}>
          <div class={styles.panelHeader}>
            <Dialog.Title class={styles.panelTitle}>Column Configuration</Dialog.Title>
            <Dialog.CloseButton
              class={styles.btnClose}
              aria-label="Close column editor"
            >
              ✕
            </Dialog.CloseButton>
          </div>
          <Dialog.Description class={styles.srOnly}>
            Edit column titles, colors, WIP limits, and behavior for this board.
          </Dialog.Description>

          <div class={styles.columnList}>
            <For each={sortedStages()}>
              {(stage: PipelineStage) => <StageRow stage={stage} />}
            </For>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  );
};

export default ColumnEditor;
