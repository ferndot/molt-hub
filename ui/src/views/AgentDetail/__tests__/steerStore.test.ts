/**
 * Tests for steerStore — message management, API calls, and selectors.
 */
import { describe, it, expect, beforeEach, vi } from "vitest";

// Mock the api module before importing the store
vi.mock("../../../lib/api", () => ({
  api: {
    steerAgent: vi.fn().mockResolvedValue({}),
  },
}));

import {
  sendMessage,
  addAgentMessage,
  getMessages,
  isSending,
  clearMessages,
  useSteerStore,
  _resetCounter,
} from "../steerStore";
import { api } from "../../../lib/api";

const AGENT_ID = "agent-test-001";
const AGENT_ID_2 = "agent-test-002";

describe("steerStore", () => {
  beforeEach(() => {
    // Clear messages for both test agents between tests
    clearMessages(AGENT_ID);
    clearMessages(AGENT_ID_2);
    _resetCounter();
    vi.clearAllMocks();
  });

  // --------------------------------------------------------------------------
  // getMessages
  // --------------------------------------------------------------------------

  describe("getMessages", () => {
    it("returns empty array for unknown agent", () => {
      expect(getMessages("nonexistent-agent")).toEqual([]);
    });

    it("returns empty array after clearMessages", () => {
      addAgentMessage(AGENT_ID, "hello");
      clearMessages(AGENT_ID);
      expect(getMessages(AGENT_ID)).toEqual([]);
    });
  });

  // --------------------------------------------------------------------------
  // addAgentMessage
  // --------------------------------------------------------------------------

  describe("addAgentMessage", () => {
    it("appends an agent message with role 'agent'", () => {
      addAgentMessage(AGENT_ID, "Starting analysis...");

      const msgs = getMessages(AGENT_ID);
      expect(msgs).toHaveLength(1);
      expect(msgs[0].role).toBe("agent");
      expect(msgs[0].content).toBe("Starting analysis...");
      expect(msgs[0].timestamp).toBeTruthy();
      expect(msgs[0].id).toBeTruthy();
    });

    it("appends multiple messages in order", () => {
      addAgentMessage(AGENT_ID, "First");
      addAgentMessage(AGENT_ID, "Second");
      addAgentMessage(AGENT_ID, "Third");

      const msgs = getMessages(AGENT_ID);
      expect(msgs).toHaveLength(3);
      expect(msgs[0].content).toBe("First");
      expect(msgs[1].content).toBe("Second");
      expect(msgs[2].content).toBe("Third");
    });

    it("keeps messages separate per agent", () => {
      addAgentMessage(AGENT_ID, "Agent 1 message");
      addAgentMessage(AGENT_ID_2, "Agent 2 message");

      expect(getMessages(AGENT_ID)).toHaveLength(1);
      expect(getMessages(AGENT_ID_2)).toHaveLength(1);
      expect(getMessages(AGENT_ID)[0].content).toBe("Agent 1 message");
      expect(getMessages(AGENT_ID_2)[0].content).toBe("Agent 2 message");
    });
  });

  // --------------------------------------------------------------------------
  // sendMessage
  // --------------------------------------------------------------------------

  describe("sendMessage", () => {
    it("adds a human message and calls the API", async () => {
      await sendMessage(AGENT_ID, "Focus on the auth module");

      const msgs = getMessages(AGENT_ID);
      expect(msgs).toHaveLength(1);
      expect(msgs[0].role).toBe("human");
      expect(msgs[0].content).toBe("Focus on the auth module");
      expect(msgs[0].priority).toBe("normal");
      expect(api.steerAgent).toHaveBeenCalledWith(
        AGENT_ID,
        "Focus on the auth module",
        "normal",
      );
    });

    it("trims whitespace from content", async () => {
      await sendMessage(AGENT_ID, "  trimmed  ");

      const msgs = getMessages(AGENT_ID);
      expect(msgs[0].content).toBe("trimmed");
      expect(api.steerAgent).toHaveBeenCalledWith(AGENT_ID, "trimmed", "normal");
    });

    it("does nothing for empty or whitespace-only input", async () => {
      await sendMessage(AGENT_ID, "");
      await sendMessage(AGENT_ID, "   ");

      expect(getMessages(AGENT_ID)).toHaveLength(0);
      expect(api.steerAgent).not.toHaveBeenCalled();
    });

    it("sends with urgent priority when specified", async () => {
      await sendMessage(AGENT_ID, "Stop immediately", "urgent");

      const msgs = getMessages(AGENT_ID);
      expect(msgs[0].priority).toBe("urgent");
      expect(api.steerAgent).toHaveBeenCalledWith(
        AGENT_ID,
        "Stop immediately",
        "urgent",
      );
    });

    it("appends error message when API call fails", async () => {
      vi.mocked(api.steerAgent).mockRejectedValueOnce(new Error("Network error"));

      await sendMessage(AGENT_ID, "Will fail");

      const msgs = getMessages(AGENT_ID);
      expect(msgs).toHaveLength(2);
      expect(msgs[0].role).toBe("human");
      expect(msgs[0].content).toBe("Will fail");
      expect(msgs[1].role).toBe("agent");
      expect(msgs[1].content).toContain("[Error]");
      expect(msgs[1].content).toContain("Network error");
    });

    it("resets sending state after completion", async () => {
      // Before sending
      expect(isSending(AGENT_ID)).toBe(false);

      const promise = sendMessage(AGENT_ID, "test");
      // During send the flag may have toggled; await completion
      await promise;

      expect(isSending(AGENT_ID)).toBe(false);
    });

    it("resets sending state after failure", async () => {
      vi.mocked(api.steerAgent).mockRejectedValueOnce(new Error("fail"));

      await sendMessage(AGENT_ID, "test");

      expect(isSending(AGENT_ID)).toBe(false);
    });
  });

  // --------------------------------------------------------------------------
  // isSending
  // --------------------------------------------------------------------------

  describe("isSending", () => {
    it("returns false for unknown agent", () => {
      expect(isSending("nonexistent")).toBe(false);
    });
  });

  // --------------------------------------------------------------------------
  // clearMessages
  // --------------------------------------------------------------------------

  describe("clearMessages", () => {
    it("removes all messages for the specified agent", () => {
      addAgentMessage(AGENT_ID, "one");
      addAgentMessage(AGENT_ID, "two");
      expect(getMessages(AGENT_ID)).toHaveLength(2);

      clearMessages(AGENT_ID);
      expect(getMessages(AGENT_ID)).toHaveLength(0);
    });

    it("does not affect other agents", () => {
      addAgentMessage(AGENT_ID, "agent 1");
      addAgentMessage(AGENT_ID_2, "agent 2");

      clearMessages(AGENT_ID);

      expect(getMessages(AGENT_ID)).toHaveLength(0);
      expect(getMessages(AGENT_ID_2)).toHaveLength(1);
    });
  });

  // --------------------------------------------------------------------------
  // useSteerStore
  // --------------------------------------------------------------------------

  describe("useSteerStore", () => {
    it("exposes read-only state access", () => {
      const { state } = useSteerStore();
      expect(state).toBeDefined();
      expect(state.messages).toBeDefined();
      expect(state.sending).toBeDefined();
    });
  });

  // --------------------------------------------------------------------------
  // Message interleaving
  // --------------------------------------------------------------------------

  describe("interleaved conversation", () => {
    it("maintains correct order of human and agent messages", async () => {
      await sendMessage(AGENT_ID, "Run the tests");
      addAgentMessage(AGENT_ID, "Running tests...");
      addAgentMessage(AGENT_ID, "All 24 tests passed");
      await sendMessage(AGENT_ID, "Great, now deploy");

      const msgs = getMessages(AGENT_ID);
      expect(msgs).toHaveLength(4);
      expect(msgs[0].role).toBe("human");
      expect(msgs[0].content).toBe("Run the tests");
      expect(msgs[1].role).toBe("agent");
      expect(msgs[1].content).toBe("Running tests...");
      expect(msgs[2].role).toBe("agent");
      expect(msgs[2].content).toBe("All 24 tests passed");
      expect(msgs[3].role).toBe("human");
      expect(msgs[3].content).toBe("Great, now deploy");
    });
  });

  // --------------------------------------------------------------------------
  // Unique IDs
  // --------------------------------------------------------------------------

  describe("message IDs", () => {
    it("generates unique IDs for each message", () => {
      addAgentMessage(AGENT_ID, "first");
      addAgentMessage(AGENT_ID, "second");
      addAgentMessage(AGENT_ID, "third");

      const msgs = getMessages(AGENT_ID);
      const ids = msgs.map((m) => m.id);
      const uniqueIds = new Set(ids);
      expect(uniqueIds.size).toBe(3);
    });
  });
});
