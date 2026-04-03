/**
 * Agent detail store — holds state for a single agent's detail view.
 *
 * Fetches real agent data from the backend API and subscribes
 * to WebSocket topic `agent:{id}` for live output.
 */

import { createStore, produce } from "solid-js/store";
import { subscribe } from "../../lib/ws";
import { api } from "../../lib/api";
import { addAgentMessage } from "./steerStore";
import type { AgentSummary } from "../../lib/api";
import type { Priority } from "../../types/domain";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ToolCallEntry {
  callId: string;
  toolName: string;
  input: unknown;
  output?: unknown;
  isError?: boolean;
  startedAt: string;
  completedAt?: string;
}

export type ChatEvent =
  | { kind: "text";      lines: string[]; timestamp: string }
  | { kind: "thinking";  lines: string[]; timestamp: string }
  | { kind: "tool_call"; callId: string; toolName: string; input: unknown; result?: unknown; isError?: boolean; startedAt: string; completedAt?: string; awaitingAnswer?: boolean }
  | { kind: "user";      text: string; priority: "normal" | "urgent"; timestamp: string };

export interface OutputLine {
  timestamp: string;
  text: string;
}

export interface FileDiff {
  path: string;
  unifiedDiff: string;
  timestamp: string;
}

export interface StageEntry {
  stage: string;
  enteredAt: string;
}

