/**
 * AgentChat — reusable chat stream + input bar for any agent.
 *
 * Renders the unified terminal-style stream (agent output + user steers)
 * and the input bar. Session lifecycle (start/end, localStorage, polling,
 * subscription setup) is the responsibility of the parent component.
 */

import {
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  type Component,
} from "solid-js";
import { getAgent } from "../../views/AgentDetail/agentStore";
import type { OutputLine } from "../../views/AgentDetail/agentStore";
import { sendMessage } from "../../views/AgentDetail/steerStore";
import type { SteerPriority } from "../../views/AgentDetail/steerStore";
import styles from "./AgentChat.module.css";

// ---------------------------------------------------------------------------
// Stream block types
// ---------------------------------------------------------------------------

interface SteerInsertion {
  id: string;
  text: string;
  priority: SteerPriority;
  atLineIndex: number; // outputLines.length when steer was sent
}

type StreamBlock =
  | { kind: "output"; lines: OutputLine[] }
  | { kind: "user"; text: string; priority: SteerPriority };

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

interface AgentChatProps {
  agentId: string;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const AgentChat: Component<AgentChatProps> = (props) => {
  // Steer insertions tracked locally — not persisted.
  const [steerInsertions, setSteerInsertions] = createSignal<SteerInsertion[]>([]);

  // Input state
  const [inputText, setInputText] = createSignal("");
  const [sending, setSending] = createSignal(false);

  // Ref for the stream scroll container
  let streamRef: HTMLDivElement | undefined;

  // Build the unified stream blocks as a memo.
  const blocks = createMemo<StreamBlock[]>(() => {
    const lines: OutputLine[] = getAgent(props.agentId)?.outputLines ?? [];
    const insertions = steerInsertions();

    const result: StreamBlock[] = [];
    let lineIdx = 0;

    for (const ins of insertions) {
      if (ins.atLineIndex > lineIdx) {
        result.push({ kind: "output", lines: lines.slice(lineIdx, ins.atLineIndex) });
      }
      result.push({ kind: "user", text: ins.text, priority: ins.priority });
      lineIdx = ins.atLineIndex;
    }

    if (lineIdx < lines.length) {
      result.push({ kind: "output", lines: lines.slice(lineIdx) });
    }

    return result;
  });

  // Auto-scroll to bottom whenever blocks change.
  createEffect(() => {
    // Access blocks() to track reactivity.
    blocks();
    if (streamRef) {
      streamRef.scrollTop = streamRef.scrollHeight;
    }
  });

  const handleSend = async (priority: SteerPriority) => {
    const text = inputText().trim();
    if (!text || sending()) return;

    const atLineIndex = getAgent(props.agentId)?.outputLines.length ?? 0;

    // Record the insertion locally.
    const insertion: SteerInsertion = {
      id: `ins-${Date.now()}-${Math.random()}`,
      text,
      priority,
      atLineIndex,
    };
    setSteerInsertions((prev) => [...prev, insertion]);
    setInputText("");

    setSending(true);
    try {
      await sendMessage(props.agentId, text, priority);
    } finally {
      setSending(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSend("normal");
    } else if (e.key === "Enter" && e.shiftKey) {
      e.preventDefault();
      void handleSend("urgent");
    }
  };

  return (
    <>
      {/* Unified terminal stream */}
      <div class={styles.stream} ref={streamRef}>
        <Show when={blocks().length === 0}>
          <span class={styles.emptyState}>Waiting for output…</span>
        </Show>
        <For each={blocks()}>
          {(block) => (
            <Show
              when={block.kind === "user"}
              fallback={
                <div class={styles.outputBlock}>
                  <For each={(block as { kind: "output"; lines: OutputLine[] }).lines}>
                    {(line) => <span class={styles.outputLine}>{line.text}{"\n"}</span>}
                  </For>
                </div>
              }
            >
              {() => {
                const b = block as { kind: "user"; text: string; priority: SteerPriority };
                return (
                  <div class={styles.userBlock}>
                    <div class={`${styles.userBubble} ${b.priority === "urgent" ? styles.userBubbleUrgent : ""}`}>
                      {b.text}
                    </div>
                  </div>
                );
              }}
            </Show>
          )}
        </For>
      </div>

      {/* Input bar */}
      <div class={styles.inputRow}>
        <textarea
          class={styles.textInput}
          placeholder="Send a message… (Enter = normal, Shift+Enter = urgent)"
          value={inputText()}
          onInput={(e) => setInputText(e.currentTarget.value)}
          onKeyDown={handleKeyDown}
          rows={1}
          disabled={sending()}
        />
        <button
          type="button"
          class={styles.sendBtn}
          disabled={sending() || !inputText().trim()}
          onClick={() => void handleSend("normal")}
        >
          {sending() ? "…" : "Send"}
        </button>
      </div>
    </>
  );
};

export default AgentChat;
