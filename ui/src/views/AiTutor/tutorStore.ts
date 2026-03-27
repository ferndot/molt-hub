/**
 * tutorStore — manages AI Tutor chat state per session.
 *
 * Holds message history keyed by session ID. Provides actions to send student
 * messages (via POST /api/tutor/sessions/:id/messages) and to append tutor
 * responses from the WebSocket output stream line-by-line.
 */

import { createStore, produce } from "solid-js/store";
import type { Suggestion } from "../../types/chat";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface TutorMessage {
  id: string;
  role: "student" | "tutor";
  content: string;
  timestamp: string; // ISO string
  suggestions?: Suggestion[]; // set when agent calls SuggestFollowups tool
}

interface TutorState {
  /** Messages keyed by sessionId. */
  messages: Record<string, TutorMessage[]>;
  /** Whether a send is in-flight, keyed by sessionId. */
  sending: Record<string, boolean>;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

const [state, setState] = createStore<TutorState>({
  messages: {},
  sending: {},
});

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

let counter = 0;

function genId(): string {
  counter += 1;
  return `tutor-${Date.now()}-${counter}`;
}

function nowIso(): string {
  return new Date().toISOString();
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/**
 * Send a student message to the tutor session. Adds the student message to
 * the history immediately (optimistic), then calls the backend API.
 */
export async function sendMessage(
  sessionId: string,
  content: string,
): Promise<void> {
  if (!content.trim()) return;

  const msg: TutorMessage = {
    id: genId(),
    role: "student",
    content: content.trim(),
    timestamp: nowIso(),
  };

  setState(
    produce((s) => {
      if (!s.messages[sessionId]) s.messages[sessionId] = [];
      s.messages[sessionId].push(msg);
      s.sending[sessionId] = true;
    }),
  );

  try {
    const res = await fetch(`/api/tutor/sessions/${sessionId}/messages`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message: content.trim() }),
    });
    if (!res.ok) {
      throw new Error(`POST failed: ${res.status}`);
    }
  } catch (err) {
    const errorMsg: TutorMessage = {
      id: genId(),
      role: "tutor",
      content: `[Error] Failed to send: ${err instanceof Error ? err.message : "Unknown error"}`,
      timestamp: nowIso(),
    };
    setState(
      produce((s) => {
        if (!s.messages[sessionId]) s.messages[sessionId] = [];
        s.messages[sessionId].push(errorMsg);
      }),
    );
  } finally {
    setState(
      produce((s) => {
        s.sending[sessionId] = false;
      }),
    );
  }
}

/**
 * Append a line to the last tutor message (streaming), or create a new tutor
 * message if the last message has role "student". Called line-by-line from WS output.
 */
export function addTutorLine(sessionId: string, line: string): void {
  setState(
    produce((s) => {
      if (!s.messages[sessionId]) s.messages[sessionId] = [];
      const msgs = s.messages[sessionId];
      const last = msgs[msgs.length - 1];
      if (last && last.role === "tutor") {
        // Append line to existing tutor message
        last.content = last.content + "\n" + line;
      } else {
        // Create a new tutor message
        msgs.push({
          id: genId(),
          role: "tutor",
          content: line,
          timestamp: nowIso(),
        });
      }
    }),
  );
}

/**
 * Set suggestions on a specific message by id. Called when the
 * SuggestFollowups tool call is parsed from WS output.
 */
export function attachSuggestions(
  sessionId: string,
  messageId: string,
  suggestions: Suggestion[],
): void {
  setState(
    produce((s) => {
      const msgs = s.messages[sessionId];
      if (!msgs) return;
      const msg = msgs.find((m) => m.id === messageId);
      if (msg) {
        msg.suggestions = suggestions;
      }
    }),
  );
}

// ---------------------------------------------------------------------------
// Selectors
// ---------------------------------------------------------------------------

/** Get messages for a specific session. */
export function getMessages(sessionId: string): TutorMessage[] {
  return state.messages[sessionId] ?? [];
}

/** Check if a message is currently being sent for a session. */
export function isSending(sessionId: string): boolean {
  return state.sending[sessionId] ?? false;
}

/** Clear all messages for a session. */
export function clearMessages(sessionId: string): void {
  setState(
    produce((s) => {
      s.messages[sessionId] = [];
    }),
  );
}

/** Read-only store access for testing/debugging. */
export function useTutorStore() {
  return { state };
}

/**
 * Reset the internal ID counter. Only used in tests.
 * @internal
 */
export function _resetCounter(): void {
  counter = 0;
}
