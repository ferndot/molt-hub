/**
 * Route shell for `/boards/:id` — keeps the URL and active board in sync.
 */

import { createEffect, type Component } from "solid-js";
import { useNavigate, useParams } from "@solidjs/router";
import BoardView from "./BoardView";
import {
  boardKanbanPath,
  boardState,
  setActiveBoard,
} from "./boardStore";

const BoardPage: Component = () => {
  const params = useParams<{ id: string }>();
  const navigate = useNavigate();

  createEffect(() => {
    const id = params.id;
    if (!id) return;

    const boards = boardState.boards;
    const synced = boardState.boardsSynced;
    const activeBoardId = boardState.activeBoardId;
    const exists = boards.some((b) => b.id === id);

    if (synced && !exists) {
      navigate(
        activeBoardId ? boardKanbanPath(activeBoardId) : "/boards",
        { replace: true },
      );
      return;
    }
    if (exists && id !== activeBoardId) {
      void setActiveBoard(id);
    }
  });

  return <BoardView />;
};

export default BoardPage;
