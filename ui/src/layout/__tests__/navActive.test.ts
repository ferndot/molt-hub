/**
 * Tests for nav active-state logic.
 * Pure logic test — no DOM, no SolidJS rendering needed.
 */
import { describe, it, expect } from "vitest";

// Replicate the isActive logic from Sidebar.tsx so it can be tested in isolation
function isActive(currentPath: string, href: string): boolean {
  if (href === "/chat") return currentPath === "/chat";
  if (href === "/") return currentPath === "/";
  return currentPath.startsWith(href);
}

describe("Sidebar nav active state", () => {
  it("exact path match is active", () => {
    expect(isActive("/triage", "/triage")).toBe(true);
    expect(isActive("/boards", "/boards")).toBe(true);
    expect(isActive("/agents", "/agents")).toBe(true);
  });

  it("non-matching path is not active", () => {
    expect(isActive("/boards/default", "/triage")).toBe(false);
    expect(isActive("/triage", "/boards")).toBe(false);
  });

  it("sub-path is active for parent nav item", () => {
    expect(isActive("/boards/default", "/boards")).toBe(true);
    // /agents/:id should keep /agents nav item active
    expect(isActive("/agents/agent-001", "/agents")).toBe(true);
  });

  it("root path / does not match sub-routes", () => {
    // The root "/" link should only match exactly "/"
    expect(isActive("/triage", "/")).toBe(false);
    expect(isActive("/boards/default", "/")).toBe(false);
  });

  it("active route at / matches exactly", () => {
    expect(isActive("/", "/")).toBe(true);
  });

  it("/chat is active only for exact path", () => {
    expect(isActive("/chat", "/chat")).toBe(true);
    expect(isActive("/chats", "/chat")).toBe(false);
    expect(isActive("/chat/extra", "/chat")).toBe(false);
  });
});
