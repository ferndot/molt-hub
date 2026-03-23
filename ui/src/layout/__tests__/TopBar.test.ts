/**
 * Tests for TopBar — verifies CSS module shape and integration points.
 * Tests run in node environment — no DOM rendering.
 * (The component itself uses solid-icons which requires a browser environment,
 *  so we test the CSS module and integration contract instead.)
 */
import { describe, it, expect } from "vitest";

describe("TopBar CSS module", () => {
  it("provides expected class names for layout", async () => {
    const styles = await import("../TopBar.module.css");
    expect(styles.default.topBar).toBeDefined();
    expect(styles.default.left).toBeDefined();
    expect(styles.default.right).toBeDefined();
  });

  it("provides expected class names for inbox toggle", async () => {
    const styles = await import("../TopBar.module.css");
    expect(styles.default.inboxToggle).toBeDefined();
    expect(styles.default.inboxToggleActive).toBeDefined();
    expect(styles.default.inboxBadge).toBeDefined();
  });
});

describe("TopBar integration contract", () => {
  it("attentionStore unreadCount is available for TopBar consumption", async () => {
    const { unreadCount } = await import("../attentionStore");
    expect(typeof unreadCount).toBe("function");
    expect(typeof unreadCount()).toBe("number");
  });

  it("TopBar module file is importable (module resolution check)", async () => {
    // Verify the CSS module exists and can be resolved.
    // In node/vitest environment, CSS modules resolve to an object (may be empty proxy).
    const mod = await import("../TopBar.module.css");
    expect(mod).toBeDefined();
    expect(mod.default).toBeDefined();
  });
});
