/**
 * Settings store — holds global application configuration.
 *
 * Covers:
 *  - Jira integration config (baseUrl, OAuth connection status, site info)
 *  - Appearance (theme, colorblind mode)
 *  - Kanban column definitions (configurable)
 *  - Sidebar widths (persisted to localStorage)
 *
 * Actions are pure functions so they can be tested in a node environment.
 */

import { createStore, produce } from "solid-js/store";
import { createEffect } from "solid-js";
import { createSignal } from "solid-js";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type Theme = "light" | "dark" | "system";

export interface JiraConfig {
  baseUrl: string;
  connected: boolean;
  siteName: string;
  cloudId: string;
  lastError: string | null;
}

export type ConnectionTestStatus = "idle" | "testing" | "success" | "error";

export interface AppearanceConfig {
  theme: Theme;
  colorblindMode: boolean;
}

export interface ColumnBehavior {
  wipLimit: number | null;
  autoAssign: boolean;
  autoTransition: string | null;
  requireApproval: boolean;
}

export interface ColumnHooks {
  onEnter: string[];
  onExit: string[];
  onStall: string[];
}

export interface KanbanColumn {
  /** Unique stable identifier */
  id: string;
  /** Display title */
  title: string;
  /** Glob / comma-separated pattern of stage names this column captures */
  stageMatch: string;
  /** Accent color (hex or CSS color) */
  color: string;
  /** Display order (ascending) */
  order: number;
  /** Column behavior settings */
  behavior: ColumnBehavior;
  /** Column lifecycle hooks */
  hooks: ColumnHooks;
}

export interface SidebarWidths {
  /** Left navigation sidebar width in pixels */
  navSidebar: number;
  /** Triage/inbox sidebar width in pixels */
  triageSidebar: number;
}

export interface SettingsState {
  jiraConfig: JiraConfig;
  connectionTestStatus: ConnectionTestStatus;
  appearance: AppearanceConfig;
  kanbanColumns: KanbanColumn[];
  sidebarWidths: SidebarWidths;
}

// ---------------------------------------------------------------------------
// Default column definitions matching boardStore STAGES
// ---------------------------------------------------------------------------

const defaultBehavior: ColumnBehavior = {
  wipLimit: null,
  autoAssign: false,
  autoTransition: null,
  requireApproval: false,
};

const defaultHooks: ColumnHooks = {
  onEnter: [],
  onExit: [],
  onStall: [],
};

export const DEFAULT_KANBAN_COLUMNS: KanbanColumn[] = [
  { id: "col-backlog",     title: "Backlog",      stageMatch: "backlog",      color: "#6b7280", order: 0, behavior: { ...defaultBehavior }, hooks: { ...defaultHooks } },
  { id: "col-in-progress", title: "In Progress",  stageMatch: "in-progress",  color: "#3b82f6", order: 1, behavior: { ...defaultBehavior }, hooks: { ...defaultHooks } },
  { id: "col-code-review", title: "Code Review",  stageMatch: "code-review",  color: "#f59e0b", order: 2, behavior: { ...defaultBehavior, requireApproval: true }, hooks: { ...defaultHooks } },
  { id: "col-testing",     title: "Testing",      stageMatch: "testing",      color: "#8b5cf6", order: 3, behavior: { ...defaultBehavior }, hooks: { ...defaultHooks } },
  { id: "col-deployed",    title: "Deployed",     stageMatch: "deployed",     color: "#10b981", order: 4, behavior: { ...defaultBehavior }, hooks: { ...defaultHooks } },
];

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const STORAGE_KEY = "molt-hub-settings";

/** Keys that are persisted to localStorage (excludes transient state) */
type PersistedState = Pick<SettingsState, "appearance" | "kanbanColumns" | "sidebarWidths"> & {
  jiraConfig: Pick<JiraConfig, "baseUrl" | "connected" | "siteName" | "cloudId">;
};

/** Load persisted state from localStorage, merging with defaults. */
export function loadPersistedSettings(): Partial<SettingsState> {
  if (typeof localStorage === "undefined") return {};
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Partial<PersistedState>;
    const result: Partial<SettingsState> = {};
    if (parsed.appearance) result.appearance = parsed.appearance;
    if (parsed.kanbanColumns) result.kanbanColumns = parsed.kanbanColumns;
    if (parsed.sidebarWidths) result.sidebarWidths = parsed.sidebarWidths;
    if (parsed.jiraConfig) {
      result.jiraConfig = {
        lastError: null,
        ...parsed.jiraConfig,
      };
    }
    return result;
  } catch {
    return {};
  }
}

