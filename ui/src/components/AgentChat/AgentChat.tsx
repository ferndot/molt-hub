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
import { getAgent } from "../../views/AgentDetail/agentStore";
import type { OutputLine } from "../../views/AgentDetail/agentStore";
import { sendMessage } from "../../views/AgentDetail/steerStore";
import type { SteerPriority } from "../../views/AgentDetail/steerStore";
import styles from "./AgentChat.module.css";

// Configure marked for GitHub-flavored markdown (gfm is the default in v5+)
marked.use({ breaks: true });

// ---------------------------------------------------------------------------
// Tool call parsing
// ---------------------------------------------------------------------------
// Claude Code emits tool calls using Unicode bullets:
//   ⏺ ToolName(args)        ← tool invocation  (U+23FA)
//     ⎿  result line 1      ← result start      (U+23BF, 2-space indent)
//        result line 2      ← result cont.      (5-space indent)
//
// We also accept ● (U+25CF) as an alternate invocation bullet.

const TOOL_CALL_RE = /^[⏺●]\s+([\w:]+)\((.*)\)\s*$/;
const RESULT_START_RE = /^[ \t]{0,3}⎿\s{0,2}/;
const RESULT_CONT_RE = /^[ \t]{5}/;

// Strip ANSI escape sequences that Claude Code may emit
const ANSI_RE = /\x1b\[[0-9;]*m/g;
function stripAnsi(s: string): string {
  return s.replace(ANSI_RE, "");
}

interface TextSegment   { kind: "text";     lines: string[] }
interface ToolCallSegment {
  kind:   "toolcall";
  name:   string;
  args:   string;
  result: string[];
}
type OutputSegment = TextSegment | ToolCallSegment;

function parseOutputSegments(lines: OutputLine[]): OutputSegment[] {
  const segments: OutputSegment[] = [];
  let textBuf: string[] = [];
  let i = 0;

  const flushText = () => {
    if (textBuf.length > 0) {
      segments.push({ kind: "text", lines: textBuf });
      textBuf = [];
    }
  };

  while (i < lines.length) {
    const raw = stripAnsi(lines[i].text);
    const toolMatch = raw.match(TOOL_CALL_RE);

    if (toolMatch) {
      flushText();
      const name   = toolMatch[1];
      const args   = toolMatch[2];
      const result: string[] = [];
      i++;

      // Collect result lines
      while (i < lines.length) {
        const rRaw = stripAnsi(lines[i].text);
        if (RESULT_START_RE.test(rRaw)) {
          result.push(rRaw.replace(RESULT_START_RE, ""));
          i++;
        } else if (result.length > 0 && RESULT_CONT_RE.test(rRaw)) {
          // Continuation — strip the leading 5 spaces
          result.push(rRaw.replace(/^[ \t]{5}/, ""));
          i++;
        } else {
          break;
        }
      }

      segments.push({ kind: "toolcall", name, args, result });
    } else {
      textBuf.push(raw);
      i++;
    }
  }

  flushText();
  return segments;
}

// ---------------------------------------------------------------------------
// Markdown renderer (plain text segments)
// ---------------------------------------------------------------------------

function renderMarkdownText(lines: string[]): string {
  const raw  = lines.join("\n");
  const html = marked.parse(raw) as string;
  return DOMPurify.sanitize(html);
}

// ---------------------------------------------------------------------------
// OutputBlock — renders an output chunk (text + tool calls interleaved)
// ---------------------------------------------------------------------------

const OutputBlock: Component<{ lines: OutputLine[] }> = (props) => {
  const segments = createMemo(() => parseOutputSegments(props.lines));

  return (
    <For each={segments()}>
      {(seg) => (
        <Show
          when={seg.kind === "toolcall"}
          fallback={
            <div
              class={styles.outputBlock}
              innerHTML={renderMarkdownText((seg as TextSegment).lines)}
            />
          }
        >
          <details class={styles.toolCall}>
            <summary class={styles.toolCallSummary}>
              <span class={styles.toolCallBullet}>⏺</span>
              <span class={styles.toolCallName}>{(seg as ToolCallSegment).name}</span>
              <Show when={(seg as ToolCallSegment).args}>
                <span class={styles.toolCallArgs}>({(seg as ToolCallSegment).args})</span>
              </Show>
            </summary>
            <Show when={(seg as ToolCallSegment).result.join("\n").trim()}>
              <div class={styles.toolCallResult}>
                <pre class={styles.toolCallPre}><code>{(seg as ToolCallSegment).result.join("\n")}</code></pre>
              </div>
            </Show>
          </details>
        </Show>
      )}
    </For>
  );
};

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
  | { kind: "user";   text: string; priority: SteerPriority };

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
  // Steer insertions tracked locally — not persisted.
  const [steerInsertions, setSteerInsertions] = createSignal<SteerInsertion[]>([]);

  // Input state
  const [inputText, setInputText] = createSignal("");
  const [sending, setSending] = createSignal(false);

  // Thinking indicator — true from when the user's steer is sent until new agent output arrives.
  const [waitingForResponse, setWaitingForResponse] = createSignal(false);
  const [waitingAfterLine, setWaitingAfterLine] = createSignal(0);

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
    blocks();
    if (streamRef) {
      streamRef.scrollTop = streamRef.scrollHeight;
    }
  });

  // Clear thinking indicator when new output arrives after the steer point.
  createEffect(() => {
    const lines = getAgent(props.agentId)?.outputLines ?? [];
    if (waitingForResponse() && lines.length > waitingAfterLine()) {
      setWaitingForResponse(false);
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
      // Show the thinking indicator from this line index onwards.
      setWaitingAfterLine(atLineIndex);
      setWaitingForResponse(true);
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
          {(block) => (
            <Show
              when={block.kind === "user"}
              fallback={
                <OutputBlock lines={(block as { kind: "output"; lines: OutputLine[] }).lines} />
              }
            >
              <div class={styles.userBlock}>
                <div class={`${styles.userBubble} ${(block as { kind: "user"; text: string; priority: SteerPriority }).priority === "urgent" ? styles.userBubbleUrgent : ""}`}>
                  {(block as { kind: "user"; text: string; priority: SteerPriority }).text}
                </div>
              </div>
            </Show>
          )}
        </For>
        <Show when={waitingForResponse()}>
          <div class={styles.thinkingRow}>
            <span class={styles.thinkingDot} />
            <span class={styles.thinkingDot} />
            <span class={styles.thinkingDot} />
          </div>
        </Show>
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
