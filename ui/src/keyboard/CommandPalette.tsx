/**
 * CommandPalette — Cmd+K / Ctrl+K modal command launcher.
 *
 * Fuzzy-filtered list of app commands. Arrow keys to navigate,
 * Enter to execute, Escape to close.
 */

import type { Component } from "solid-js";
import { createSignal, createMemo, For, Show } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { Dialog } from "@kobalte/core/dialog";
import { TbOutlineCommand } from "solid-icons/tb";
import { boardKanbanPath, boardState } from "../views/Board/boardStore";
import { filterCommands, type Command } from "./commands";
import styles from "./CommandPalette.module.css";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onShowHelp: () => void;
}

const CommandPalette: Component<Props> = (props) => {
  const navigate = useNavigate();
  const [query, setQuery] = createSignal("");
  const [activeIndex, setActiveIndex] = createSignal(0);

  const results = createMemo(() => filterCommands(query()));

  const close = () => props.onOpenChange(false);

  const executeCommand = (cmd: Command) => {
    close();
    switch (cmd.id) {
      case "goto-triage":
        navigate("/triage");
        break;
      case "goto-board":
        navigate(
          boardState.activeBoardId
            ? boardKanbanPath(boardState.activeBoardId)
            : "/boards",
        );
        break;
      case "goto-boards-list":
        navigate("/boards");
        break;
      case "goto-agents":
        navigate("/agents");
        break;
      case "goto-code-chat":
        navigate("/chat");
        break;
      case "show-help":
        props.onShowHelp();
        break;
      default:
        break;
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    const items = results();
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setActiveIndex((i) => Math.min(i + 1, items.length - 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setActiveIndex((i) => Math.max(i - 1, 0));
        break;
      case "Enter": {
        e.preventDefault();
        const cmd = items[activeIndex()];
        if (cmd) executeCommand(cmd);
        break;
      }
      case "Escape":
        e.preventDefault();
        close();
        break;
      default:
        break;
    }
  };

  const handleInput = (e: InputEvent) => {
    setQuery((e.target as HTMLInputElement).value);
    setActiveIndex(0);
  };

  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay class={styles.overlay} />
        <Dialog.Content class={styles.modal}>
          <Dialog.Title class={styles.srOnly}>Command palette</Dialog.Title>
          <Dialog.Description class={styles.srOnly}>
            Type to filter commands. Use arrow keys to navigate and Enter to run a command.
          </Dialog.Description>
          <div class={styles.searchWrapper}>
            <span class={styles.searchIcon} aria-hidden="true">
              <TbOutlineCommand size={16} />
            </span>
            <input
              class={styles.searchInput}
              type="text"
              placeholder="Type a command..."
              value={query()}
              onInput={handleInput}
              onKeyDown={handleKeyDown}
              autofocus
              aria-label="Search commands"
              aria-autocomplete="list"
              role="combobox"
              aria-expanded="true"
            />
          </div>

          <div class={styles.results} role="listbox">
            <Show
              when={results().length > 0}
              fallback={<div class={styles.empty}>No commands found</div>}
            >
              <For each={results()}>
                {(cmd, i) => (
                  <div
                    class={`${styles.resultItem} ${i() === activeIndex() ? styles.resultItemActive : ""}`}
                    role="option"
                    aria-selected={i() === activeIndex()}
                    onClick={() => executeCommand(cmd)}
                    onMouseEnter={() => setActiveIndex(i())}
                  >
                    <span class={styles.resultLabel}>{cmd.label}</span>
                    <Show when={cmd.description}>
                      <span class={styles.resultDescription}>{cmd.description}</span>
                    </Show>
                  </div>
                )}
              </For>
            </Show>
          </div>

          <div class={styles.footer}>
            <span class={styles.footerHint}>
              <kbd>↑↓</kbd> navigate
            </span>
            <span class={styles.footerHint}>
              <kbd>↵</kbd> select
            </span>
            <span class={styles.footerHint}>
              <kbd>Esc</kbd> close
            </span>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  );
};

export default CommandPalette;
