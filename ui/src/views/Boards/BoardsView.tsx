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
import {
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
  const [newId, setNewId] = createSignal("");
  const [newName, setNewName] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  onMount(() => {
    void refreshBoardList();
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
    navigate("/board");
  };

  const handleCreate = async () => {
    const id = newId().trim();
    if (!id) {
      setError("Board id is required.");
      return;
    }
    setError(null);
    setBusy(true);
    try {
      const name = newName().trim();
      await createBoard(id, name || undefined);
      setNewId("");
      setNewName("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async (boardId: string) => {
    if (boardId === "default") return;
    if (!confirm(`Delete board "${boardId}"? This cannot be undone.`)) return;
    setError(null);
    setBusy(true);
    try {
      await deleteBoard(boardId);
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

      <div class={styles.createPanel}>
        <div class={styles.field}>
          <label class={styles.fieldLabel} for="board-new-id">
            Id
          </label>
          <input
            id="board-new-id"
            class={styles.fieldInput}
            placeholder="e.g. release"
            value={newId()}
            onInput={(e) => setNewId(e.currentTarget.value)}
            disabled={busy()}
          />
        </div>
        <div class={styles.field}>
          <label class={styles.fieldLabel} for="board-new-name">
            Display name (optional)
          </label>
          <input
            id="board-new-name"
            class={styles.fieldInput}
            placeholder="Release train"
            value={newName()}
            onInput={(e) => setNewName(e.currentTarget.value)}
            disabled={busy()}
          />
        </div>
        <button
          type="button"
          class={styles.createBtn}
          onClick={() => void handleCreate()}
          disabled={busy()}
        >
          Create board
        </button>
        <Show when={error()}>
          {(msg) => <p class={styles.errorText}>{msg()}</p>}
        </Show>
      </div>

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
                  <div class={styles.cardId}>{board.id}</div>
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
                  <Show when={board.id !== "default"}>
                    <button
                      type="button"
                      class={styles.deleteBtn}
                      onClick={() => void handleDelete(board.id)}
                      disabled={busy()}
                    >
                      Delete
                    </button>
                  </Show>
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
