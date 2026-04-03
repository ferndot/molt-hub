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
import { marked } from "marked";
import DOMPurify from "dompurify";
import { getAgent, insertUserSteer } from "../../views/AgentDetail/agentStore";
import type { ChatEvent } from "../../views/AgentDetail/agentStore";
import { sendMessage } from "../../views/AgentDetail/steerStore";
import type { SteerPriority } from "../../views/AgentDetail/steerStore";
import { api } from "../../lib/api";
import styles from "./AgentChat.module.css";

// Configure marked for GitHub-flavored markdown (gfm is the default in v5+)
marked.use({ breaks: true });

// ---------------------------------------------------------------------------
// Markdown renderer (plain text segments)
// ---------------------------------------------------------------------------

function renderMarkdownText(lines: string[]): string {
  const raw  = lines.join("\n");
  const html = marked.parse(raw) as string;
  return DOMPurify.sanitize(html);
}

// ---------------------------------------------------------------------------
// Extract a short primary arg for display in tool call summaries
// ---------------------------------------------------------------------------

function extractPrimaryArg(input: unknown): string {
  if (typeof input === "string") {
    return input.length > 60 ? input.slice(0, 57) + "…" : input;
  }
  if (input && typeof input === "object") {
    const obj = input as Record<string, unknown>;
    // Prefer path/file/command/query/pattern as the primary display arg
    const preferredKeys = ["path", "file_path", "command", "query", "pattern", "url", "content"];
    for (const key of preferredKeys) {
      if (typeof obj[key] === "string") {
        const v = obj[key] as string;
        return v.length > 60 ? v.slice(0, 57) + "…" : v;
      }
    }
    // Fall back to first string value
    for (const val of Object.values(obj)) {
      if (typeof val === "string") {
        return val.length > 60 ? val.slice(0, 57) + "…" : val;
      }
    }
  }
  return "";
}

// ---------------------------------------------------------------------------
// ToolCallBlock
// ---------------------------------------------------------------------------

type ToolCallEvent = Extract<ChatEvent, { kind: "tool_call" }>;

const ToolCallBlock: Component<{ event: ToolCallEvent }> = (props) => {
  const primaryArg = createMemo(() => extractPrimaryArg(props.event.input));
  const statusClass = createMemo(() => {
    if (!props.event.completedAt) return styles.toolCallStatusPending;
    if (props.event.isError) return styles.toolCallStatusError;
    return styles.toolCallStatusDone;
  });
  const statusGlyph = createMemo(() => {
    if (!props.event.completedAt) return "⋯";
    if (props.event.isError) return "✗";
    return "✓";
  });

  return (
    <details class={styles.toolCall}>
      <summary class={styles.toolCallSummary}>
        <span class={`${styles.toolCallStatus} ${statusClass()}`}>{statusGlyph()}</span>
        <span class={styles.toolCallName}>{props.event.toolName}</span>
        <Show when={primaryArg()}>
          <span class={styles.toolCallArgs}>({primaryArg()})</span>
        </Show>
      </summary>
      <div class={styles.toolCallBody}>
        <Show when={props.event.input !== undefined && props.event.input !== null}>
          <div class={styles.toolCallSection}>
            <div class={styles.toolCallSectionLabel}>Input</div>
            <pre class={styles.toolCallPre}>
              <code>
                {typeof props.event.input === "string"
                  ? props.event.input
                  : JSON.stringify(props.event.input, null, 2)}
              </code>
            </pre>
          </div>
        </Show>
        <Show when={props.event.result !== undefined && props.event.result !== null}>
          <div class={styles.toolCallSection}>
            <div class={styles.toolCallSectionLabel}>Output</div>
            <pre class={styles.toolCallPre}>
              <code>
                {typeof props.event.result === "string"
                  ? props.event.result
                  : JSON.stringify(props.event.result, null, 2)}
              </code>
            </pre>
          </div>
        </Show>
      </div>
    </details>
  );
};

// ---------------------------------------------------------------------------
// ThinkingBlock
// ---------------------------------------------------------------------------

type ThinkingEvent = Extract<ChatEvent, { kind: "thinking" }>;

const ThinkingBlock: Component<{ event: ThinkingEvent }> = (props) => {
  return (
    <details class={styles.thinkingBlock}>
      <summary class={styles.thinkingBlockSummary}>Thinking…</summary>
      <pre class={styles.thinkingContent}>{props.event.lines.join("\n")}</pre>
    </details>
  );
};

// ---------------------------------------------------------------------------
// AskUserQuestionBlock
// ---------------------------------------------------------------------------

type AskUserQuestionEvent = Extract<ChatEvent, { kind: "tool_call" }> & { toolName: "AskUserQuestion" };