/** Serialize the settings state to localStorage. */
export function persistSettings(state: SettingsState): void {
  if (typeof localStorage === "undefined") return;
  try {
    const persisted: PersistedState = {
      appearance: state.appearance,
      kanbanColumns: state.kanbanColumns,
      sidebarWidths: state.sidebarWidths,
      jiraConfig: {
        baseUrl: state.jiraConfig.baseUrl,
        connected: state.jiraConfig.connected,
        siteName: state.jiraConfig.siteName,
        cloudId: state.jiraConfig.cloudId,
      },
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(persisted));
  } catch {
    // localStorage may be unavailable in some environments; silently ignore
  }
}

const defaultState: SettingsState = {
  jiraConfig: {
    baseUrl: "",
    connected: false,
    siteName: "",
    cloudId: "",
    lastError: null,
  },
  connectionTestStatus: "idle",
  appearance: {
    theme: "system",
    colorblindMode: false,
  },
  kanbanColumns: DEFAULT_KANBAN_COLUMNS,
  sidebarWidths: {
    navSidebar: 240,
    triageSidebar: 320,
  },
};

const persisted = loadPersistedSettings();
const initialState: SettingsState = {
  ...defaultState,
  ...persisted,
  jiraConfig: { ...defaultState.jiraConfig, ...(persisted.jiraConfig ?? {}) },
  appearance: { ...defaultState.appearance, ...(persisted.appearance ?? {}) },
  sidebarWidths: { ...defaultState.sidebarWidths, ...(persisted.sidebarWidths ?? {}) },
};

export const [settingsState, setSettingsState] =
  createStore<SettingsState>(initialState);

// ---------------------------------------------------------------------------
// Backend sync error signal (shown in status bar)
// ---------------------------------------------------------------------------

/**
 * Reactive signal: non-null when the last backend save failed.
 * Components (e.g., StatusBar) can read this to show a subtle error notice.
 */
export const [backendSaveError, setBackendSaveError] = createSignal<string | null>(null);

// ---------------------------------------------------------------------------
// Backend API helpers
// ---------------------------------------------------------------------------

/**
 * Serialize the current settings state and persist it to the backend API.
 * Writes to localStorage as an offline cache regardless of backend outcome.
 *
 * On failure, sets backendSaveError so the status bar can surface a hint.
 */
export async function saveToBackend(state: SettingsState): Promise<void> {
  // Always write localStorage as offline cache
  persistSettings(state);

  try {
    const persisted: PersistedState = {
      appearance: state.appearance,
      kanbanColumns: state.kanbanColumns,
      sidebarWidths: state.sidebarWidths,
      jiraConfig: {
        baseUrl: state.jiraConfig.baseUrl,
        connected: state.jiraConfig.connected,
        siteName: state.jiraConfig.siteName,
        cloudId: state.jiraConfig.cloudId,
      },
    };
    const response = await fetch("/api/settings", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(persisted),
    });
    if (!response.ok) {
      setBackendSaveError(`Settings not saved (HTTP ${response.status})`);
    } else {
      setBackendSaveError(null);
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : "Network error";
    setBackendSaveError(`Settings not saved (${message})`);
  }
}

/**
 * Load settings from the backend API.
 * Returns a partial settings object on success, or null if unavailable.
 */
export async function loadFromBackend(): Promise<Partial<SettingsState> | null> {
  try {
    const response = await fetch("/api/settings");
    if (!response.ok) return null;
    const data = await response.json() as Partial<PersistedState>;
    const result: Partial<SettingsState> = {};
    if (data.appearance) result.appearance = data.appearance;
    if (data.kanbanColumns) result.kanbanColumns = data.kanbanColumns;
    if (data.sidebarWidths) result.sidebarWidths = data.sidebarWidths;
    if (data.jiraConfig) {
      result.jiraConfig = { lastError: null, ...data.jiraConfig };
    }
    return result;
  } catch {
    return null;
  }
}