export interface AgentDetail {
  id: string;
  name: string;
  taskId: string;
  taskName: string;
  taskDescription: string;
  currentStage: string;
  stageHistory: StageEntry[];
  status: "running" | "paused" | "terminated" | "idle";
  priority: Priority;
  assignedAt: string;
  outputLines: OutputLine[];
  chatTimeline: ChatEvent[];
  fileDiffs: FileDiff[];
  toolCalls: ToolCallEntry[];
  authError?: string;
  inputTokens: number;
  outputTokens: number;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export interface AgentDetailState {
  agents: AgentDetail[];
  activeId: string | null;
}

const [state, setState] = createStore<AgentDetailState>({
  agents: [],
  activeId: null,
});

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

export function setActiveAgent(id: string): void {
  setState("activeId", id);
}

export function clearAuthError(agentId: string): void {
  setState(
    "agents",
    (a) => a.id === agentId,
    "authError",
    undefined,
  );
}

export function appendOutputLine(agentId: string, line: OutputLine): void {
  setState("agents", (agents) =>
    agents.map((a) =>
      a.id === agentId
        ? { ...a, outputLines: [...a.outputLines, line] }
        : a,
    ),
  );
}

export function insertUserSteer(agentId: string, text: string, priority: "normal" | "urgent"): void {
  setState(
    "agents",
    (a) => a.id === agentId,
    produce((a) => {
      a.chatTimeline.push({ kind: "user", text, priority, timestamp: new Date().toISOString() });
    }),
  );
}

// ---------------------------------------------------------------------------
// Selectors
// ---------------------------------------------------------------------------

export function getAgent(id: string): AgentDetail | undefined {
  return state.agents.find((a) => a.id === id);
}

/** Read-only store access. */
export function useAgentDetailStore() {
  return { state };
}

// ---------------------------------------------------------------------------
// WebSocket subscription (stub)
// ---------------------------------------------------------------------------

// Tracks which agent IDs already have an active WS subscription so that
// fetchAgents() does not set up duplicate listeners on every poll.
const subscribedAgentIds = new Set<string>();

// Per-agent state for timeline parsing (kept outside the store to avoid reactivity overhead)
const agentInThinking = new Set<string>();
const agentInToolResult = new Set<string>();

const ANSI_RE_WS = /\x1b\[[0-9;]*m/g;
function stripAnsiWs(s: string): string {
  return s.replace(ANSI_RE_WS, "");
}

const TOOL_CALL_LINE_RE_WS = /^[⏺●]\s+[\w:]+\(/;
const RESULT_START_RE_WS = /^[ \t]{0,3}⎿/;

/** Coalesce into last same-kind event or push new one. */
function coalesceOrAddToTimeline(
  agentId: string,
  kind: "text" | "thinking",
  line: string,
  timestamp: string,
): void {
  setState(
    "agents",
    (a) => a.id === agentId,
    produce((a) => {
      const last = a.chatTimeline[a.chatTimeline.length - 1];
      if (last && last.kind === kind) {
        (last as Extract<ChatEvent, { kind: "text" | "thinking" }>).lines.push(line);
      } else {
        a.chatTimeline.push({ kind, lines: [line], timestamp } as ChatEvent);
      }
    }),
  );
}

// ---------------------------------------------------------------------------
// API fetch — load real agents from the backend
// ---------------------------------------------------------------------------

function mapApiAgent(a: AgentSummary): AgentDetail {
  return {
    id: a.agent_id,
    name: a.name || a.agent_id.slice(0, 8),
    taskId: a.task_id ?? "",
    taskName: a.task_id ?? "",
    taskDescription: "",
    currentStage: a.status ?? "unknown",
    stageHistory: [],
    status: (a.status?.toLowerCase() as AgentDetail["status"]) ?? "idle",
    priority: "p2" as Priority,
    assignedAt: new Date().toISOString(),
    outputLines: [],
    chatTimeline: [],
    inputTokens: 0,
    outputTokens: 0,
    fileDiffs: [],
    toolCalls: [],
  };
}

/**
 * Fetch agents from the backend and merge into the store.
 * Preserves `outputLines` for agents still present so polling does not wipe the terminal buffer.
 */
export async function fetchAgents(): Promise<void> {
  try {
    const data = await api.getAgents();
    const agents = data.agents ?? [];
    setState("agents", (prev) => {
      if (agents.length === 0) return [];
      const prevById = new Map(prev.map((a) => [a.id, a]));
      return agents.map((raw) => {
        const m = mapApiAgent(raw);
        const old = prevById.get(m.id);
        if (old) {
          return {
            ...m,
            outputLines: old.outputLines.length > 0 ? old.outputLines : m.outputLines,
            chatTimeline: old.chatTimeline.length > 0 ? old.chatTimeline : m.chatTimeline,
            fileDiffs: old.fileDiffs.length > 0 ? old.fileDiffs : m.fileDiffs,
            toolCalls: old.toolCalls,
            authError: old.authError,
            inputTokens: old.inputTokens,
            outputTokens: old.outputTokens,
          };
        }
        return m;
      });
    });
    // Auto-subscribe to any newly seen agents (setupAgentSubscription is idempotent)
    for (const raw of agents) {
      setupAgentSubscription(raw.agent_id);
    }
  } catch {
    // API unreachable — keep existing state
  }
}

function formatOutputTimestamp(iso: string | undefined): string {
  if (!iso) {
    return new Date().toLocaleTimeString("en-US", {
      hour12: false,
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }
  const d = new Date(iso);
  return d.toLocaleTimeString("en-US", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** Ensure a row exists so detail/chat views can render before the next poll. */
export function registerAgentPlaceholder(
  id: string,
  opts?: { taskName?: string },
): void {
  if (state.agents.some((a) => a.id === id)) return;
  setState("agents", (agents) => [
    ...agents,
    {
      id,
      name: "Claude Code",
      taskId: "",
      taskName: opts?.taskName ?? "Project chat",
      taskDescription: "",
      currentStage: "running",
      stageHistory: [],
      status: "running",
      priority: "p2" as Priority,
      assignedAt: new Date().toISOString(),
      outputLines: [],
      chatTimeline: [],
      inputTokens: 0,
      outputTokens: 0,
      fileDiffs: [],
      toolCalls: [],
    },
  ]);
}

// ---------------------------------------------------------------------------
// Hydration helpers — rebuild chatTimeline from raw output lines
// ---------------------------------------------------------------------------

const ANSI_RE_HYDRATE = /\x1b\[[0-9;]*m/g;
function stripAnsiHydrate(s: string): string {
  return s.replace(ANSI_RE_HYDRATE, "");
}

const TOOL_CALL_LINE_RE_HYDRATE = /^[⏺●]\s+([\w:]+)\((.*)\)\s*$/;
const RESULT_START_RE_HYDRATE = /^[ \t]{0,3}⎿/;
const RESULT_CONT_RE_HYDRATE = /^[ \t]{5}/;

function buildChatTimeline(lines: OutputLine[]): ChatEvent[] {
  const timeline: ChatEvent[] = [];
  let inThinking = false;
  let inResult = false;

  const coalesceOrAdd = (kind: "text" | "thinking", line: string, timestamp: string) => {
    const last = timeline[timeline.length - 1];
    if (last && last.kind === kind) {
      (last as { kind: "text" | "thinking"; lines: string[]; timestamp: string }).lines.push(line);
    } else {
      timeline.push({ kind, lines: [line], timestamp } as ChatEvent);
    }
  };

  for (const ol of lines) {
    const raw = stripAnsiHydrate(ol.text);
    const ts = ol.timestamp;

    if (raw === "<thinking>") {
      inThinking = true;
      continue;
    }
    if (raw === "</thinking>") {
      inThinking = false;
      continue;
    }
    if (inThinking) {
      coalesceOrAdd("thinking", raw, ts);
      continue;
    }

    const toolMatch = raw.match(TOOL_CALL_LINE_RE_HYDRATE);
    if (toolMatch) {
      inResult = false;
      // Tool call line — push a tool_call event (hydrated, no structured callId)
      const idx = timeline.filter((e) => e.kind === "tool_call").length;
      timeline.push({
        kind: "tool_call",
        callId: `hydrated-${idx}`,
        toolName: toolMatch[1],
        input: toolMatch[2],
        startedAt: ts,
      });
      continue;
    }

    if (RESULT_START_RE_HYDRATE.test(raw)) {
      inResult = true;
      const resultText = raw.replace(RESULT_START_RE_HYDRATE, "").replace(/^\s{0,2}/, "");
      // Attach to last tool_call event
      const last = timeline[timeline.length - 1];
      if (last && last.kind === "tool_call") {
        const tc = last as Extract<ChatEvent, { kind: "tool_call" }>;
        tc.result = resultText;
        tc.completedAt = ts;
      }
      continue;
    }

    if (inResult && RESULT_CONT_RE_HYDRATE.test(raw)) {
      // Continuation of result — append to last tool_call result
      const last = timeline[timeline.length - 1];
      if (last && last.kind === "tool_call") {
        const tc = last as Extract<ChatEvent, { kind: "tool_call" }>;
        tc.result = (tc.result ? String(tc.result) + "\n" : "") + raw.replace(/^[ \t]{5}/, "");
      }
      continue;
    }

    // Regular text line
    inResult = false;
    coalesceOrAdd("text", raw, ts);
  }

  return timeline;
}

/** Replace buffered output from the server snapshot (e.g. after navigation or resume). */
export async function hydrateAgentOutput(agentId: string): Promise<void> {
  try {
    const res = await api.getAgentOutput(agentId);
    const lines = (res.lines ?? []) as Array<{ line?: string; timestamp?: string }>;
    const mapped: OutputLine[] = lines.map((l) => ({
      text: String(l.line ?? ""),
      timestamp: formatOutputTimestamp(l.timestamp),
    }));
    const authLine = mapped.findLast((l) => l.text.startsWith("auth_required:"));
    const authError = authLine ? authLine.text.replace(/^auth_required:\s*/, "") : undefined;
    const chatTimeline = buildChatTimeline(mapped);
    setState("agents", (agents) =>
      agents.map((a) => (a.id === agentId ? { ...a, outputLines: mapped, chatTimeline, authError } : a)),
    );
  } catch {
    /* ignore */
  }
}

export function removeAgentFromStore(agentId: string): void {
  setState("agents", (agents) => agents.filter((a) => a.id !== agentId));
  subscribedAgentIds.delete(agentId);
  agentInThinking.delete(agentId);
  agentInToolResult.delete(agentId);
}

/**
 * Start polling agents every `intervalMs` milliseconds.
 * Returns a cleanup function that stops the polling interval.
 */
export function startAgentPolling(intervalMs = 3000): () => void {
  // Initial fetch
  fetchAgents();
  const timer = setInterval(fetchAgents, intervalMs);
  return () => clearInterval(timer);
}

export function setupAgentSubscription(agentId: string): () => void {
  // Idempotent — skip if already subscribed (prevents duplicates when called
  // from both fetchAgents() and AgentDetailView mount).
  if (subscribedAgentIds.has(agentId)) {
    return () => {};
  }
  subscribedAgentIds.add(agentId);

  const topic = `agent:${agentId}`;
  const unsubscribe = subscribe(topic, (msg) => {
    if (msg.type !== "event") return;
    const payload = msg.payload as Record<string, unknown>;

    // Accumulate token counts from TurnEnd events.
    if (payload.type === "turn_end") {
      const inputTokens = (payload.input_tokens as number | undefined) ?? 0;
      const outputTokens = (payload.output_tokens as number | undefined) ?? 0;
      setState("agents", (agents) =>
        agents.map((a) =>
          a.id === agentId
            ? { ...a, inputTokens: a.inputTokens + inputTokens, outputTokens: a.outputTokens + outputTokens }
            : a,
        ),
      );
      return;
    }

    // Handle auth errors.
    if (payload.type === "agent_error") {
      const authRequired = payload.auth_required as boolean | undefined;
      const message = payload.message as string | undefined;
      setState(
        "agents",
        (a) => a.id === agentId,
        "authError",
        authRequired ? (message ?? "Authentication required") : undefined,
      );
      return;
    }

    // Handle file diffs emitted when an agent completes.
    if (payload.type === "file_diff") {
      const path = payload.path as string | undefined;
      const unifiedDiff = (payload.unified_diff ?? payload.unifiedDiff) as string | undefined;
      const timestamp = payload.timestamp as string | undefined;
      if (path && unifiedDiff) {
        setState(
          "agents",
          (a) => a.id === agentId,
          produce((a) => {
            a.fileDiffs.push({ path, unifiedDiff, timestamp: timestamp ?? new Date().toISOString() });
          }),
        );
      }
      return;
    }

    // Handle user_question events — mark the matching tool_call as awaiting an answer.
    if (payload.type === "user_question") {
      const callId = payload.call_id as string | undefined;
      if (callId) {
        setState(
          "agents",
          (a) => a.id === agentId,
          "chatTimeline",
          (ev) => ev.kind === "tool_call" && (ev as Extract<ChatEvent, { kind: "tool_call" }>).callId === callId,
          produce((ev) => {
            (ev as Extract<ChatEvent, { kind: "tool_call" }>).awaitingAnswer = true;
          }),
        );
      }
      return;
    }

    // Handle structured tool call events.
    if (payload.type === "tool_call") {
      const callId = payload.call_id as string | undefined;
      const toolName = (payload.tool_name as string | undefined) ?? "Tool";
      const input = payload.input;
      const timestamp = (payload.timestamp as string | undefined) ?? new Date().toISOString();
      if (callId) {
        setState(
          "agents",
          (a) => a.id === agentId,
          produce((a) => {
            a.toolCalls.push({ callId, toolName, input, startedAt: timestamp });
            a.chatTimeline.push({ kind: "tool_call", callId, toolName, input, startedAt: timestamp });
          }),
        );
      }
      return;
    }

    if (payload.type === "tool_result") {
      const callId = payload.call_id as string | undefined;
      const output = payload.output;
      const isError = payload.is_error as boolean | undefined;
      const timestamp = (payload.timestamp as string | undefined) ?? new Date().toISOString();
      if (callId) {
        // Update legacy toolCalls array
        setState(
          "agents",
          (a) => a.id === agentId,
          "toolCalls",
          (tc) => tc.callId === callId,
          produce((tc) => {
            tc.output = output;
            tc.isError = isError ?? false;
            tc.completedAt = timestamp;
          }),
        );
        // Update chatTimeline tool_call entry
        setState(
          "agents",
          (a) => a.id === agentId,
          "chatTimeline",
          (ev) => ev.kind === "tool_call" && (ev as Extract<ChatEvent, { kind: "tool_call" }>).callId === callId,
          produce((ev) => {
            const tc = ev as Extract<ChatEvent, { kind: "tool_call" }>;
            tc.result = output;
            tc.isError = isError ?? false;
            tc.completedAt = timestamp;
          }),
        );
      }
      return;
    }

    // Append agent output lines to the store.
    // Server sends { type: "agent_output", line: "...", timestamp: "..." }
    const output = (payload.line ?? payload.output) as string | undefined;
    const timestamp = payload.timestamp as string | undefined;
    if (output) {
      const ts = timestamp
        ? new Date(timestamp).toLocaleTimeString("en-US", {
            hour12: false,
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
          })
        : new Date().toLocaleTimeString("en-US", {
            hour12: false,
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
          });
      appendOutputLine(agentId, { timestamp: ts, text: output });
      addAgentMessage(agentId, output);

      // Also parse into chatTimeline
      const rawLine = stripAnsiWs(output);

      if (rawLine === "<thinking>") {
        agentInThinking.add(agentId);
        return;
      }
      if (rawLine === "</thinking>") {
        agentInThinking.delete(agentId);
        return;
      }
      if (agentInThinking.has(agentId)) {
        coalesceOrAddToTimeline(agentId, "thinking", rawLine, ts);
        return;
      }

      // Skip tool call text representation lines — structured events handle display
      if (TOOL_CALL_LINE_RE_WS.test(rawLine)) {
        agentInToolResult.delete(agentId);
        return;
      }

      if (RESULT_START_RE_WS.test(rawLine)) {
        agentInToolResult.add(agentId);
        return;
      }

      if (agentInToolResult.has(agentId) && /^[ \t]{5}/.test(rawLine)) {
        return;
      }

      // Regular text line
      agentInToolResult.delete(agentId);
      coalesceOrAddToTimeline(agentId, "text", rawLine, ts);
    }
  });
  return unsubscribe;
}
