/**
 * AiTutorChat — chat panel for AI Tutor sessions.
 *
 * Shows message history (student messages + tutor responses) in a scrollable
 * list with an input bar at the bottom.
 *
 * - Enter sends the message
 * - Two suggestion kinds:
 *   - "complete": clicking immediately sends the full text as a message
 *   - "partial": clicking populates the input with the text (minus trailing "…")
 *     and focuses the input so the user can finish the sentence
 *
 * When the chat is fresh (no messages), default suggestion buttons are shown.
 * After each tutor message, per-message suggestions may be shown if the tutor
 * agent emitted a SuggestFollowups tool call.
 *
 * Tool call parsing: lines matching the SuggestFollowups pattern are consumed
 * and NOT rendered as text output.
 */

import type { Component } from "solid-js";
import { For, Show, createSignal, createEffect, onMount, onCleanup } from "solid-js";
import { FaSolidCommentAlt } from "solid-icons/fa";
import {
  getMessages,
  sendMessage,
  isSending,
  addTutorLine,
  attachSuggestions,
  type TutorMessage,
} from "./tutorStore";
import type { Suggestion } from "../../types/chat";
import { subscribe } from "../../lib/ws";
import styles from "./AiTutorChat.module.css";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/** Default suggestions shown when no messages exist yet. */
const DEFAULT_SUGGESTIONS: Suggestion[] = [
  { kind: "partial",  text: "Find the lesson that covers…" },
  { kind: "partial",  text: "Can you explain…" },
  { kind: "partial",  text: "What is the difference between…" },
  { kind: "partial",  text: "Help me understand why…" },
  { kind: "complete", text: "What topics should I study next?" },
  { kind: "complete", text: "Give me a quiz on what I just learned." },
];

/**
 * Regex to detect SuggestFollowups tool call lines emitted by the agent.
 * Matches lines like: "⏺ SuggestFollowups({...})" or "● SuggestFollowups({...})"
 */
const SUGGEST_TOOL_RE = /^[⏺●]\s+SuggestFollowups\((.+)\)\s*$/;

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
  sessionId: string;
  /** Override the default initial suggestions. Pass [] to hide. */
  initialSuggestions?: Suggestion[];
}

const AiTutorChat: Component<Props> = (props) => {
  let scrollRef: HTMLDivElement | undefined;
  let textareaRef: HTMLTextAreaElement | undefined;

  const [inputValue, setInputValue] = createSignal("");

  const messages = () => getMessages(props.sessionId);
  const sending = () => isSending(props.sessionId);
  const initialSuggestions = () => props.initialSuggestions ?? DEFAULT_SUGGESTIONS;

  // Auto-scroll to bottom when messages change
  function scrollToBottom(): void {
    if (scrollRef) {
      scrollRef.scrollTop = scrollRef.scrollHeight;
    }
  }

  onMount(() => scrollToBottom());

  createEffect(() => {
    void messages().length;
    requestAnimationFrame(scrollToBottom);
  });

  // WebSocket subscription for tutor output
  onMount(() => {
    const topic = `tutor:${props.sessionId}`;
    const unsub = subscribe(topic, (msg) => {
      if (msg.type !== "event") return;
      const payload = msg.payload as Record<string, unknown>;

      if (payload.type === "tutor_output") {
        const line = payload.line as string;
        const match = SUGGEST_TOOL_RE.exec(line);
        if (match) {
          // Parse the JSON argument and attach suggestions to the last tutor message
          try {
            const parsed = JSON.parse(match[1]) as { suggestions: Suggestion[] };
            const msgs = getMessages(props.sessionId);
            // Find the last tutor message to attach suggestions to
            for (let i = msgs.length - 1; i >= 0; i--) {
              if (msgs[i].role === "tutor") {
                attachSuggestions(props.sessionId, msgs[i].id, parsed.suggestions);
                break;
              }
            }
          } catch {
            // Malformed JSON — ignore
          }
          // Do NOT add this line as text output
          return;
        }
        addTutorLine(props.sessionId, line);
      } else if (payload.type === "tutor_error") {
        const errorMsg = payload.message as string;
        addTutorLine(props.sessionId, `[Error] ${errorMsg}`);
      }
    });
    onCleanup(unsub);
  });

  // Auto-resize textarea
  function adjustTextarea(): void {
    if (textareaRef) {
      textareaRef.style.height = "auto";
      textareaRef.style.height = `${Math.min(textareaRef.scrollHeight, 120)}px`;
    }
  }

  async function handleSend(): Promise<void> {
    const content = inputValue().trim();
    if (!content || sending()) return;

    setInputValue("");
    if (textareaRef) {
      textareaRef.style.height = "auto";
    }

    await sendMessage(props.sessionId, content);
  }

  async function handleSuggestionClick(suggestion: Suggestion): Promise<void> {
    const value = suggestion.value ?? suggestion.text;
    if (suggestion.kind === "complete") {
      await sendMessage(props.sessionId, value);
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
    if (e.key === "Enter" && !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      void handleSend();
    }
  }

  // Determine the id of the last tutor message (for interactive suggestions)
  const lastTutorMessageId = () => {
    const msgs = messages();
    for (let i = msgs.length - 1; i >= 0; i--) {
      if (msgs[i].role === "tutor") return msgs[i].id;
    }
    return null;
  };

  return (
    <div class={styles.container}>
      {/* Header */}
      <div class={styles.header}>
        <span class={styles.headerTitle}>AI Tutor</span>
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
                Ask the AI Tutor anything to get started.
              </p>
              <Show when={initialSuggestions().length > 0}>
                <div class={styles.suggestions}>
                  <For each={initialSuggestions()}>
                    {(s) => (
                      <button
                        type="button"
                        class={styles.suggestionBtn}
                        onClick={() => void handleSuggestionClick(s)}
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
            {(msg: TutorMessage) => {
              const isLastTutor = () => msg.role === "tutor" && msg.id === lastTutorMessageId();
              return (
                <div
                  class={`${styles.message} ${
                    msg.role === "student" ? styles.messageStudent : styles.messageTutor
                  }`}
                >
                  <div
                    class={`${styles.bubble} ${
                      msg.role === "student" ? styles.bubbleStudent : styles.bubbleTutor
                    }`}
                  >
                    {msg.content}
                  </div>
                  <span class={styles.timestamp}>{formatTime(msg.timestamp)}</span>
                  {/* Per-message suggestions (only for tutor messages with suggestions) */}
                  <Show when={msg.role === "tutor" && msg.suggestions && msg.suggestions.length > 0}>
                    <div class={styles.messageSuggestions}>
                      <For each={msg.suggestions}>
                        {(s) => (
                          <button
                            type="button"
                            class={styles.suggestionBtn}
                            onClick={() => void handleSuggestionClick(s)}
                            disabled={!isLastTutor() || sending()}
                          >
                            <FaSolidCommentAlt class={styles.suggestionIcon} />
                            <span>{s.text}</span>
                          </button>
                        )}
                      </For>
                    </div>
                  </Show>
                </div>
              );
            }}
          </For>
        </Show>
      </div>

      {/* Input area */}
      <div class={styles.inputArea}>
        <textarea
          ref={textareaRef}
          class={styles.textInput}
          placeholder="Ask the AI Tutor..."
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
          onClick={() => void handleSend()}
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

export default AiTutorChat;
