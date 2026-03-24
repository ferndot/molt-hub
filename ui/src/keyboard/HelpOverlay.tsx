/**
 * HelpOverlay — keyboard shortcut reference sheet.
 *
 * Triggered by "?" key. Escape to close.
 */

import type { Component } from "solid-js";
import { Dialog } from "@kobalte/core/dialog";
import { TbOutlineX } from "solid-icons/tb";
import styles from "./HelpOverlay.module.css";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
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
      { keys: ["g", "b"], desc: "Go to Workboard", chord: true },
      { keys: ["g", "a"], desc: "Go to Agents", chord: true },
      { keys: ["g", "c"], desc: "Go to Claude Code", chord: true },
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
  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay class={styles.overlay} />
        <Dialog.Content class={styles.modal}>
          <div class={styles.header}>
            <Dialog.Title class={styles.title}>Keyboard Shortcuts</Dialog.Title>
            <Dialog.CloseButton class={styles.closeBtn} aria-label="Close">
              <TbOutlineX size={16} />
            </Dialog.CloseButton>
          </div>
          <Dialog.Description class={styles.srOnly}>
            Reference of keyboard shortcuts. Press Escape to close.
          </Dialog.Description>

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
                        : row.keys.map((k) => <kbd class={styles.key}>{k}</kbd>)}
                    </span>
                  </div>
                ))}
              </div>
            ))}
          </div>

          <div class={styles.footer}>Press Esc to close</div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  );
};

export default HelpOverlay;
