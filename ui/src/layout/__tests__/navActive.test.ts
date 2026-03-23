/**
 * Tests for nav active-state logic.
 * Pure logic test — no DOM, no SolidJS rendering needed.
 */
import { describe, it, expect } from "vitest";

// Replicate the isActive logic from Sidebar.tsx so it can be tested in isolation
function isActive(currentPath: string, href: string): boolean {
  return currentPath === href || (href !== "/" && currentPath.startsWith(href));
}

describe("Sidebar nav active state", () => {
  it("exact path match is active", () => {
    expect(isActive("/triage", "/triage")).toBe(true);
    expect(isActive("/board", "/board")).toBe(true);
    expect(isActive("/agents", "/agents")).toBe(true);
  });

  it("non-matching path is not active", () => {
    expect(isActive("/board", "/triage")).toBe(false);
    expect(isActive("/triage", "/board")).toBe(false);
  });

  it("sub-path is active for parent nav item", () => {
    // /agents/:id should keep /agents nav item active
    expect(isActive("/agents/agent-001", "/agents")).toBe(true);
  });

  it("root path / does not match sub-routes", () => {
    // The root "/" link should only match exactly "/"
    expect(isActive("/triage", "/")).toBe(false);
    expect(isActive("/board", "/")).toBe(false);
  });

  it("active route at / matches exactly", () => {
    expect(isActive("/", "/")).toBe(true);
  });
});
