/**
 * steerStore — manages steering chat state per agent.
 *
 * Holds message history keyed by agent ID. Provides actions to send human
 * messages (via POST /api/agents/:id/steer) and to append agent responses
 * from the WebSocket output stream.
 */

import { createStore, produce } from "solid-js/store";
import { api } from "../../lib/api";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type SteerPriority = "normal" | "urgent";

export interface SteerMessage {
  id: string;
  role: "human" | "agent";
  content: string;
  timestamp: string;
  priority?: SteerPriority;
}

export interface SteerState {
  /** Messages keyed by agent ID. */
  messages: Record<string, SteerMessage[]>;
  /** Whether a send is in-flight, keyed by agent ID. */
  sending: Record<string, boolean>;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

const [state, setState] = createStore<SteerState>({
  messages: {},
  sending: {},
});

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

let counter = 0;

function genId(): string {
  counter += 1;
  return `steer-${Date.now()}-${counter}`;
}

function nowIso(): string {
  return new Date().toISOString();
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/**
 * Send a steering message to an agent. Adds the human message to the
 * history immediately, then calls the backend API.
 */
export async function sendMessage(
  agentId: string,
  content: string,
  priority: SteerPriority = "normal",
): Promise<void> {
  if (!content.trim()) return;

  const msg: SteerMessage = {
    id: genId(),
    role: "human",
    content: content.trim(),
    timestamp: nowIso(),
    priority,
  };

  setState(
    produce((s) => {
      if (!s.messages[agentId]) s.messages[agentId] = [];
      s.messages[agentId].push(msg);
      s.sending[agentId] = true;
    }),
  );

  try {
    await api.steerAgent(agentId, msg.content, priority);
  } catch (err) {
    // Mark the message as failed by appending an error agent message
    const errorMsg: SteerMessage = {
      id: genId(),
      role: "agent",
      content: `[Error] Failed to send: ${err instanceof Error ? err.message : "Unknown error"}`,
      timestamp: nowIso(),
    };
    setState(
      produce((s) => {
        if (!s.messages[agentId]) s.messages[agentId] = [];
        s.messages[agentId].push(errorMsg);
      }),
    );
  } finally {
    setState(
      produce((s) => {
        s.sending[agentId] = false;
      }),
    );
  }
}

/**
 * Add an agent output message to the chat history for a given agent.
 * Called when output arrives via the WebSocket subscription.
 */
export function addAgentMessage(agentId: string, content: string): void {
  const msg: SteerMessage = {
    id: genId(),
    role: "agent",
    content,
    timestamp: nowIso(),
  };

  setState(
    produce((s) => {
      if (!s.messages[agentId]) s.messages[agentId] = [];
      s.messages[agentId].push(msg);
    }),
  );
}

// ---------------------------------------------------------------------------
// Selectors
// ---------------------------------------------------------------------------

/** Get messages for a specific agent. */
export function getMessages(agentId: string): SteerMessage[] {
  return state.messages[agentId] ?? [];
}

/** Check if a message is currently being sent for an agent. */
export function isSending(agentId: string): boolean {
  return state.sending[agentId] ?? false;
}

/** Clear all messages for an agent. */
export function clearMessages(agentId: string): void {
  setState(
    produce((s) => {
      s.messages[agentId] = [];
    }),
  );
}

/** Read-only store access for testing/debugging. */
export function useSteerStore() {
  return { state };
}

/**
 * Reset the internal ID counter. Only used in tests.
 * @internal
 */
export function _resetCounter(): void {
  counter = 0;
}
