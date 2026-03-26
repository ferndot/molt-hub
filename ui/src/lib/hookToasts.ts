/**
 * Hook activity toast store.
 * Emits short-lived notifications when pipeline hooks fire.
 */
import { createSignal } from "solid-js";

export interface HookToast {
  id: number;
  stage: string;
  event: "on_enter" | "on_exit";
  taskName: string;
}

let _nextId = 0;
const [toasts, setToasts] = createSignal<HookToast[]>([]);

export { toasts };

export function emitHookToast(
  stage: string,
  event: "on_enter" | "on_exit",
  taskName: string,
): void {
  const id = ++_nextId;
  setToasts((prev) => [
    ...prev.slice(-4), // keep at most 5 (4 old + 1 new)
    { id, stage, event, taskName },
  ]);
  // Auto-dismiss after 3.5s
  setTimeout(() => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, 3500);
}
