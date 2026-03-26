import type { Component } from "solid-js";
import { For } from "solid-js";
import { toasts } from "../../lib/hookToasts";
import styles from "./HookActivityToast.module.css";

const HOOK_ICONS: Record<string, string> = {
  on_enter: "→",
  on_exit: "←",
};

const HookActivityToast: Component = () => (
  <div class={styles.toastStack} aria-live="polite">
    <For each={toasts()}>
      {(t) => (
        <div class={styles.toast}>
          <span class={styles.hookIcon}>🪝</span>
          <span class={styles.hookText}>
            <span class={styles.hookEvent}>{t.event}</span>
            <span class={styles.hookStage}>:{t.stage}</span>
          </span>
          <span class={styles.hookArrow}>{HOOK_ICONS[t.event]}</span>
          <span class={styles.hookTask}>{t.taskName}</span>
        </div>
      )}
    </For>
  </div>
);

export default HookActivityToast;
