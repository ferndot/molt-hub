import {
  createSignal,
  createMemo,
  For,
  Show,
  onMount,
  type Component,
} from "solid-js";
import { useNavigate } from "@solidjs/router";
import { TbOutlineLayoutDashboard } from "solid-icons/tb";
import { api, type PipelineStage } from "../../lib/api";
import {
  boardKanbanPath,
  boardState,
  createBoard,
  deleteBoard,
  refreshBoardList,
  setActiveBoard,
} from "../Board/boardStore";
import styles from "./BoardsView.module.css";

const BoardsView: Component = () => {
  const navigate = useNavigate();
  const [query, setQuery] = createSignal("");
  const [newBoardName, setNewBoardName] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [templateStages, setTemplateStages] = createSignal<PipelineStage[]>([]);
  const [boardApiError, setBoardApiError] = createSignal<string | null>(null);

  onMount(() => {
    void (async () => {
      let listErr: string | null = null;
      try {
        await refreshBoardList();
      } catch (e) {
        listErr = e instanceof Error ? e.message : String(e);
      }
      let tmplErr: string | null = null;
      try {
        const res = await api.getBoardTemplate();
        const stages = [...(res.stages ?? [])].sort((a, b) => a.order - b.order);
        setTemplateStages(stages);
      } catch (e) {
        setTemplateStages([]);
        tmplErr = e instanceof Error ? e.message : String(e);
      }
      setBoardApiError(listErr ?? tmplErr);
    })();
  });

  const filtered = createMemo(() => {
    const q = query().toLowerCase().trim();
    const boards = boardState.boards;
    if (!q) return boards;
    return boards.filter(
      (b) =>
        b.name.toLowerCase().includes(q) || b.id.toLowerCase().includes(q),
    );
  });

  const openBoard = async (boardId: string) => {
    await setActiveBoard(boardId);
    navigate(boardKanbanPath(boardId));
  };

  const handleCreate = async () => {
    const name = newBoardName().trim();
    if (!name) {
      setError("Board name is required.");
      return;
    }
    setError(null);
    setBusy(true);
    try {
      const id = await createBoard(name);
      setNewBoardName("");
      navigate(boardKanbanPath(id));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async (board: { id: string; name: string }) => {
    if (!confirm(`Delete board "${board.name}"? This cannot be undone.`)) return;
    setError(null);
    setBusy(true);
    try {
      await deleteBoard(board.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class={styles.container}>
      <div class={styles.header}>
        <h2 class={styles.title}>Boards</h2>
        <span class={styles.countBadge}>{boardState.boards.length}</span>
        <input
          class={styles.searchInput}
          type="search"
          placeholder="Filter boards..."
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          aria-label="Filter boards"
        />
      </div>

      <Show when={boardApiError()}>
        {(msg) => (
          <p class={styles.staleBanner} role="alert">
            {msg()}
          </p>
        )}
      </Show>

      <Show when={templateStages().length > 0}>
        <div class={styles.templatePanel} role="region" aria-label="New board template">
          <div class={styles.templateTitle}>New boards start with these columns</div>
          <ul class={styles.templateList}>
            <For each={templateStages()}>
              {(s) => (
                <li>
                  <span class={styles.templateLabel}>{s.label}</span>
                  <span class={styles.templateId}>{s.id}</span>
                </li>
              )}
            </For>
          </ul>
        </div>
      </Show>

      <form
        class={styles.createPanel}
        onSubmit={(e) => {
          e.preventDefault();
          if (busy()) return;
          void handleCreate();
        }}
      >
        <div class={`${styles.field} ${styles.fieldGrow}`}>
          <label class={styles.fieldLabel} for="board-new-name">
            Board name
          </label>
          <input
            id="board-new-name"
            class={styles.fieldInput}
            placeholder="e.g. Release train"
            value={newBoardName()}
            onInput={(e) => setNewBoardName(e.currentTarget.value)}
            disabled={busy()}
          />
        </div>
        <button type="submit" class={styles.createBtn} disabled={busy()}>
          Create board
        </button>
        <Show when={error()}>
          {(msg) => <p class={styles.errorText}>{msg()}</p>}
        </Show>
      </form>

      <div class={styles.list}>
        <Show
          when={filtered().length > 0}
          fallback={
            <div class={styles.emptyState}>
              <TbOutlineLayoutDashboard size={32} />
              <span>No boards match the current filter.</span>
            </div>
          }
        >
          <For each={filtered()}>
            {(board) => (
              <div class={styles.boardCard}>
                <div class={styles.cardMain}>
                  <div class={styles.cardTitle}>{board.name}</div>
                </div>
                <div class={styles.cardActions}>
                  <button
                    type="button"
                    class={styles.openBtn}
                    onClick={() => void openBoard(board.id)}
                    disabled={busy()}
                  >
                    Open
                  </button>
                  <button
                    type="button"
                    class={styles.deleteBtn}
                    onClick={() => void handleDelete(board)}
                    disabled={busy()}
                  >
                    Delete
                  </button>
                </div>
              </div>
            )}
          </For>
        </Show>
      </div>
    </div>
  );
};

export default BoardsView;
