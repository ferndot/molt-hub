/**
 * HelpOverlay — keyboard shortcut reference sheet.
 *
 * Triggered by "?" key. Escape to close.
 */

import type { Component } from "solid-js";
import styles from "./HelpOverlay.module.css";

interface Props {
  onClose: () => void;
}

interface ShortcutRow {
  keys: string[];
  desc: string;
  chord?: boolean;
}

interface ShortcutGroup {
  title: string;
  rows: ShortcutRow[];
}

const GROUPS: ShortcutGroup[] = [
  {
    title: "Navigation",
    rows: [
      { keys: ["j"], desc: "Move selection down" },
      { keys: ["k"], desc: "Move selection up" },
      { keys: ["Enter"], desc: "Expand / navigate to detail" },
      { keys: ["Esc"], desc: "Collapse / go back" },
    ],
  },
  {
    title: "View switching",
    rows: [
      { keys: ["g", "t"], desc: "Go to Triage", chord: true },
      { keys: ["g", "b"], desc: "Go to Board", chord: true },
      { keys: ["g", "a"], desc: "Go to Agents", chord: true },
    ],
  },
  {
    title: "Triage actions",
    rows: [
      { keys: ["a"], desc: "Approve selected item" },
      { keys: ["r"], desc: "Reject selected item" },
    ],
  },
  {
    title: "Global",
    rows: [
      { keys: ["⌘K"], desc: "Open command palette" },
      { keys: ["?"], desc: "Show this help" },
    ],
  },
];

const HelpOverlay: Component<Props> = (props) => {
  const handleOverlayClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) props.onClose();
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") props.onClose();
  };

  return (
    <div
      class={styles.overlay}
      onClick={handleOverlayClick}
      onKeyDown={handleKeyDown}
      role="dialog"
      aria-modal="true"
      aria-label="Keyboard shortcuts"
      tabindex="-1"
    >
      <div class={styles.modal}>
        <div class={styles.header}>
          <h2 class={styles.title}>Keyboard Shortcuts</h2>
          <button class={styles.closeBtn} onClick={props.onClose} aria-label="Close">
            ×
          </button>
        </div>

        <div class={styles.body}>
          {GROUPS.map((group) => (
            <div class={styles.group}>
              <p class={styles.groupTitle}>{group.title}</p>
              {group.rows.map((row) => (
                <div class={styles.row}>
                  <span class={styles.desc}>{row.desc}</span>
                  <span class={styles.keyList}>
                    {row.chord
                      ? row.keys.map((k, i) => (
                          <>
                            <kbd class={styles.key}>{k}</kbd>
                            {i < row.keys.length - 1 && (
                              <span class={styles.chord}>then</span>
                            )}
                          </>
                        ))
                      : row.keys.map((k) => (
                          <kbd class={styles.key}>{k}</kbd>
                        ))}
                  </span>
                </div>
              ))}
            </div>
          ))}
        </div>

        <div class={styles.footer}>Press Esc to close</div>
      </div>
    </div>
  );
};

export default HelpOverlay;
