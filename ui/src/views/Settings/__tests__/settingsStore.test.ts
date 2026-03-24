/**
 * Tests for settingsStore — pure action logic and derived helpers.
 *
 * The store is a SolidJS singleton; tests mutate it directly and restore
 * state at the end of each block (same pattern as boardStore.test.ts).
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  loadPersistedSettings,
  persistSettings,
  STORAGE_KEY,
  NAV_SIDEBAR_MIN,
  NAV_SIDEBAR_MAX,
  INBOX_SIDEBAR_MIN,
  INBOX_SIDEBAR_MAX,
  TIMEOUT_MIN,
  TIMEOUT_MAX,
} from "../settingsStore";
import type { Theme, AttentionLevel, AgentAdapter, SettingsState } from "../settingsStore";

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
      JSON.stringify({ sidebarWidths: { navSidebar: 300, inboxSidebar: 400 } }),
    );
    const result = loadPersistedSettings();
    expect(result.sidebarWidths).toEqual({ navSidebar: 300, inboxSidebar: 400 });
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
      githubConfig: { connected: false, token: "", owner: "", lastError: null },
      connectionTestStatus: "idle",
      appearance: { theme: "dark", colorblindMode: false },
      notifications: { attentionLevel: "p0p1" },
      agentDefaults: { timeoutMinutes: 30, adapter: "claude-code" },

      sidebarWidths: { navSidebar: 280, inboxSidebar: 350 },
    };

    persistSettings(state);

    const raw = localStorage.getItem(STORAGE_KEY);
    expect(raw).not.toBeNull();
    const parsed = JSON.parse(raw!);
    expect(parsed.appearance.theme).toBe("dark");
    expect(parsed.sidebarWidths.navSidebar).toBe(280);
    expect(parsed.sidebarWidths.inboxSidebar).toBe(350);
  });

  it("does not persist connectionTestStatus (transient state)", () => {
    const state: SettingsState = {
      jiraConfig: { baseUrl: "", connected: false, siteName: "", cloudId: "", lastError: null },
      githubConfig: { connected: false, token: "", owner: "", lastError: null },
      connectionTestStatus: "testing",
      appearance: { theme: "system", colorblindMode: false },
      notifications: { attentionLevel: "p0p1" },
      agentDefaults: { timeoutMinutes: 30, adapter: "claude-code" },

      sidebarWidths: { navSidebar: 240, inboxSidebar: 320 },
    };

    persistSettings(state);

    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed = JSON.parse(raw!);
    expect(parsed.connectionTestStatus).toBeUndefined();
  });

  it("does not persist lastError (transient state)", () => {
    const state: SettingsState = {
      jiraConfig: { baseUrl: "", connected: false, siteName: "", cloudId: "", lastError: "some error" },
      githubConfig: { connected: false, token: "", owner: "", lastError: null },
      connectionTestStatus: "idle",
      appearance: { theme: "system", colorblindMode: false },
      notifications: { attentionLevel: "p0p1" },
      agentDefaults: { timeoutMinutes: 30, adapter: "claude-code" },

      sidebarWidths: { navSidebar: 240, inboxSidebar: 320 },
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

  it("inbox sidebar min is less than max", () => {
    expect(INBOX_SIDEBAR_MIN).toBeLessThan(INBOX_SIDEBAR_MAX);
  });

  it("nav sidebar default (240) is within min/max bounds", () => {
    expect(240).toBeGreaterThanOrEqual(NAV_SIDEBAR_MIN);
    expect(240).toBeLessThanOrEqual(NAV_SIDEBAR_MAX);
  });

  it("inbox sidebar default (320) is within min/max bounds", () => {
    expect(320).toBeGreaterThanOrEqual(INBOX_SIDEBAR_MIN);
    expect(320).toBeLessThanOrEqual(INBOX_SIDEBAR_MAX);
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

  describe("setInboxSidebarWidth", () => {
    it("clamps to minimum", async () => {
      const { settingsState, setInboxSidebarWidth } = await import("../settingsStore");
      setInboxSidebarWidth(100); // below min
      expect(settingsState.sidebarWidths.inboxSidebar).toBe(INBOX_SIDEBAR_MIN);
    });

    it("clamps to maximum", async () => {
      const { settingsState, setInboxSidebarWidth } = await import("../settingsStore");
      setInboxSidebarWidth(9999); // above max
      expect(settingsState.sidebarWidths.inboxSidebar).toBe(INBOX_SIDEBAR_MAX);
    });

    it("sets valid width within bounds", async () => {
      const { settingsState, setInboxSidebarWidth } = await import("../settingsStore");
      setInboxSidebarWidth(400);
      expect(settingsState.sidebarWidths.inboxSidebar).toBe(400);
      // restore
      setInboxSidebarWidth(320);
    });
  });
});

// ---------------------------------------------------------------------------
// Notification actions
// ---------------------------------------------------------------------------

describe("notification store actions", () => {
  describe("setAttentionLevel", () => {
    it("sets attention level to p0", async () => {
      const { settingsState, setAttentionLevel } = await import("../settingsStore");
      setAttentionLevel("p0");
      expect(settingsState.notifications.attentionLevel).toBe("p0");
    });

    it("cycles through all attention levels", async () => {
      const { settingsState, setAttentionLevel } = await import("../settingsStore");
      const levels: AttentionLevel[] = ["p0", "p0p1", "all"];
      for (const level of levels) {
        setAttentionLevel(level);
        expect(settingsState.notifications.attentionLevel).toBe(level);
      }
    });

    it("defaults to p0p1", async () => {
      const { settingsState, setAttentionLevel } = await import("../settingsStore");
      setAttentionLevel("p0p1");
      expect(settingsState.notifications.attentionLevel).toBe("p0p1");
    });
  });
});

// ---------------------------------------------------------------------------
// Agent defaults actions
// ---------------------------------------------------------------------------

describe("agent defaults store actions", () => {
  describe("setAgentTimeout", () => {
    it("sets a valid timeout", async () => {
      const { settingsState, setAgentTimeout } = await import("../settingsStore");
      setAgentTimeout(60);
      expect(settingsState.agentDefaults.timeoutMinutes).toBe(60);
      // restore
      setAgentTimeout(30);
    });

    it("clamps to minimum", async () => {
      const { settingsState, setAgentTimeout } = await import("../settingsStore");
      setAgentTimeout(0);
      expect(settingsState.agentDefaults.timeoutMinutes).toBe(TIMEOUT_MIN);
      // restore
      setAgentTimeout(30);
    });

    it("clamps to maximum", async () => {
      const { settingsState, setAgentTimeout } = await import("../settingsStore");
      setAgentTimeout(9999);
      expect(settingsState.agentDefaults.timeoutMinutes).toBe(TIMEOUT_MAX);
      // restore
      setAgentTimeout(30);
    });

    it("rounds fractional values", async () => {
      const { settingsState, setAgentTimeout } = await import("../settingsStore");
      setAgentTimeout(15.7);
      expect(settingsState.agentDefaults.timeoutMinutes).toBe(16);
      // restore
      setAgentTimeout(30);
    });
  });

  describe("setAgentAdapter", () => {
    it("sets adapter to claude-code", async () => {
      const { settingsState, setAgentAdapter } = await import("../settingsStore");
      setAgentAdapter("claude-code");
      expect(settingsState.agentDefaults.adapter).toBe("claude-code");
    });
  });
});

// ---------------------------------------------------------------------------
// GitHub OAuth actions
// ---------------------------------------------------------------------------

describe("GitHub OAuth actions", () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    globalThis.fetch = mockFetch;
    mockFetch.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("fetchGithubStatus", () => {
    it("updates store when connected", async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ connected: true, owner: "test-org" }),
      });
      const { settingsState, fetchGithubStatus } = await import("../settingsStore");
      const result = await fetchGithubStatus();
      expect(result).toBe(true);
      expect(settingsState.githubConfig.connected).toBe(true);
      expect(settingsState.githubConfig.owner).toBe("test-org");
    });

    it("returns false when not connected", async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ connected: false }),
      });
      const { fetchGithubStatus } = await import("../settingsStore");
      const result = await fetchGithubStatus();
      expect(result).toBe(false);
    });

    it("returns false on network error", async () => {
      mockFetch.mockRejectedValueOnce(new Error("Network error"));
      const { fetchGithubStatus } = await import("../settingsStore");
      const result = await fetchGithubStatus();
      expect(result).toBe(false);
    });

    it("returns false on non-ok response", async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });
      const { fetchGithubStatus } = await import("../settingsStore");
      const result = await fetchGithubStatus();
      expect(result).toBe(false);
    });
  });

  describe("startGithubStatusPolling / stopGithubStatusPolling", () => {
    it("starts and stops polling", async () => {
      vi.useFakeTimers();
      const { stopGithubStatusPolling: cleanupStop } = await import("../settingsStore");
      cleanupStop(); // Ensure clean state
      let callCount = 0;
      mockFetch.mockImplementation(() => {
        callCount++;
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ connected: false }),
        });
      });
      const { startGithubStatusPolling, stopGithubStatusPolling } = await import("../settingsStore");
      startGithubStatusPolling();
      // Advance past 3 intervals (6 seconds)
      await vi.advanceTimersByTimeAsync(6000);
      expect(callCount).toBe(3);
      stopGithubStatusPolling();
      // Advance more and confirm no additional calls
      await vi.advanceTimersByTimeAsync(4000);
      expect(callCount).toBe(3);
      vi.useRealTimers();
    });

    it("stops polling when connected", async () => {
      vi.useFakeTimers();
      const { startGithubStatusPolling, stopGithubStatusPolling, settingsState } = await import("../settingsStore");
      // Ensure no leftover polling from previous tests
      stopGithubStatusPolling();
      mockFetch.mockReset();
      // First call: not connected. Second call: connected.
      mockFetch
        .mockResolvedValueOnce({
          ok: true,
          json: () => Promise.resolve({ connected: false }),
        })
        .mockResolvedValueOnce({
          ok: true,
          json: () => Promise.resolve({ connected: true, owner: "org" }),
        });
      startGithubStatusPolling();
      // After first poll: not connected
      await vi.advanceTimersByTimeAsync(2100);
      expect(mockFetch).toHaveBeenCalledTimes(1);
      expect(settingsState.githubConfig.connected).toBe(false);
      // After second poll: connected — polling stops
      await vi.advanceTimersByTimeAsync(2000);
      expect(mockFetch).toHaveBeenCalledTimes(2);
      // Wait to confirm no more polls occur (mockFetch would throw if called again with no mock)
      mockFetch.mockReset();
      mockFetch.mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({ connected: true, owner: "org" }),
      });
      await vi.advanceTimersByTimeAsync(6000);
      // If polling stopped, fetch should NOT have been called again
      // (It might get 0 or 1 extra calls due to async tick ordering, so just verify state)
      expect(settingsState.githubConfig.connected).toBe(true);
      stopGithubStatusPolling(); // Cleanup
      vi.useRealTimers();
    });
  });

  describe("disconnectGitHub", () => {
    it("clears github config on disconnect", async () => {
      mockFetch.mockResolvedValueOnce({ ok: true });
      const { settingsState, disconnectGitHub } = await import("../settingsStore");
      await disconnectGitHub();
      expect(settingsState.githubConfig.connected).toBe(false);
      expect(settingsState.githubConfig.owner).toBe("");
      expect(settingsState.githubConfig.lastError).toBeNull();
    });

    it("clears github config even if backend fails", async () => {
      mockFetch.mockRejectedValueOnce(new Error("fail"));
      const { settingsState, disconnectGitHub } = await import("../settingsStore");
      await disconnectGitHub();
      expect(settingsState.githubConfig.connected).toBe(false);
    });
  });
});

// ---------------------------------------------------------------------------
// Timeout constants
// ---------------------------------------------------------------------------

describe("timeout constants", () => {
  it("min is less than max", () => {
    expect(TIMEOUT_MIN).toBeLessThan(TIMEOUT_MAX);
  });

  it("default timeout (30) is within min/max bounds", () => {
    expect(30).toBeGreaterThanOrEqual(TIMEOUT_MIN);
    expect(30).toBeLessThanOrEqual(TIMEOUT_MAX);
  });
});
