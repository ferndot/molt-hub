/**
 * SteerChat — chat panel for sending steering messages to a running agent.
 *
 * Shows message history (human messages + agent responses) in a scrollable
 * list with an input bar at the bottom. Supports normal and urgent priority.
 *
 * - Enter sends with "normal" priority
 * - Shift+Enter sends with "urgent" priority
 *
 * When the chat is fresh (no messages), initial suggestion buttons are shown.
 * Two kinds:
 * - "complete": clicking immediately sends the full text as a message
 * - "partial": clicking populates the input with the text (minus trailing "…")
 *   and focuses the input so the user can finish the sentence
 */

import type { Component } from "solid-js";
import { For, Show, createSignal, createEffect, onMount } from "solid-js";
import { FaSolidCommentAlt } from "solid-icons/fa";
import {
  getMessages,
  sendMessage,
  isSending,
  type SteerMessage,
  type SteerPriority,
} from "./steerStore";
import styles from "./SteerChat.module.css";

// ---------------------------------------------------------------------------
// Suggestion types
// ---------------------------------------------------------------------------

export type SuggestionKind = "complete" | "partial";

export interface Suggestion {
  kind: SuggestionKind;
  /** Display text shown on the button */
  text: string;
  /**
   * Full message sent (complete) or pre-filled in input (partial).
   * Defaults to `text` if omitted.
   */
  value?: string;
}

/** Default suggestions shown when no messages exist yet. */
const DEFAULT_SUGGESTIONS: Suggestion[] = [
  { kind: "complete", text: "What is your current status?" },
  { kind: "complete", text: "Summarise what you have done so far." },
  { kind: "partial", text: "Explain why you chose…" },
  { kind: "partial", text: "Show me the code for…" },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatTime(isoString: string): string {
  return new Date(isoString).toLocaleTimeString("en-US", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface Props {
  agentId: string;
  /** Override the default initial suggestions. Pass [] to hide them. */
  suggestions?: Suggestion[];
}

const SteerChat: Component<Props> = (props) => {
  let scrollRef: HTMLDivElement | undefined;
  let textareaRef: HTMLTextAreaElement | undefined;

  const [inputValue, setInputValue] = createSignal("");

  const messages = () => getMessages(props.agentId);
  const sending = () => isSending(props.agentId);
  const suggestions = () => props.suggestions ?? DEFAULT_SUGGESTIONS;

  // Auto-scroll to bottom when messages change
  function scrollToBottom(): void {
    if (scrollRef) {
      scrollRef.scrollTop = scrollRef.scrollHeight;
    }
  }

  onMount(() => scrollToBottom());

  createEffect(() => {
    void messages().length;
    // Use requestAnimationFrame to ensure DOM has updated
    requestAnimationFrame(scrollToBottom);
  });

  // Auto-resize textarea
  function adjustTextarea(): void {
    if (textareaRef) {
      textareaRef.style.height = "auto";
      textareaRef.style.height = `${Math.min(textareaRef.scrollHeight, 120)}px`;
    }
  }

  async function handleSend(priority: SteerPriority = "normal"): Promise<void> {
    const content = inputValue().trim();
    if (!content || sending()) return;

    setInputValue("");
    if (textareaRef) {
      textareaRef.style.height = "auto";
    }

    await sendMessage(props.agentId, content, priority);
  }

  async function handleSuggestionClick(suggestion: Suggestion): Promise<void> {
    const value = suggestion.value ?? suggestion.text;
    if (suggestion.kind === "complete") {
      await sendMessage(props.agentId, value, "normal");
    } else {
      // Partial: strip trailing "…" or "..." and populate the input
      const seed = value.replace(/[…\.]{1,3}\s*$/, "").trimEnd();
      setInputValue(seed);
      if (textareaRef) {
        textareaRef.style.height = "auto";
        textareaRef.style.height = `${Math.min(textareaRef.scrollHeight, 120)}px`;
        textareaRef.focus();
        // Move cursor to end
        const len = textareaRef.value.length;
        textareaRef.setSelectionRange(len, len);
      }
    }
  }

  function handleKeyDown(e: KeyboardEvent): void {
    if (e.key === "Enter" && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      if (e.shiftKey) {
        handleSend("urgent");
      } else {
        handleSend("normal");
      }
    }
  }

  return (
    <div class={styles.container}>
      {/* Header */}
      <div class={styles.header}>
        <span class={styles.headerTitle}>Steering Chat</span>
        <Show when={messages().length > 0}>
          <span class={styles.messageCount}>
            {messages().length} message{messages().length !== 1 ? "s" : ""}
          </span>
        </Show>
      </div>

      {/* Message list */}
      <div class={styles.messageList} ref={scrollRef}>
        <Show
          when={messages().length > 0}
          fallback={
            <div class={styles.empty}>
              <p class={styles.emptyHint}>
                Send a message to steer this agent.
              </p>
              <Show when={suggestions().length > 0}>
                <div class={styles.suggestions}>
                  <For each={suggestions()}>
                    {(s) => (
                      <button
                        type="button"
                        class={styles.suggestionBtn}
                        onClick={() => handleSuggestionClick(s)}
                        disabled={sending()}
                      >
                        <FaSolidCommentAlt class={styles.suggestionIcon} />
                        <span>{s.text}</span>
                      </button>
                    )}
                  </For>
                </div>
              </Show>
            </div>
          }
        >
          <For each={messages()}>
            {(msg: SteerMessage) => (
              <div
                class={`${styles.message} ${
                  msg.role === "human" ? styles.messageHuman : styles.messageAgent
                }`}
              >
                <Show when={msg.role === "human" && msg.priority === "urgent"}>
                  <span class={styles.urgentLabel}>Urgent</span>
                </Show>
                <div
                  class={`${styles.bubble} ${
                    msg.role === "human"
                      ? msg.priority === "urgent"
                        ? styles.bubbleUrgent
                        : styles.bubbleHuman
                      : styles.bubbleAgent
                  }`}
                >
                  {msg.content}
                </div>
                <span class={styles.timestamp}>{formatTime(msg.timestamp)}</span>
              </div>
            )}
          </For>
        </Show>
      </div>

      {/* Hint */}
      <div class={styles.hint}>
        Shift+Enter = urgent
      </div>

      {/* Input area */}
      <div class={styles.inputArea}>
        <textarea
          ref={textareaRef}
          class={styles.textInput}
          placeholder="Message this agent..."
          value={inputValue()}
          onInput={(e) => {
            setInputValue(e.currentTarget.value);
            adjustTextarea();
          }}
          onKeyDown={handleKeyDown}
          rows={1}
          disabled={sending()}
        />
        <button
          class={styles.sendBtn}
          onClick={() => handleSend("normal")}
          disabled={!inputValue().trim() || sending()}
          type="button"
          title="Send message (Enter)"
        >
          {sending() ? "\u2026" : "\u2191"}
        </button>
      </div>
    </div>
  );
};

export default SteerChat;
