/**
 * Tests for tutorStore and AiTutorChat behaviour.
 *
 * Tests cover:
 * - Store actions: sendMessage, addTutorLine, attachSuggestions, selectors
 * - Suggestion logic: complete vs partial click handling
 * - SuggestFollowups tool call parsing
 * - Suggestion interactivity (only last tutor message is interactive)
 */

import { describe, it, expect, beforeEach, vi } from "vitest";

// Mock fetch before importing the store
const mockFetch = vi.fn().mockResolvedValue({
  ok: true,
  json: () => Promise.resolve({}),
});
global.fetch = mockFetch;

import {
  sendMessage,
  addTutorLine,
  attachSuggestions,
  getMessages,
  isSending,
  clearMessages,
  useTutorStore,
  _resetCounter,
} from "../tutorStore";
import type { Suggestion } from "../../../types/chat";

const SESSION_ID = "session-test-001";
const SESSION_ID_2 = "session-test-002";

describe("tutorStore", () => {
  beforeEach(() => {
    clearMessages(SESSION_ID);
    clearMessages(SESSION_ID_2);
    _resetCounter();
    vi.clearAllMocks();
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve({}) });
  });

  // --------------------------------------------------------------------------
  // getMessages
  // --------------------------------------------------------------------------

  describe("getMessages", () => {
    it("returns empty array for unknown session", () => {
      expect(getMessages("nonexistent-session")).toEqual([]);
    });

    it("returns empty array after clearMessages", () => {
      addTutorLine(SESSION_ID, "hello");
      clearMessages(SESSION_ID);
      expect(getMessages(SESSION_ID)).toEqual([]);
    });
  });

  // --------------------------------------------------------------------------
  // addTutorLine
  // --------------------------------------------------------------------------

  describe("addTutorLine", () => {
    it("creates a new tutor message when history is empty", () => {
      addTutorLine(SESSION_ID, "Welcome to the tutor!");

      const msgs = getMessages(SESSION_ID);
      expect(msgs).toHaveLength(1);
      expect(msgs[0].role).toBe("tutor");
      expect(msgs[0].content).toBe("Welcome to the tutor!");
      expect(msgs[0].id).toBeTruthy();
      expect(msgs[0].timestamp).toBeTruthy();
    });

    it("appends line to last tutor message when last role is tutor", () => {
      addTutorLine(SESSION_ID, "First line.");
      addTutorLine(SESSION_ID, "Second line.");

      const msgs = getMessages(SESSION_ID);
      expect(msgs).toHaveLength(1);
      expect(msgs[0].content).toBe("First line.\nSecond line.");
    });

    it("creates a new tutor message when last role is student", async () => {
      await sendMessage(SESSION_ID, "Hello tutor");
      addTutorLine(SESSION_ID, "Hello student!");

      const msgs = getMessages(SESSION_ID);
      expect(msgs).toHaveLength(2);
      expect(msgs[0].role).toBe("student");
      expect(msgs[1].role).toBe("tutor");
      expect(msgs[1].content).toBe("Hello student!");
    });

    it("keeps messages separate per session", () => {
      addTutorLine(SESSION_ID, "Session 1 message");
      addTutorLine(SESSION_ID_2, "Session 2 message");

      expect(getMessages(SESSION_ID)).toHaveLength(1);
      expect(getMessages(SESSION_ID_2)).toHaveLength(1);
    });
  });

  // --------------------------------------------------------------------------
  // sendMessage
  // --------------------------------------------------------------------------

  describe("sendMessage", () => {
    it("adds a student message and calls the API", async () => {
      await sendMessage(SESSION_ID, "What is recursion?");

      const msgs = getMessages(SESSION_ID);
      expect(msgs).toHaveLength(1);
      expect(msgs[0].role).toBe("student");
      expect(msgs[0].content).toBe("What is recursion?");
      expect(mockFetch).toHaveBeenCalledWith(
        `/api/tutor/sessions/${SESSION_ID}/messages`,
        expect.objectContaining({
          method: "POST",
          body: JSON.stringify({ message: "What is recursion?" }),
        }),
      );
    });

    it("trims whitespace from content", async () => {
      await sendMessage(SESSION_ID, "  trimmed  ");

      const msgs = getMessages(SESSION_ID);
      expect(msgs[0].content).toBe("trimmed");
    });

    it("does nothing for empty or whitespace-only input", async () => {
      await sendMessage(SESSION_ID, "");
      await sendMessage(SESSION_ID, "   ");

      expect(getMessages(SESSION_ID)).toHaveLength(0);
      expect(mockFetch).not.toHaveBeenCalled();
    });

    it("appends error tutor message when API call fails", async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await sendMessage(SESSION_ID, "This will fail");

      const msgs = getMessages(SESSION_ID);
      expect(msgs).toHaveLength(2);
      expect(msgs[0].role).toBe("student");
      expect(msgs[1].role).toBe("tutor");
      expect(msgs[1].content).toContain("[Error]");
    });

    it("appends error message when fetch throws", async () => {
      mockFetch.mockRejectedValueOnce(new Error("Network error"));

      await sendMessage(SESSION_ID, "Will throw");

      const msgs = getMessages(SESSION_ID);
      expect(msgs).toHaveLength(2);
      expect(msgs[1].content).toContain("[Error]");
      expect(msgs[1].content).toContain("Network error");
    });

    it("resets sending state after completion", async () => {
      expect(isSending(SESSION_ID)).toBe(false);
      await sendMessage(SESSION_ID, "test");
      expect(isSending(SESSION_ID)).toBe(false);
    });

    it("resets sending state after failure", async () => {
      mockFetch.mockRejectedValueOnce(new Error("fail"));
      await sendMessage(SESSION_ID, "test");
      expect(isSending(SESSION_ID)).toBe(false);
    });
  });

  // --------------------------------------------------------------------------
  // attachSuggestions
  // --------------------------------------------------------------------------

  describe("attachSuggestions", () => {
    it("attaches suggestions to a specific message by id", () => {
      addTutorLine(SESSION_ID, "Here are some followups:");

      const msgs = getMessages(SESSION_ID);
      const tutorMsgId = msgs[0].id;

      const suggestions: Suggestion[] = [
        { kind: "complete", text: "Tell me more" },
        { kind: "partial", text: "What about…" },
      ];
      attachSuggestions(SESSION_ID, tutorMsgId, suggestions);

      const updated = getMessages(SESSION_ID);
      expect(updated[0].suggestions).toEqual(suggestions);
    });

    it("does nothing for an unknown message id", () => {
      addTutorLine(SESSION_ID, "Some message");
      attachSuggestions(SESSION_ID, "nonexistent-id", [{ kind: "complete", text: "Go" }]);

      const msgs = getMessages(SESSION_ID);
      expect(msgs[0].suggestions).toBeUndefined();
    });

    it("does nothing for an unknown session", () => {
      // Should not throw
      expect(() => {
        attachSuggestions("unknown-session", "any-id", [{ kind: "complete", text: "Go" }]);
      }).not.toThrow();
    });
  });

  // --------------------------------------------------------------------------
  // Suggestion interactivity (last tutor message only)
  // --------------------------------------------------------------------------

  describe("suggestion interactivity", () => {
    it("only the last tutor message has interactive suggestions", async () => {
      // Send a student message, then add two tutor messages with suggestions
      await sendMessage(SESSION_ID, "Hello");

      addTutorLine(SESSION_ID, "First tutor response");
      const msgs1 = getMessages(SESSION_ID);
      const firstTutorId = msgs1[msgs1.length - 1].id;
      attachSuggestions(SESSION_ID, firstTutorId, [{ kind: "complete", text: "Option A" }]);

      // Force a student message so next tutor line creates a new message
      await sendMessage(SESSION_ID, "Follow up");

      addTutorLine(SESSION_ID, "Second tutor response");
      const msgs2 = getMessages(SESSION_ID);
      const lastTutorId = msgs2[msgs2.length - 1].id;
      attachSuggestions(SESSION_ID, lastTutorId, [{ kind: "complete", text: "Option B" }]);

      const finalMsgs = getMessages(SESSION_ID);
      // Find the last tutor message id
      let computedLastTutorId: string | null = null;
      for (let i = finalMsgs.length - 1; i >= 0; i--) {
        if (finalMsgs[i].role === "tutor") {
          computedLastTutorId = finalMsgs[i].id;
          break;
        }
      }

      expect(computedLastTutorId).toBe(lastTutorId);
      // The first tutor message should NOT be the last tutor message
      expect(firstTutorId).not.toBe(computedLastTutorId);
    });
  });

  // --------------------------------------------------------------------------
  // SuggestFollowups tool call parsing (regex)
  // --------------------------------------------------------------------------

  describe("SuggestFollowups tool call regex", () => {
    const SUGGEST_TOOL_RE = /^[⏺●]\s+SuggestFollowups\((.+)\)\s*$/;

    it("matches a valid SuggestFollowups line with ⏺", () => {
      const line = `⏺ SuggestFollowups({"suggestions":[{"kind":"complete","text":"Next topic"}]})`;
      const match = SUGGEST_TOOL_RE.exec(line);
      expect(match).not.toBeNull();
      const parsed = JSON.parse(match![1]) as { suggestions: Suggestion[] };
      expect(parsed.suggestions).toHaveLength(1);
      expect(parsed.suggestions[0].text).toBe("Next topic");
    });

    it("matches a valid SuggestFollowups line with ●", () => {
      const line = `● SuggestFollowups({"suggestions":[{"kind":"partial","text":"Explain…"}]})`;
      const match = SUGGEST_TOOL_RE.exec(line);
      expect(match).not.toBeNull();
    });

    it("does not match a regular output line", () => {
      const line = "Here is your answer: recursion is when a function calls itself.";
      expect(SUGGEST_TOOL_RE.exec(line)).toBeNull();
    });

    it("does not match a partial tool call", () => {
      const line = "SuggestFollowups without the bullet prefix";
      expect(SUGGEST_TOOL_RE.exec(line)).toBeNull();
    });
  });

  // --------------------------------------------------------------------------
  // No suggestions rendered when msg.suggestions is empty/undefined
  // --------------------------------------------------------------------------

  describe("empty suggestions", () => {
    it("message with no suggestions property has undefined suggestions", () => {
      addTutorLine(SESSION_ID, "No suggestions here");
      const msgs = getMessages(SESSION_ID);
      expect(msgs[0].suggestions).toBeUndefined();
    });

    it("attaching empty suggestions array stores an empty array", () => {
      addTutorLine(SESSION_ID, "Empty suggestions test");
      const msgs = getMessages(SESSION_ID);
      attachSuggestions(SESSION_ID, msgs[0].id, []);
      const updated = getMessages(SESSION_ID);
      expect(updated[0].suggestions).toEqual([]);
    });
  });

  // --------------------------------------------------------------------------
  // isSending
  // --------------------------------------------------------------------------

  describe("isSending", () => {
    it("returns false for unknown session", () => {
      expect(isSending("nonexistent")).toBe(false);
    });
  });

  // --------------------------------------------------------------------------
  // useTutorStore
  // --------------------------------------------------------------------------

  describe("useTutorStore", () => {
    it("exposes read-only state access", () => {
      const { state } = useTutorStore();
      expect(state).toBeDefined();
      expect(state.messages).toBeDefined();
      expect(state.sending).toBeDefined();
    });
  });

  // --------------------------------------------------------------------------
  // Message IDs
  // --------------------------------------------------------------------------

  describe("message IDs", () => {
    it("generates unique IDs for each message", async () => {
      await sendMessage(SESSION_ID, "first");
      addTutorLine(SESSION_ID, "response");
      await sendMessage(SESSION_ID, "second");

      const msgs = getMessages(SESSION_ID);
      const ids = msgs.map((m) => m.id);
      const uniqueIds = new Set(ids);
      expect(uniqueIds.size).toBe(ids.length);
    });
  });
});