/**
 * Initialize settings: try the backend first, fall back to localStorage.
 * Call this once at app startup (e.g., in App.tsx or a root component).
 */
export async function initSettings(): Promise<void> {
  const backend = await loadFromBackend();
  if (backend) {
    setSettingsState((current) => ({
      ...current,
      ...backend,
      jiraConfig: { ...current.jiraConfig, ...(backend.jiraConfig ?? {}) },
      appearance: { ...current.appearance, ...(backend.appearance ?? {}) },
      sidebarWidths: { ...current.sidebarWidths, ...(backend.sidebarWidths ?? {}) },
    }));
  }
  // localStorage was already loaded at module init; backend hydration takes precedence
}

// ---------------------------------------------------------------------------
// Debounced backend save on state changes
// ---------------------------------------------------------------------------

let _saveTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleSave(state: SettingsState): void {
  if (_saveTimer !== null) clearTimeout(_saveTimer);
  _saveTimer = setTimeout(() => {
    _saveTimer = null;
    saveToBackend(state);
  }, 500);
}

// Persist to localStorage and schedule backend save whenever state changes
if (typeof createEffect !== "undefined") {
  createEffect(() => {
    // Access reactive fields to track dependencies
    const snapshot = settingsState;
    persistSettings(snapshot);
    scheduleSave(snapshot);
  });

  // Apply theme to the document root so CSS variables respond
  createEffect(() => {
    const theme = settingsState.appearance.theme;
    const root = document.documentElement;
    if (theme === "system") {
      root.removeAttribute("data-theme");
      root.style.colorScheme = "light dark";
    } else {
      root.setAttribute("data-theme", theme);
      root.style.colorScheme = theme;
    }
  });
}

// ---------------------------------------------------------------------------
// Jira OAuth actions
// ---------------------------------------------------------------------------

export function setJiraConfig(
  partial: Partial<Omit<JiraConfig, "connected" | "lastError" | "siteName" | "cloudId">>,
): void {
  setSettingsState(
    produce((s) => {
      Object.assign(s.jiraConfig, partial);
    }),
  );
}

export function setJiraConnected(
  connected: boolean,
  siteName: string = "",
  cloudId: string = "",
  error: string | null = null,
): void {
  setSettingsState(
    produce((s) => {
      s.jiraConfig.connected = connected;
      s.jiraConfig.siteName = siteName;
      s.jiraConfig.cloudId = cloudId;
      s.jiraConfig.lastError = error;
    }),
  );
}

export function setConnectionTestStatus(status: ConnectionTestStatus): void {
  setSettingsState("connectionTestStatus", status);
}

/**
 * Initiate OAuth flow: fetch the authorization URL from the backend,
 * then redirect the browser to Atlassian's consent screen.
 */
export async function initiateOAuth(): Promise<void> {
  try {
    const response = await fetch("/api/integrations/jira/oauth/authorize");
    if (!response.ok) {
      const text = await response.text();
      setJiraConnected(false, "", "", text || `HTTP ${response.status}`);
      return;
    }
    // Guard against SPA fallback returning HTML instead of JSON
    const ct = response.headers.get("content-type") ?? "";
    if (!ct.includes("application/json")) {
      setJiraConnected(false, "", "", "Backend not available — start the server first");
      return;
    }
    const data = await response.json() as { url: string };
    window.location.href = data.url;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Network error";
    setJiraConnected(false, "", "", message);
  }
}

/**
 * Handle the OAuth callback result (called after redirect back from Atlassian).
 * Expects a code query param to be exchanged for tokens by the backend.
 */
export async function handleOAuthCallback(code: string): Promise<void> {
  setConnectionTestStatus("testing");
  try {
    const response = await fetch("/api/integrations/jira/oauth/callback", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code }),
    });
    if (response.ok) {
      const data = await response.json() as { siteName: string; cloudId: string; baseUrl: string };
      setJiraConfig({ baseUrl: data.baseUrl });
      setJiraConnected(true, data.siteName, data.cloudId, null);
      setConnectionTestStatus("success");
    } else {
      const text = await response.text();
      setJiraConnected(false, "", "", text || `HTTP ${response.status}`);
      setConnectionTestStatus("error");
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : "Network error";
    setJiraConnected(false, "", "", message);
    setConnectionTestStatus("error");
  }
}

