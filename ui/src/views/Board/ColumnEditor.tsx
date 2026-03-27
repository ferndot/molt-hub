/**
 * ColumnEditor — modal panel for configuring kanban columns.
 *
 * Persists edits via PATCH per stage; add/remove columns use PUT with the full
 * stage list. Per-column hooks are configured via a visual hook builder UI.
 */

import { For, Show, createEffect, createSignal, type Component } from "solid-js";
import { Dialog } from "@kobalte/core/dialog";
import {
  addBoardColumn,
  boardState,
  getSortedStages,
  patchStage,
  removeBoardColumn,
} from "./boardStore";
import type { HookDefinitionJson, PipelineStage } from "../../lib/api";
import styles from "./ColumnEditor.module.css";

// ---------------------------------------------------------------------------
// Hook builder types
// ---------------------------------------------------------------------------

type HookTrigger = "enter" | "exit"; // TODO: add "on_stall" when stall detection is implemented
type HookKind = "agent_dispatch" | "shell" | "webhook";

interface HookFormState {
  on: HookTrigger;
  kind: HookKind;
  instruction: string;
  adapter: string;
  command: string;
  working_dir: string;
  timeout_seconds: string;
  url: string;
  method: string;
}

function emptyForm(): HookFormState {
  return {
    on: "enter",
    kind: "agent_dispatch",
    instruction: "",
    adapter: "",
    command: "",
    working_dir: "",
    timeout_seconds: "",
    url: "",
    method: "POST",
  };
}

function hookToForm(hook: HookDefinitionJson): HookFormState {
  const h = hook as Record<string, unknown>;
  return {
    on: (h.on as HookTrigger) ?? "enter",
    kind: (h.kind as HookKind) ?? "agent_dispatch",
    instruction: typeof h.instruction === "string" ? h.instruction : "",
    adapter: typeof h.adapter === "string" ? h.adapter : "",
    command: typeof h.command === "string" ? h.command : "",
    working_dir: typeof h.working_dir === "string" ? h.working_dir : "",
    timeout_seconds:
      typeof h.timeout_seconds === "number" ? String(h.timeout_seconds) : "",
    url: typeof h.url === "string" ? h.url : "",
    method: typeof h.method === "string" ? h.method : "POST",
  };
}

function formToHook(form: HookFormState): HookDefinitionJson {
  const base: Record<string, unknown> = { kind: form.kind, on: form.on };
  const timeout =
    form.timeout_seconds !== "" ? Number(form.timeout_seconds) : undefined;
  if (form.kind === "agent_dispatch") {
    base.instruction = form.instruction;
    if (form.adapter) base.adapter = form.adapter;
    if (timeout !== undefined) base.timeout_seconds = timeout;
    if (form.working_dir) base.working_dir = form.working_dir;
  } else if (form.kind === "shell") {
    base.command = form.command;
    if (form.working_dir) base.working_dir = form.working_dir;
    if (timeout !== undefined) base.timeout_seconds = timeout;
  } else if (form.kind === "webhook") {
    base.url = form.url;
    base.method = form.method || "POST";
  }
  return base as HookDefinitionJson;
}

function hookSummary(hook: HookDefinitionJson): string {
  const h = hook as Record<string, unknown>;
  if (h.kind === "agent_dispatch" && typeof h.instruction === "string") {
    return h.instruction.slice(0, 50) + (h.instruction.length > 50 ? "…" : "");
  }
  if (h.kind === "shell" && typeof h.command === "string") {
    return h.command;
  }
  if (h.kind === "webhook" && typeof h.url === "string") {
    return h.url;
  }
  return "";
}

function isFormValid(form: HookFormState): boolean {
  if (form.kind === "agent_dispatch") return form.instruction.trim().length > 0;
  if (form.kind === "shell") return form.command.trim().length > 0;
  if (form.kind === "webhook") return form.url.trim().length > 0;
  return false;
}

// ---------------------------------------------------------------------------
// HookForm — shared add/edit form
// ---------------------------------------------------------------------------

interface HookFormProps {
  initial: HookFormState;
  onSave: (hook: HookDefinitionJson) => void;
  onCancel: () => void;
  saveLabel: string;
}

