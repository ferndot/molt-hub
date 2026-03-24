/**
 * Kobalte-backed import menu — portaled popper with viewport-aware placement.
 */

import { DropdownMenu } from "@kobalte/core/dropdown-menu";
import { Show, type Component } from "solid-js";
import { TbOutlineChevronDown } from "solid-icons/tb";
import styles from "./ImportIssuesMenu.module.css";

export interface ImportIssuesMenuProps {
  jiraConnected: boolean;
  githubConnected: boolean;
  onSelectJira: () => void;
  onSelectGitHub: () => void;
}

const ImportIssuesMenu: Component<ImportIssuesMenuProps> = (props) => {
  return (
    <DropdownMenu
      placement="top"
      gutter={6}
      flip
      slide
      fitViewport
      sameWidth
      modal={false}
    >
      <DropdownMenu.Trigger class={styles.trigger}>
        Import issues
        <TbOutlineChevronDown size={14} class={styles.triggerIcon} aria-hidden />
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class={styles.content}>
          <Show when={props.jiraConnected}>
            <DropdownMenu.Item
              class={styles.item}
              onSelect={() => props.onSelectJira()}
            >
              <DropdownMenu.ItemLabel>Jira</DropdownMenu.ItemLabel>
            </DropdownMenu.Item>
          </Show>
          <Show when={props.githubConnected}>
            <DropdownMenu.Item
              class={styles.item}
              onSelect={() => props.onSelectGitHub()}
            >
              <DropdownMenu.ItemLabel>GitHub</DropdownMenu.ItemLabel>
            </DropdownMenu.Item>
          </Show>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu>
  );
};

export default ImportIssuesMenu;