const AskUserQuestionBlock: Component<{ event: AskUserQuestionEvent; agentId: string }> = (props) => {
  const input = createMemo(() => {
    const raw = props.event.input;
    if (raw && typeof raw === "object") return raw as { question?: string; options?: string[] };
    return {};
  });

  const question = createMemo(() => input().question ?? "");
  const options = createMemo(() => input().options ?? []);

  const [answer, setAnswer] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);

  const submit = async (text: string) => {
    if (!text.trim() || submitting()) return;
    setSubmitting(true);
    try {
      // Insert the answer as a user event in the timeline so it appears in chat
      insertUserSteer(props.agentId, text.trim(), "normal");
      // Send as steer so the agent receives the answer text
      await sendMessage(props.agentId, text.trim(), "normal");
      // Auto-approve the pending tool call
      await api.respondToolApproval(props.agentId, { requestId: props.event.callId, approved: true });
      setAnswer("");
    } finally {
      setSubmitting(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit(answer());
    }
  };

  return (
    <div class={styles.askQuestion}>
      <div class={styles.askQuestionLabel}>Question</div>
      <div class={styles.askQuestionText}>{question()}</div>
      <Show
        when={options().length > 0}
        fallback={
          <div class={styles.askQuestionInput}>
            <textarea
              class={styles.textInput}
              placeholder="Type your answer…"
              value={answer()}
              onInput={(e) => setAnswer(e.currentTarget.value)}
              onKeyDown={handleKeyDown}
              rows={1}
              disabled={submitting()}
            />
            <button
              type="button"
              class={styles.sendBtn}
              disabled={submitting() || !answer().trim()}
              onClick={() => void submit(answer())}
            >
              {submitting() ? "…" : "Answer"}
            </button>
          </div>
        }
      >
        <div class={styles.askQuestionOptions}>
          <For each={options()}>
            {(opt) => (
              <button
                type="button"
                class={styles.askQuestionOption}
                disabled={submitting()}
                onClick={() => void submit(opt)}
              >
                {opt}
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

interface AgentChatProps {
  agentId: string;
  status?: "running" | "paused" | "terminated" | "idle";
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const AgentChat: Component<AgentChatProps> = (props) => {
  // Input state
  const [inputText, setInputText] = createSignal("");
  const [sending, setSending] = createSignal(false);

  // Ref for the stream scroll container
  let streamRef: HTMLDivElement | undefined;

  // Read chatTimeline directly — user events are inserted in-place by insertUserSteer.
  const blocks = createMemo<ChatEvent[]>(() => getAgent(props.agentId)?.chatTimeline ?? []);

  // Auto-scroll to bottom whenever blocks change.
  createEffect(() => {
    blocks();
    if (streamRef) {
      streamRef.scrollTop = streamRef.scrollHeight;
    }
  });

  const handleSend = async (priority: SteerPriority) => {
    const text = inputText().trim();
    if (!text || sending()) return;

    // Insert user message directly into the timeline before sending.
    insertUserSteer(props.agentId, text, priority);
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
          <div class={styles.emptyState}>
            <Show
              when={props.status === "running" || props.status === undefined}
              fallback={<span>No output</span>}
            >
              <span class={styles.runningDot} />
              <span>Agent is running…</span>
            </Show>
          </div>
        </Show>
        <For each={blocks()}>
          {(event) => (
            <Show
              when={event.kind === "user"}
              fallback={
                <Show
                  when={event.kind === "tool_call"}
                  fallback={
                    <Show
                      when={event.kind === "thinking"}
                      fallback={
                        <div
                          class={styles.outputBlock}
                          innerHTML={renderMarkdownText((event as Extract<ChatEvent, { kind: "text" }>).lines)}
                        />
                      }
                    >
                      <ThinkingBlock event={event as Extract<ChatEvent, { kind: "thinking" }>} />
                    </Show>
                  }
                >
                  <Show
                    when={
                      (event as Extract<ChatEvent, { kind: "tool_call" }>).toolName === "AskUserQuestion" &&
                      (event as Extract<ChatEvent, { kind: "tool_call" }>).awaitingAnswer &&
                      !(event as Extract<ChatEvent, { kind: "tool_call" }>).completedAt
                    }
                    fallback={
                      <ToolCallBlock event={event as Extract<ChatEvent, { kind: "tool_call" }>} />
                    }
                  >
                    <AskUserQuestionBlock
                      event={event as AskUserQuestionEvent}
                      agentId={props.agentId}
                    />
                  </Show>
                </Show>
              }
            >
              <div class={styles.userBlock}>
                <div class={`${styles.userBubble} ${(event as Extract<ChatEvent, { kind: "user" }>).priority === "urgent" ? styles.userBubbleUrgent : ""}`}>
                  {(event as Extract<ChatEvent, { kind: "user" }>).text}
                </div>
              </div>
            </Show>
          )}
        </For>
      </div>

      {/* Input bar */}
      <div class={styles.inputRow}>
        <Show when={props.status === "running"}>
          <button
            type="button"
            class={styles.stopBtn}
            onClick={() => {
              api.terminateAgent(props.agentId).catch((err: unknown) => {
                console.error("[AgentChat] Stop failed", err);
              });
            }}
            title="Stop agent"
          >
            ■ Stop
          </button>
        </Show>
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