const HookForm: Component<HookFormProps> = (props) => {
  const [form, setForm] = createSignal<HookFormState>({ ...props.initial });

  const update = <K extends keyof HookFormState>(key: K, value: HookFormState[K]) => {
    setForm((prev) => ({ ...prev, [key]: value }));
  };

  const handleSave = () => {
    if (!isFormValid(form())) return;
    props.onSave(formToHook(form()));
  };

  return (
    <div class={styles.hookAddForm}>
      <div class={styles.hookFormRow}>
        <label>Trigger</label>
        <select
          value={form().on}
          onChange={(e) => update("on", e.currentTarget.value as HookTrigger)}
        >
          <option value="enter">enter</option>
          <option value="exit">exit</option>
          {/* on_stall is not yet implemented — omitted until stall detection is wired */}
        </select>
      </div>

      <div class={styles.hookFormRow}>
        <label>Kind</label>
        <select
          value={form().kind}
          onChange={(e) => update("kind", e.currentTarget.value as HookKind)}
        >
          <option value="agent_dispatch">agent_dispatch</option>
          <option value="shell">shell</option>
          <option value="webhook">webhook</option>
        </select>
      </div>

      <Show when={form().kind === "agent_dispatch"}>
        <div class={styles.hookFormRow}>
          <label>Instruction *</label>
          <textarea
            rows={3}
            placeholder="Agent instruction…"
            value={form().instruction}
            onInput={(e) => update("instruction", e.currentTarget.value)}
          />
        </div>
        <div class={styles.hookFormRow}>
          <label>Adapter</label>
          <input
            type="text"
            placeholder="Optional adapter name"
            value={form().adapter}
            onInput={(e) => update("adapter", e.currentTarget.value)}
          />
        </div>
        <div class={styles.hookFormRow}>
          <label>Timeout (s)</label>
          <input
            type="number"
            min="1"
            placeholder="Optional"
            value={form().timeout_seconds}
            onInput={(e) => update("timeout_seconds", e.currentTarget.value)}
          />
        </div>
      </Show>

      <Show when={form().kind === "shell"}>
        <div class={styles.hookFormRow}>
          <label>Command *</label>
          <input
            type="text"
            placeholder="Shell command"
            value={form().command}
            onInput={(e) => update("command", e.currentTarget.value)}
          />
        </div>
        <div class={styles.hookFormRow}>
          <label>Working Dir</label>
          <input
            type="text"
            placeholder="Optional path"
            value={form().working_dir}
            onInput={(e) => update("working_dir", e.currentTarget.value)}
          />
        </div>
        <div class={styles.hookFormRow}>
          <label>Timeout (s)</label>
          <input
            type="number"
            min="1"
            placeholder="Optional"
            value={form().timeout_seconds}
            onInput={(e) => update("timeout_seconds", e.currentTarget.value)}
          />
        </div>
      </Show>

      <Show when={form().kind === "webhook"}>
        <div class={styles.hookFormRow}>
          <label>URL *</label>
          <input
            type="text"
            placeholder="https://…"
            value={form().url}
            onInput={(e) => update("url", e.currentTarget.value)}
          />
        </div>
        <div class={styles.hookFormRow}>
          <label>Method</label>
          <select
            value={form().method}
            onChange={(e) => update("method", e.currentTarget.value)}
          >
            <option value="POST">POST</option>
            <option value="GET">GET</option>
            <option value="PUT">PUT</option>
            <option value="PATCH">PATCH</option>
          </select>
        </div>
      </Show>

      <div class={styles.hookFormActions}>
        <button
          type="button"
          class={styles.btnHookAction}
          disabled={!isFormValid(form())}
          onClick={handleSave}
        >
          {props.saveLabel}
        </button>
        <button type="button" class={styles.btnHookCancel} onClick={props.onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// HookBuilder — list + add/edit UI
// ---------------------------------------------------------------------------

interface HookBuilderProps {
  stageId: string;
  hooks: HookDefinitionJson[];
}

const HookBuilder: Component<HookBuilderProps> = (props) => {
  const [showAddForm, setShowAddForm] = createSignal(false);
  const [editIndex, setEditIndex] = createSignal<number | null>(null);

  const hooks = () => props.hooks ?? [];

  const saveHooks = (updated: HookDefinitionJson[]) => {
    void patchStage(props.stageId, { hooks: updated });
  };

  const handleAdd = (hook: HookDefinitionJson) => {
    saveHooks([...hooks(), hook]);
    setShowAddForm(false);
  };

  const handleRemove = (idx: number, e: MouseEvent) => {
    e.stopPropagation();
    const updated = hooks().filter((_, i) => i !== idx);
    saveHooks(updated);
    if (editIndex() === idx) setEditIndex(null);
  };

  const handleEdit = (hook: HookDefinitionJson, idx: number) => {
    const updated = hooks().map((h, i) => (i === idx ? hook : h));
    saveHooks(updated);
    setEditIndex(null);
  };

  return (
    <div class={styles.hooksBlock}>
      <span class={styles.hooksBlockLabel}>Hooks</span>

      <Show when={hooks().length > 0}>
        <div class={styles.hookList}>
          <For each={hooks()}>
            {(hook, i) => (
              <>
                <div
                  class={styles.hookRow}
                  onClick={() => {
                    if (editIndex() === i()) {
                      setEditIndex(null);
                    } else {
                      setShowAddForm(false);
                      setEditIndex(i());
                    }
                  }}
                >
                  <span
                    class={`${styles.hookBadge} ${styles.hookBadgeTrigger} ${styles[`hookBadgeTrigger_${(hook as Record<string, unknown>).on as string}`]}`}
                  >
                    {String((hook as Record<string, unknown>).on)}
                  </span>
                  <span
                    class={`${styles.hookBadge} ${styles.hookBadgeKind} ${styles[`hookBadgeKind_${(hook as Record<string, unknown>).kind as string}`]}`}
                  >
                    {String((hook as Record<string, unknown>).kind)}
                  </span>
                  <span class={styles.hookSummary}>{hookSummary(hook)}</span>
                  <button
                    type="button"
                    class={styles.btnHookRemove}
                    title="Remove hook"
                    aria-label="Remove hook"
                    onClick={(e) => handleRemove(i(), e)}
                  >
                    Remove
                  </button>
                </div>
                <Show when={editIndex() === i()}>
                  <HookForm
                    initial={hookToForm(hook)}
                    onSave={(updated) => handleEdit(updated, i())}
                    onCancel={() => setEditIndex(null)}
                    saveLabel="Save"
                  />
                </Show>
              </>
            )}
          </For>
        </div>
      </Show>

      <Show when={showAddForm()}>
        <HookForm
          initial={emptyForm()}
          onSave={handleAdd}
          onCancel={() => setShowAddForm(false)}
          saveLabel="Add"
        />
      </Show>

      <Show when={!showAddForm()}>
        <button
          type="button"
          class={styles.btnAddHook}
          onClick={() => {
            setEditIndex(null);
            setShowAddForm(true);
          }}
        >
          + Add hook
        </button>
      </Show>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Stage row
// ---------------------------------------------------------------------------

interface StageRowProps {
  stage: PipelineStage;
  onRemove: () => void;
  removeDisabled: boolean;
  removeTitle: string;
}

const StageRow: Component<StageRowProps> = (props) => {
  const stage = () => props.stage;

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
        <button
          type="button"
          class={styles.btnRemove}
          disabled={props.removeDisabled}
          title={props.removeTitle}
          aria-label={props.removeTitle}
          onClick={() => props.onRemove()}
        >
          Remove
        </button>
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

      <HookBuilder stageId={stage().id} hooks={stage().hooks ?? []} />
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
  const [structureError, setStructureError] = createSignal<string | null>(null);

  const handleAddColumn = () => {
    setStructureError(null);
    void addBoardColumn().catch((e) =>
      setStructureError(e instanceof Error ? e.message : String(e)),
    );
  };

  const handleRemoveColumn = (stageId: string) => {
    setStructureError(null);
    void removeBoardColumn(stageId).catch((e) =>
      setStructureError(e instanceof Error ? e.message : String(e)),
    );
  };

  const removeTitleFor = (stageId: string) => {
    if (sortedStages().length <= 1) {
      return "The board must keep at least one column";
    }
    const n = boardState.tasks.filter((t) => t.stage === stageId).length;
    if (n > 0) {
      return `Move ${n} task${n === 1 ? "" : "s"} out of this column before removing it`;
    }
    return "Remove this column";
  };

  return (
    <Dialog
      open={props.open}
      onOpenChange={(open: boolean) => {
        if (open) setStructureError(null);
        props.onOpenChange(open);
      }}
    >
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
            Configure columns, limits, and per-column hooks for this board.
          </Dialog.Description>

          <div class={styles.editorBody}>
            <p class={styles.helpText}>
              Add or remove columns below. Hooks run automatically when tasks enter or
              leave a column.
            </p>

            <Show when={structureError()}>
              {(msg) => (
                <div class={styles.structureError} role="alert">
                  {msg()}
                </div>
              )}
            </Show>

            <div class={styles.columnList}>
              <For each={sortedStages()}>
                {(stage: PipelineStage) => (
                  <StageRow
                    stage={stage}
                    onRemove={() => handleRemoveColumn(stage.id)}
                    removeDisabled={
                      sortedStages().length <= 1 ||
                      boardState.tasks.some((t) => t.stage === stage.id)
                    }
                    removeTitle={removeTitleFor(stage.id)}
                  />
                )}
              </For>
            </div>

            <button type="button" class={styles.btnAddColumn} onClick={handleAddColumn}>
              + Add column
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  );
};

export default ColumnEditor;
