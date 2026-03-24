import { createSignal, For, Show, type Component } from "solid-js";
import { A, useLocation, useNavigate } from "@solidjs/router";
import { boardState, setActiveBoard } from "../views/Board/boardStore";
import { attentionCount } from "./attentionStore";
import styles from "./BoardList.module.css";

interface Props {
  collapsed?: boolean;
}

function isBoardViewPath(pathname: string): boolean {
  return pathname === "/board";
}

const BoardList: Component<Props> = (props) => {
  const location = useLocation();
  const navigate = useNavigate();
  const [query, setQuery] = createSignal("");

  const filteredBoards = () => {
    const q = query().toLowerCase().trim();
    const all = boardState.boards;
    if (!q) return all;
    return all.filter(
      (b) =>
        b.name.toLowerCase().includes(q) || b.id.toLowerCase().includes(q),
    );
  };

  const openBoard = async (boardId: string) => {
    await setActiveBoard(boardId);
    navigate("/board");
  };

  const rowActive = (boardId: string) =>
    isBoardViewPath(location.pathname) && boardState.activeBoardId === boardId;

  return (
    <div class={styles.section} classList={{ [styles.collapsed]: props.collapsed }}>
      <A
        href="/boards"
        class={styles.sectionTitle}
        classList={{
          [styles.sectionTitleActive]: location.pathname.startsWith("/boards"),
        }}
      >
        <span class={styles.sectionTitleInner}>
          <span>Boards</span>
          <Show when={attentionCount() > 0}>
            <span
              class={styles.attentionBadge}
              title={`${attentionCount()} item(s) needing attention`}
            >
              {attentionCount()}
            </span>
          </Show>
        </span>
      </A>
      <div class={styles.searchWrapper}>
        <input
          class={styles.searchInput}
          type="search"
          placeholder="Search boards..."
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          aria-label="Search boards"
        />
      </div>

      <Show
        when={!props.collapsed}
        fallback={
          <For each={filteredBoards()}>
            {(board) => (
              <button
                type="button"
                class={styles.boardItem}
                classList={{ [styles.active]: rowActive(board.id) }}
                onClick={() => void openBoard(board.id)}
                title={`${board.name} (${board.id})`}
              >
                <span class={styles.boardIcon} />
              </button>
            )}
          </For>
        }
      >
        <For each={filteredBoards()}>
          {(board) => (
            <button
              type="button"
              class={styles.boardItem}
              classList={{ [styles.active]: rowActive(board.id) }}
              onClick={() => void openBoard(board.id)}
            >
              <span class={styles.boardIcon} />
              <div class={styles.boardInfo}>
                <div class={styles.boardName}>{board.name}</div>
                <div class={styles.boardId}>{board.id}</div>
              </div>
            </button>
          )}
        </For>
      </Show>
    </div>
  );
};

export default BoardList;