/**
 * Disconnect Jira OAuth — revokes tokens on the backend and clears local state.
 */
export async function disconnectJira(): Promise<void> {
  try {
    await fetch("/api/integrations/jira/oauth/disconnect", { method: "POST" });
  } finally {
    setJiraConnected(false, "", "", null);
    setJiraConfig({ baseUrl: "" });
    setConnectionTestStatus("idle");
  }
}

// ---------------------------------------------------------------------------
// Appearance actions
// ---------------------------------------------------------------------------

export function setTheme(theme: Theme): void {
  setSettingsState("appearance", "theme", theme);
}

export function setColorblindMode(enabled: boolean): void {
  setSettingsState("appearance", "colorblindMode", enabled);
}

// ---------------------------------------------------------------------------
// Kanban column actions
// ---------------------------------------------------------------------------

export function addColumn(col: Omit<KanbanColumn, "order">): void {
  setSettingsState(
    produce((s) => {
      const maxOrder = s.kanbanColumns.reduce(
        (max, c) => Math.max(max, c.order),
        -1,
      );
      s.kanbanColumns.push({ ...col, order: maxOrder + 1 });
    }),
  );
}

export function removeColumn(id: string): void {
  setSettingsState(
    produce((s) => {
      s.kanbanColumns = s.kanbanColumns.filter((c) => c.id !== id);
    }),
  );
}

export function updateColumn(id: string, partial: Partial<Omit<KanbanColumn, "id">>): void {
  setSettingsState(
    produce((s) => {
      const col = s.kanbanColumns.find((c) => c.id === id);
      if (col) Object.assign(col, partial);
    }),
  );
}

export function updateColumnBehavior(id: string, partial: Partial<ColumnBehavior>): void {
  setSettingsState(
    produce((s) => {
      const col = s.kanbanColumns.find((c) => c.id === id);
      if (col) Object.assign(col.behavior, partial);
    }),
  );
}

export function updateColumnHooks(id: string, partial: Partial<ColumnHooks>): void {
  setSettingsState(
    produce((s) => {
      const col = s.kanbanColumns.find((c) => c.id === id);
      if (col) Object.assign(col.hooks, partial);
    }),
  );
}

/**
 * Reorder columns by supplying a new ordered array of IDs.
 * The `order` field on each column is updated to match the index position.
 */
export function reorderColumns(orderedIds: string[]): void {
  setSettingsState(
    produce((s) => {
      orderedIds.forEach((id, idx) => {
        const col = s.kanbanColumns.find((c) => c.id === id);
        if (col) col.order = idx;
      });
    }),
  );
}

// ---------------------------------------------------------------------------
// Sidebar width actions
// ---------------------------------------------------------------------------

export const NAV_SIDEBAR_MIN = 180;
export const NAV_SIDEBAR_MAX = 400;
export const TRIAGE_SIDEBAR_MIN = 250;
export const TRIAGE_SIDEBAR_MAX = 600;

export function setNavSidebarWidth(width: number): void {
  const clamped = Math.max(NAV_SIDEBAR_MIN, Math.min(NAV_SIDEBAR_MAX, width));
  setSettingsState("sidebarWidths", "navSidebar", clamped);
}

export function setTriageSidebarWidth(width: number): void {
  const clamped = Math.max(TRIAGE_SIDEBAR_MIN, Math.min(TRIAGE_SIDEBAR_MAX, width));
  setSettingsState("sidebarWidths", "triageSidebar", clamped);
}

// ---------------------------------------------------------------------------
// Derived helpers (pure — safe to use in tests)
// ---------------------------------------------------------------------------

/** Returns columns sorted by their order field. */
export function getSortedColumns(columns: KanbanColumn[]): KanbanColumn[] {
  return [...columns].sort((a, b) => a.order - b.order);
}

/** Returns the stage names matched by a column (comma or space separated). */
export function getStagesForColumn(col: KanbanColumn): string[] {
  return col.stageMatch.split(/[\s,]+/).filter(Boolean);
}

/** Parse a comma-separated hook string into an array of hook IDs. */
export function parseHookIds(value: string): string[] {
  return value.split(/[\s,]+/).filter(Boolean);
}

/** Serialize hook IDs array to comma-separated string for display. */
export function serializeHookIds(ids: string[]): string {
  return ids.join(", ");
}
