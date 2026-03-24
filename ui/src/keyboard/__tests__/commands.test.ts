/**
 * Tests for command palette filtering.
 * Runs in node environment — no DOM.
 */

import { describe, it, expect } from "vitest";
import { filterCommands, COMMANDS } from "../commands";

describe("filterCommands", () => {
  describe("empty query", () => {
    it("returns all commands when query is empty string", () => {
      const results = filterCommands("");
      expect(results).toHaveLength(COMMANDS.length);
    });

    it("returns all commands when query is whitespace only", () => {
      const results = filterCommands("   ");
      expect(results).toHaveLength(COMMANDS.length);
    });
  });

  describe("label matching", () => {
    it("finds 'Go to Triage' by label substring", () => {
      const results = filterCommands("triage");
      const labels = results.map((c) => c.label);
      expect(labels).toContain("Go to Triage");
    });

    it("finds workboard command by label substring 'board'", () => {
      const results = filterCommands("board");
      expect(results.some((c) => c.id === "goto-board")).toBe(true);
    });

    it("is case-insensitive", () => {
      const lower = filterCommands("triage");
      const upper = filterCommands("TRIAGE");
      expect(lower.map((c) => c.id)).toEqual(upper.map((c) => c.id));
    });
  });

  describe("keyword matching", () => {
    it("finds commands by keyword 'kanban'", () => {
      const results = filterCommands("kanban");
      expect(results.some((c) => c.id === "goto-board")).toBe(true);
    });

    it("finds approve command by keyword 'accept'", () => {
      const results = filterCommands("accept");
      expect(results.some((c) => c.id === "approve-item")).toBe(true);
    });

    it("finds reject command by keyword 'deny'", () => {
      const results = filterCommands("deny");
      expect(results.some((c) => c.id === "reject-item")).toBe(true);
    });

    it("finds help command by keyword 'shortcuts'", () => {
      const results = filterCommands("shortcuts");
      expect(results.some((c) => c.id === "show-help")).toBe(true);
    });
  });

  describe("description matching", () => {
    it("finds commands by description text", () => {
      const results = filterCommands("agent list");
      expect(results.some((c) => c.id === "goto-agents")).toBe(true);
    });
  });

  describe("no match", () => {
    it("returns empty array when no commands match", () => {
      const results = filterCommands("xyzzy-nonexistent-query-12345");
      expect(results).toHaveLength(0);
    });
  });

  describe("command structure", () => {
    it("all commands have required fields", () => {
      for (const cmd of COMMANDS) {
        expect(typeof cmd.id).toBe("string");
        expect(typeof cmd.label).toBe("string");
        expect(["navigation", "action"]).toContain(cmd.category);
      }
    });

    it("navigation commands are a subset of all commands", () => {
      const navCmds = COMMANDS.filter((c) => c.category === "navigation");
      expect(navCmds.length).toBeGreaterThan(0);
    });
  });
});
