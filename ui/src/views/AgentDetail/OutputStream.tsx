/**
 * OutputStream — terminal-style scrollable output for an agent's log.
 *
 * Auto-scrolls to bottom on new output. Dark background, monospace font,
 * green-tinted text.
 */

import type { Component } from "solid-js";
import { For, Show, createEffect, onMount } from "solid-js";
import type { AgentDetail, OutputLine } from "./agentStore";
import styles from "./OutputStream.module.css";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function dotClass(status: AgentDetail["status"]): string {
  switch (status) {
    case "paused":
      return styles.dotPaused;
    case "terminated":
      return styles.dotTerminated;
    case "idle":
      return styles.dotIdle;
    default:
      return "";
  }
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface Props {
  lines: OutputLine[];
  status: AgentDetail["status"];
}

const OutputStream: Component<Props> = (props) => {
  let scrollRef: HTMLDivElement | undefined;

  function scrollToBottom(): void {
    if (scrollRef) {
      scrollRef.scrollTop = scrollRef.scrollHeight;
    }
  }

  onMount(() => scrollToBottom());

  createEffect(() => {
    // Re-run when lines change
    void props.lines.length;
    scrollToBottom();
  });

  return (
    <div class={styles.container}>
      {/* Header bar */}
      <div class={styles.header}>
        <span class={`${styles.dot} ${dotClass(props.status)}`} />
        <span class={styles.headerTitle}>Output Stream</span>
      </div>

      {/* Scrollable log area */}
      <div class={styles.scroll} ref={scrollRef}>
        <Show
          when={props.lines.length > 0}
          fallback={<div class={styles.empty}>No output yet</div>}
        >
          <For each={props.lines}>
            {(line: OutputLine) => (
              <div class={styles.line}>
                <span class={styles.timestamp}>[{line.timestamp}]</span>
                <span class={styles.text}>{line.text}</span>
              </div>
            )}
          </For>
        </Show>
      </div>
    </div>
  );
};

export default OutputStream;
