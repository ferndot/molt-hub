/**
 * Tests for settingsStore — pure action logic and derived helpers.
 *
 * The store is a SolidJS singleton; tests mutate it directly and restore
 * state at the end of each block (same pattern as boardStore.test.ts).
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  getSortedColumns,
  getStagesForColumn,
  parseHookIds,
  serializeHookIds,
  DEFAULT_KANBAN_COLUMNS,
  loadPersistedSettings,
  persistSettings,
  STORAGE_KEY,
  NAV_SIDEBAR_MIN,
  NAV_SIDEBAR_MAX,
  TRIAGE_SIDEBAR_MIN,
  TRIAGE_SIDEBAR_MAX,
} from "../settingsStore";
import type { KanbanColumn, Theme, SettingsState } from "../settingsStore";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeColumn(overrides: Partial<KanbanColumn> & { id: string }): KanbanColumn {
  return {
    title: `Column ${overrides.id}`,
    stageMatch: overrides.id,
    color: "#000",
    order: 0,
    behavior: {
      wipLimit: null,
      autoAssign: false,
      autoTransition: null,
      requireApproval: false,
    },
    hooks: {
      onEnter: [],
      onExit: [],
      onStall: [],
    },
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// getSortedColumns
// ---------------------------------------------------------------------------

describe("getSortedColumns", () => {
  it("returns columns sorted by order ascending", () => {
    const cols: KanbanColumn[] = [
      makeColumn({ id: "c", order: 2 }),
      makeColumn({ id: "a", order: 0 }),
      makeColumn({ id: "b", order: 1 }),
    ];
    const sorted = getSortedColumns(cols);
    expect(sorted.map((c) => c.id)).toEqual(["a", "b", "c"]);
  });

  it("does not mutate the original array", () => {
    const cols: KanbanColumn[] = [
      makeColumn({ id: "z", order: 10 }),
      makeColumn({ id: "a", order: 0 }),
    ];
    const original = cols.map((c) => c.id);
    getSortedColumns(cols);
    expect(cols.map((c) => c.id)).toEqual(original);
  });

  it("handles a single column", () => {
    const cols = [makeColumn({ id: "only", order: 0 })];
    expect(getSortedColumns(cols)).toHaveLength(1);
  });

  it("handles an empty array", () => {
    expect(getSortedColumns([])).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// getStagesForColumn
// ---------------------------------------------------------------------------

describe("getStagesForColumn", () => {
  it("returns a single stage when stageMatch has no delimiter", () => {
    const col = makeColumn({ id: "x", stageMatch: "backlog" });
    expect(getStagesForColumn(col)).toEqual(["backlog"]);
  });

  it("splits comma-separated stage names", () => {
    const col = makeColumn({ id: "x", stageMatch: "in-progress,code-review" });
    expect(getStagesForColumn(col)).toEqual(["in-progress", "code-review"]);
  });

  it("splits space-separated stage names", () => {
    const col = makeColumn({ id: "x", stageMatch: "testing deployed" });
    expect(getStagesForColumn(col)).toEqual(["testing", "deployed"]);
  });

  it("trims extra whitespace and commas", () => {
    const col = makeColumn({ id: "x", stageMatch: " backlog , in-progress " });
    expect(getStagesForColumn(col)).toEqual(["backlog", "in-progress"]);
  });

  it("returns empty array for empty stageMatch", () => {
    const col = makeColumn({ id: "x", stageMatch: "" });
    expect(getStagesForColumn(col)).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// parseHookIds / serializeHookIds
// ---------------------------------------------------------------------------

describe("parseHookIds", () => {
  it("splits comma-separated hook IDs", () => {
    expect(parseHookIds("hook-a, hook-b, hook-c")).toEqual(["hook-a", "hook-b", "hook-c"]);
  });

  it("splits space-separated hook IDs", () => {
    expect(parseHookIds("hook-a hook-b")).toEqual(["hook-a", "hook-b"]);
  });

  it("returns empty array for empty string", () => {
    expect(parseHookIds("")).toEqual([]);
  });

  it("trims whitespace", () => {
    expect(parseHookIds("  hook-a  ,  hook-b  ")).toEqual(["hook-a", "hook-b"]);
  });
});

describe("serializeHookIds", () => {
  it("joins IDs with comma-space", () => {
    expect(serializeHookIds(["hook-a", "hook-b"])).toBe("hook-a, hook-b");
  });

  it("returns empty string for empty array", () => {
    expect(serializeHookIds([])).toBe("");
  });
});

// ---------------------------------------------------------------------------
// DEFAULT_KANBAN_COLUMNS
// ---------------------------------------------------------------------------

describe("DEFAULT_KANBAN_COLUMNS", () => {
  it("contains exactly 5 default columns", () => {
    expect(DEFAULT_KANBAN_COLUMNS).toHaveLength(5);
  });

  it("has unique ids", () => {
    const ids = DEFAULT_KANBAN_COLUMNS.map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("covers the expected stages", () => {
    const allStages = DEFAULT_KANBAN_COLUMNS.flatMap(getStagesForColumn);
    expect(allStages).toContain("backlog");
    expect(allStages).toContain("in-progress");
    expect(allStages).toContain("code-review");
    expect(allStages).toContain("testing");
    expect(allStages).toContain("deployed");
  });

  it("orders are unique and sequential starting at 0", () => {
    const orders = DEFAULT_KANBAN_COLUMNS.map((c) => c.order).sort((a, b) => a - b);
    expect(orders).toEqual([0, 1, 2, 3, 4]);
  });

  it("each column has behavior and hooks fields", () => {
    for (const col of DEFAULT_KANBAN_COLUMNS) {
      expect(col.behavior).toBeDefined();
      expect(col.hooks).toBeDefined();
      expect(col.hooks.onEnter).toBeInstanceOf(Array);
      expect(col.hooks.onExit).toBeInstanceOf(Array);
      expect(col.hooks.onStall).toBeInstanceOf(Array);
    }
  });
});

// ---------------------------------------------------------------------------
// Store actions (via dynamic import to avoid isolation issues)
// ---------------------------------------------------------------------------

describe("settingsStore actions", () => {
  describe("setJiraConfig", () => {
    it("updates the jira baseUrl", async () => {
      const { settingsState, setJiraConfig } = await import("../settingsStore");
      setJiraConfig({ baseUrl: "https://example.atlassian.net" });
      expect(settingsState.jiraConfig.baseUrl).toBe("https://example.atlassian.net");
      // restore
      setJiraConfig({ baseUrl: "" });
    });
  });

  describe("setJiraConnected", () => {
    it("marks as connected with site info", async () => {
      const { settingsState, setJiraConnected } = await import("../settingsStore");
      setJiraConnected(true, "Example Site", "cloud-id-123", null);
      expect(settingsState.jiraConfig.connected).toBe(true);
      expect(settingsState.jiraConfig.siteName).toBe("Example Site");
      expect(settingsState.jiraConfig.cloudId).toBe("cloud-id-123");
      expect(settingsState.jiraConfig.lastError).toBeNull();
      // restore
      setJiraConnected(false, "", "", null);
    });

    it("marks as disconnected with error message", async () => {
      const { settingsState, setJiraConnected } = await import("../settingsStore");
      setJiraConnected(false, "", "", "Unauthorized");
      expect(settingsState.jiraConfig.connected).toBe(false);
      expect(settingsState.jiraConfig.siteName).toBe("");
      expect(settingsState.jiraConfig.lastError).toBe("Unauthorized");
    });
  });

  describe("setTheme", () => {
    it("updates theme to dark", async () => {
      const { settingsState, setTheme } = await import("../settingsStore");
      setTheme("dark");
      expect(settingsState.appearance.theme).toBe("dark");
    });

    it("cycles through all theme values", async () => {
      const { settingsState, setTheme } = await import("../settingsStore");
      const themes: Theme[] = ["light", "dark", "system"];
      for (const t of themes) {
        setTheme(t);
        expect(settingsState.appearance.theme).toBe(t);
      }
    });
  });

  describe("setColorblindMode", () => {
    it("enables colorblind mode", async () => {
      const { settingsState, setColorblindMode } = await import("../settingsStore");
      setColorblindMode(true);
      expect(settingsState.appearance.colorblindMode).toBe(true);
    });

    it("disables colorblind mode", async () => {
      const { settingsState, setColorblindMode } = await import("../settingsStore");
      setColorblindMode(true);
      setColorblindMode(false);
      expect(settingsState.appearance.colorblindMode).toBe(false);
    });
  });

  describe("addColumn", () => {
    it("appends a column with order one greater than current max", async () => {
      const { settingsState, addColumn, removeColumn } = await import("../settingsStore");
      const beforeCount = settingsState.kanbanColumns.length;
      const maxOrder = Math.max(...settingsState.kanbanColumns.map((c) => c.order));

      addColumn({
        id: "test-col",
        title: "Test",
        stageMatch: "test",
        color: "#abc",
        behavior: { wipLimit: null, autoAssign: false, autoTransition: null, requireApproval: false },
        hooks: { onEnter: [], onExit: [], onStall: [] },
      });

      const after = settingsState.kanbanColumns.find((c) => c.id === "test-col");
      expect(after).toBeDefined();
      expect(after?.order).toBe(maxOrder + 1);
      expect(settingsState.kanbanColumns).toHaveLength(beforeCount + 1);

      // restore
      removeColumn("test-col");
    });
  });

  describe("removeColumn", () => {
    it("removes a column by id", async () => {
      const { settingsState, addColumn, removeColumn } = await import("../settingsStore");
      addColumn({
        id: "to-remove",
        title: "Remove Me",
        stageMatch: "remove",
        color: "#fff",
        behavior: { wipLimit: null, autoAssign: false, autoTransition: null, requireApproval: false },
        hooks: { onEnter: [], onExit: [], onStall: [] },
      });
      const before = settingsState.kanbanColumns.length;

      removeColumn("to-remove");

      expect(settingsState.kanbanColumns).toHaveLength(before - 1);
      expect(settingsState.kanbanColumns.find((c) => c.id === "to-remove")).toBeUndefined();
    });

    it("does not error on unknown id", async () => {
      const { settingsState, removeColumn } = await import("../settingsStore");
      const before = settingsState.kanbanColumns.length;
      removeColumn("nonexistent-id");
      expect(settingsState.kanbanColumns).toHaveLength(before);
    });
  });

  describe("updateColumn", () => {
    it("updates title without changing other fields", async () => {
      const { settingsState, updateColumn } = await import("../settingsStore");
      const first = settingsState.kanbanColumns[0];
      const originalColor = first.color;

      updateColumn(first.id, { title: "Updated Title" });

      const updated = settingsState.kanbanColumns.find((c) => c.id === first.id);
      expect(updated?.title).toBe("Updated Title");
      expect(updated?.color).toBe(originalColor);

      // restore
      updateColumn(first.id, { title: first.title });
    });
  });

  describe("updateColumnBehavior", () => {
    it("updates wipLimit without changing other behavior fields", async () => {
      const { settingsState, updateColumnBehavior } = await import("../settingsStore");
      const first = settingsState.kanbanColumns[0];
      const originalAutoAssign = first.behavior.autoAssign;

      updateColumnBehavior(first.id, { wipLimit: 5 });

      const updated = settingsState.kanbanColumns.find((c) => c.id === first.id);
      expect(updated?.behavior.wipLimit).toBe(5);
      expect(updated?.behavior.autoAssign).toBe(originalAutoAssign);

      // restore
      updateColumnBehavior(first.id, { wipLimit: null });
    });

    it("updates requireApproval flag", async () => {
      const { settingsState, updateColumnBehavior } = await import("../settingsStore");
      const first = settingsState.kanbanColumns[0];

      updateColumnBehavior(first.id, { requireApproval: true });

      const updated = settingsState.kanbanColumns.find((c) => c.id === first.id);
      expect(updated?.behavior.requireApproval).toBe(true);

      // restore
      updateColumnBehavior(first.id, { requireApproval: false });
    });
  });

  describe("updateColumnHooks", () => {
    it("sets onEnter hooks", async () => {
      const { settingsState, updateColumnHooks } = await import("../settingsStore");
      const first = settingsState.kanbanColumns[0];

      updateColumnHooks(first.id, { onEnter: ["hook-notify", "hook-assign"] });

      const updated = settingsState.kanbanColumns.find((c) => c.id === first.id);
      expect(updated?.hooks.onEnter).toEqual(["hook-notify", "hook-assign"]);

      // restore
      updateColumnHooks(first.id, { onEnter: [] });
    });

    it("sets onExit hooks without affecting onEnter", async () => {
      const { settingsState, updateColumnHooks } = await import("../settingsStore");
      const first = settingsState.kanbanColumns[0];
      const originalOnEnter = [...first.hooks.onEnter];

      updateColumnHooks(first.id, { onExit: ["hook-log"] });

      const updated = settingsState.kanbanColumns.find((c) => c.id === first.id);
      expect(updated?.hooks.onExit).toEqual(["hook-log"]);
      expect(updated?.hooks.onEnter).toEqual(originalOnEnter);

      // restore
      updateColumnHooks(first.id, { onExit: [] });
    });
  });

  describe("reorderColumns", () => {
    it("reassigns order values to match the supplied id array", async () => {
      const { settingsState, reorderColumns } = await import("../settingsStore");
      const ids = settingsState.kanbanColumns.map((c) => c.id);
      const reversed = [...ids].reverse();

      reorderColumns(reversed);

      reversed.forEach((id, idx) => {
        const col = settingsState.kanbanColumns.find((c) => c.id === id);
        expect(col?.order).toBe(idx);
      });

      // restore original order
      reorderColumns(ids);
    });
  });
});

// ---------------------------------------------------------------------------
// localStorage persistence helpers
// ---------------------------------------------------------------------------

// Mock localStorage for node test environment
function makeLocalStorageMock() {
  const store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => { store[key] = value; },
    removeItem: (key: string) => { delete store[key]; },
    clear: () => { Object.keys(store).forEach((k) => delete store[k]); },
  };
}

describe("loadPersistedSettings", () => {
  let mockStorage: ReturnType<typeof makeLocalStorageMock>;

  beforeEach(() => {
    mockStorage = makeLocalStorageMock();
    vi.stubGlobal("localStorage", mockStorage);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("returns empty object when localStorage is empty", () => {
    const result = loadPersistedSettings();
    expect(result).toEqual({});
  });

  it("returns empty object when stored JSON is invalid", () => {
    localStorage.setItem(STORAGE_KEY, "not-json");
    const result = loadPersistedSettings();
    expect(result).toEqual({});
  });

  it("restores appearance settings", () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ appearance: { theme: "dark", colorblindMode: true } }),
    );
    const result = loadPersistedSettings();
    expect(result.appearance).toEqual({ theme: "dark", colorblindMode: true });
  });

  it("restores sidebar widths", () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ sidebarWidths: { navSidebar: 300, triageSidebar: 400 } }),
    );
    const result = loadPersistedSettings();
    expect(result.sidebarWidths).toEqual({ navSidebar: 300, triageSidebar: 400 });
  });

  it("restores jira config with null lastError", () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        jiraConfig: {
          baseUrl: "https://example.atlassian.net",
          connected: true,
          siteName: "Example",
          cloudId: "abc123",
        },
      }),
    );
    const result = loadPersistedSettings();
    expect(result.jiraConfig?.baseUrl).toBe("https://example.atlassian.net");
    expect(result.jiraConfig?.connected).toBe(true);
    expect(result.jiraConfig?.lastError).toBeNull();
  });

  it("ignores unknown keys gracefully", () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ unknownKey: "value", appearance: { theme: "light", colorblindMode: false } }),
    );
    const result = loadPersistedSettings();
    expect(result.appearance?.theme).toBe("light");
    expect((result as Record<string, unknown>).unknownKey).toBeUndefined();
  });
});

describe("persistSettings", () => {
  let mockStorage: ReturnType<typeof makeLocalStorageMock>;

  beforeEach(() => {
    mockStorage = makeLocalStorageMock();
    vi.stubGlobal("localStorage", mockStorage);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("writes appearance and sidebarWidths to localStorage", () => {
    const state: SettingsState = {
      jiraConfig: { baseUrl: "", connected: false, siteName: "", cloudId: "", lastError: null },
      connectionTestStatus: "idle",
      appearance: { theme: "dark", colorblindMode: false },
      kanbanColumns: DEFAULT_KANBAN_COLUMNS,
      sidebarWidths: { navSidebar: 280, triageSidebar: 350 },
    };

    persistSettings(state);

    const raw = localStorage.getItem(STORAGE_KEY);
    expect(raw).not.toBeNull();
    const parsed = JSON.parse(raw!);
    expect(parsed.appearance.theme).toBe("dark");
    expect(parsed.sidebarWidths.navSidebar).toBe(280);
    expect(parsed.sidebarWidths.triageSidebar).toBe(350);
  });

  it("does not persist connectionTestStatus (transient state)", () => {
    const state: SettingsState = {
      jiraConfig: { baseUrl: "", connected: false, siteName: "", cloudId: "", lastError: null },
      connectionTestStatus: "testing",
      appearance: { theme: "system", colorblindMode: false },
      kanbanColumns: DEFAULT_KANBAN_COLUMNS,
      sidebarWidths: { navSidebar: 240, triageSidebar: 320 },
    };

    persistSettings(state);

    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed = JSON.parse(raw!);
    expect(parsed.connectionTestStatus).toBeUndefined();
  });

  it("does not persist lastError (transient state)", () => {
    const state: SettingsState = {
      jiraConfig: { baseUrl: "", connected: false, siteName: "", cloudId: "", lastError: "some error" },
      connectionTestStatus: "idle",
      appearance: { theme: "system", colorblindMode: false },
      kanbanColumns: DEFAULT_KANBAN_COLUMNS,
      sidebarWidths: { navSidebar: 240, triageSidebar: 320 },
    };

    persistSettings(state);

    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed = JSON.parse(raw!);
    expect(parsed.jiraConfig.lastError).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// Sidebar width constants
// ---------------------------------------------------------------------------

describe("sidebar width constants", () => {
  it("nav sidebar min is less than max", () => {
    expect(NAV_SIDEBAR_MIN).toBeLessThan(NAV_SIDEBAR_MAX);
  });

  it("triage sidebar min is less than max", () => {
    expect(TRIAGE_SIDEBAR_MIN).toBeLessThan(TRIAGE_SIDEBAR_MAX);
  });

  it("nav sidebar default (240) is within min/max bounds", () => {
    expect(240).toBeGreaterThanOrEqual(NAV_SIDEBAR_MIN);
    expect(240).toBeLessThanOrEqual(NAV_SIDEBAR_MAX);
  });

  it("triage sidebar default (320) is within min/max bounds", () => {
    expect(320).toBeGreaterThanOrEqual(TRIAGE_SIDEBAR_MIN);
    expect(320).toBeLessThanOrEqual(TRIAGE_SIDEBAR_MAX);
  });
});

// ---------------------------------------------------------------------------
// Sidebar width actions (via dynamic import)
// ---------------------------------------------------------------------------

describe("sidebar width store actions", () => {
  describe("setNavSidebarWidth", () => {
    it("clamps to minimum", async () => {
      const { settingsState, setNavSidebarWidth } = await import("../settingsStore");
      setNavSidebarWidth(50); // below min
      expect(settingsState.sidebarWidths.navSidebar).toBe(NAV_SIDEBAR_MIN);
    });

    it("clamps to maximum", async () => {
      const { settingsState, setNavSidebarWidth } = await import("../settingsStore");
      setNavSidebarWidth(9999); // above max
      expect(settingsState.sidebarWidths.navSidebar).toBe(NAV_SIDEBAR_MAX);
    });

    it("sets valid width within bounds", async () => {
      const { settingsState, setNavSidebarWidth } = await import("../settingsStore");
      setNavSidebarWidth(300);
      expect(settingsState.sidebarWidths.navSidebar).toBe(300);
      // restore
      setNavSidebarWidth(240);
    });
  });

  describe("setTriageSidebarWidth", () => {
    it("clamps to minimum", async () => {
      const { settingsState, setTriageSidebarWidth } = await import("../settingsStore");
      setTriageSidebarWidth(100); // below min
      expect(settingsState.sidebarWidths.triageSidebar).toBe(TRIAGE_SIDEBAR_MIN);
    });

    it("clamps to maximum", async () => {
      const { settingsState, setTriageSidebarWidth } = await import("../settingsStore");
      setTriageSidebarWidth(9999); // above max
      expect(settingsState.sidebarWidths.triageSidebar).toBe(TRIAGE_SIDEBAR_MAX);
    });

    it("sets valid width within bounds", async () => {
      const { settingsState, setTriageSidebarWidth } = await import("../settingsStore");
      setTriageSidebarWidth(400);
      expect(settingsState.sidebarWidths.triageSidebar).toBe(400);
      // restore
      setTriageSidebarWidth(320);
    });
  });
});
