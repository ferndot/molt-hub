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

import { projectState } from "../../stores/projectStore";

// ---------------------------------------------------------------------------
// Tauri-aware external URL opener
// ---------------------------------------------------------------------------

async function openExternalUrl(url: string): Promise<void> {
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank");
  }
}

function isTauriShell(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Normalize errors from `fetch`, Tauri plugins, etc. Some paths reject with a plain string.
 */
function formatInvokeRejection(err: unknown): string {
  if (typeof err === "string" && err.length > 0) {
    return err;
  }
  if (err instanceof Error && err.message.length > 0) {
    return err.message;
  }
  if (
    err &&
    typeof err === "object" &&
    "message" in err &&
    typeof (err as { message: unknown }).message === "string"
  ) {
    const m = (err as { message: string }).message;
    if (m.length > 0) {
      return m;
    }
  }
  return "Network error";
}

/** Shown when /api OAuth returns non-JSON or the desktop API never came up. */
function oauthApiMissingHint(): string {
  if (isTauriShell()) {
    if (import.meta.env.DEV) {
      return "The API on port 13401 is not reachable from this window. Run `molt-hub serve` on 13401 and keep `npm run dev` (Vite proxies `/api` there), or use `./dev.sh`.";
    }
    return "The built-in server on port 13401 did not return API data (often port conflict or startup failure). Quit other copies of Molt Hub, stop `molt-hub serve` if you want the app to own the port, then restart.";
  }
  return "Start the API on port 13401 (e.g. `./dev.sh` or `molt-hub serve`) and try again.";
}

/** Response body looks like HTML (SPA or wrong server on the API port). */
function responseBodyLooksLikeHtml(text: string): boolean {
  const t = text.trimStart().toLowerCase();
  return t.startsWith("<!") || t.startsWith("<html");
}

function oauthWrongPortOrSpaHint(): string {
  return "Received a web page instead of API data — another program may be using port 13401, or the UI loaded before the API was ready. Quit apps on that port, stop `molt-hub serve` if the desktop app should own it, and restart Molt Hub.";
}

function tryParseApiErrorMessage(text: string): string | null {
  try {
    const data = JSON.parse(text) as { error?: unknown };
    if (
      data &&
      typeof data === "object" &&
      typeof data.error === "string" &&
      data.error.length > 0
    ) {
      return data.error;
    }
  } catch {
    /* ignore */
  }
  return null;
}

/**
 * Parse `{ url: string }` from auth response text (already read from `Response`).
 */
function parseOAuthAuthJsonText(text: string): { url: string } | null {
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    return null;
  }
  if (!data || typeof data !== "object" || !("url" in data)) {
    return null;
  }
  const url = (data as { url: unknown }).url;
  if (typeof url !== "string" || url.length === 0) {
    return null;
  }
  try {
    new URL(url);
  } catch {
    return null;
  }
  return { url };
}

/**
 * `?projectId=` for integration OAuth when the active monitored project is not `default`.
 * Matches server `credential_scope_for_integration`.
 */
function oauthIntegrationQuerySuffix(): string {
  const id = projectState.activeProjectId?.trim();
  if (!id || id === "default") return "";
  return `?projectId=${encodeURIComponent(id)}`;
}

/**
 * OAuth start URL: always use same-origin `fetch` so dev (Vite → proxy → API) and
 * release (embedded Axum) share one code path. Avoids Tauri custom commands / ACL drift.
 */
async function getOAuthAuthorizationUrl(provider: "jira" | "github"): Promise<string> {
  const path =
    provider === "jira"
      ? "/api/integrations/jira/auth"
      : "/api/integrations/github/auth";
  const q = oauthIntegrationQuerySuffix();
  let response: Response;
  try {
    response = await fetch(`${path}${q}`);
  } catch {
    throw new Error(oauthApiMissingHint());
  }
  const text = await response.text();

  if (!response.ok) {
    const apiErr = tryParseApiErrorMessage(text);
    if (apiErr) {
      throw new Error(apiErr);
    }
    if (responseBodyLooksLikeHtml(text)) {
      throw new Error(oauthWrongPortOrSpaHint());
    }
    const ct = response.headers.get("content-type") ?? "";
    if (!ct.includes("application/json") && !text.trim().startsWith("{")) {
      throw new Error(oauthApiMissingHint());
    }
    throw new Error(text.trim() || `HTTP ${response.status}`);
  }

  const parsed = parseOAuthAuthJsonText(text);
  if (!parsed) {
    if (responseBodyLooksLikeHtml(text)) {
      throw new Error(oauthWrongPortOrSpaHint());
    }
    throw new Error(oauthApiMissingHint());
  }
  return parsed.url;
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type Theme = "light" | "dark" | "system";

export type AttentionLevel = "p0" | "p0p1" | "all";

export type AgentAdapter = "claude-code";

export interface NotificationConfig {
  attentionLevel: AttentionLevel;
}

export interface AgentDefaultsConfig {
  timeoutMinutes: number;
  adapter: AgentAdapter;
}

export interface JiraConfig {
  baseUrl: string;
  connected: boolean;
  siteName: string;
  cloudId: string;
  lastError: string | null;
}

export interface GitHubConfig {
  connected: boolean;
  token: string;
  owner: string;
  lastError: string | null;
}

export type ConnectionTestStatus = "idle" | "testing" | "success" | "error";

export interface AppearanceConfig {
  theme: Theme;
  colorblindMode: boolean;
}

export interface SidebarWidths {
  /** Left navigation sidebar width in pixels */
  navSidebar: number;
  /** Inbox sidebar width in pixels */
  inboxSidebar: number;
}

export interface SettingsState {
  jiraConfig: JiraConfig;
  githubConfig: GitHubConfig;
  connectionTestStatus: ConnectionTestStatus;
  appearance: AppearanceConfig;
  notifications: NotificationConfig;
  agentDefaults: AgentDefaultsConfig;
  sidebarWidths: SidebarWidths;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const STORAGE_KEY = "molt-hub-settings";

/** Keys that are persisted to localStorage (excludes transient state) */
type PersistedState = Pick<SettingsState, "appearance" | "notifications" | "agentDefaults" | "sidebarWidths"> & {
  jiraConfig: Pick<JiraConfig, "baseUrl" | "connected" | "siteName" | "cloudId">;
  githubConfig?: Pick<GitHubConfig, "connected" | "owner">;
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
    if (parsed.notifications) result.notifications = parsed.notifications;
    if (parsed.agentDefaults) result.agentDefaults = parsed.agentDefaults;
    if (parsed.sidebarWidths) result.sidebarWidths = parsed.sidebarWidths;
    if (parsed.jiraConfig) {
      result.jiraConfig = {
        lastError: null,
        ...parsed.jiraConfig,
      };
    }
    if (parsed.githubConfig) {
      result.githubConfig = {
        token: "",
        lastError: null,
        ...parsed.githubConfig,
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
      notifications: state.notifications,
      agentDefaults: state.agentDefaults,
      sidebarWidths: state.sidebarWidths,
      jiraConfig: {
        baseUrl: state.jiraConfig.baseUrl,
        connected: state.jiraConfig.connected,
        siteName: state.jiraConfig.siteName,
        cloudId: state.jiraConfig.cloudId,
      },
      githubConfig: {
        connected: state.githubConfig.connected,
        owner: state.githubConfig.owner,
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
  githubConfig: {
    connected: false,
    token: "",
    owner: "",
    lastError: null,
  },
  connectionTestStatus: "idle",
  appearance: {
    theme: "system",
    colorblindMode: false,
  },
  notifications: {
    attentionLevel: "p0p1",
  },
  agentDefaults: {
    timeoutMinutes: 30,
    adapter: "claude-code",
  },
  sidebarWidths: {
    navSidebar: 240,
    inboxSidebar: 320,
  },
};

const persisted = loadPersistedSettings();
const initialState: SettingsState = {
  ...defaultState,
  ...persisted,
  jiraConfig: { ...defaultState.jiraConfig, ...(persisted.jiraConfig ?? {}) },
  githubConfig: { ...defaultState.githubConfig, ...(persisted.githubConfig ?? {}) },
  appearance: { ...defaultState.appearance, ...(persisted.appearance ?? {}) },
  notifications: { ...defaultState.notifications, ...(persisted.notifications ?? {}) },
  agentDefaults: { ...defaultState.agentDefaults, ...(persisted.agentDefaults ?? {}) },
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
      notifications: state.notifications,
      agentDefaults: state.agentDefaults,
      sidebarWidths: state.sidebarWidths,
      jiraConfig: {
        baseUrl: state.jiraConfig.baseUrl,
        connected: state.jiraConfig.connected,
        siteName: state.jiraConfig.siteName,
        cloudId: state.jiraConfig.cloudId,
      },
      githubConfig: {
        connected: state.githubConfig.connected,
        owner: state.githubConfig.owner,
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
    if (data.notifications) result.notifications = data.notifications;
    if (data.agentDefaults) result.agentDefaults = data.agentDefaults;
    if (data.sidebarWidths) result.sidebarWidths = data.sidebarWidths;
    if (data.jiraConfig) {
      result.jiraConfig = { lastError: null, ...data.jiraConfig };
    }
    if (data.githubConfig) {
      result.githubConfig = { token: "", lastError: null, ...data.githubConfig };
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
      githubConfig: { ...current.githubConfig, ...(backend.githubConfig ?? {}) },
      appearance: { ...current.appearance, ...(backend.appearance ?? {}) },
      notifications: { ...current.notifications, ...(backend.notifications ?? {}) },
      agentDefaults: { ...current.agentDefaults, ...(backend.agentDefaults ?? {}) },
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

  // Apply colorblind mode class to document root
  createEffect(() => {
    const enabled = settingsState.appearance.colorblindMode;
    const root = document.documentElement;
    if (enabled) {
      root.classList.add("colorblind");
    } else {
      root.classList.remove("colorblind");
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
 * Initiate Jira OAuth: fetch the authorization URL from the backend,
 * then open it in a new window/tab so the user can authorize.
 * Starts polling for connection status so the UI updates once the
 * callback completes in the other tab.
 */
export async function connectJira(): Promise<void> {
  try {
    const url = await getOAuthAuthorizationUrl("jira");
    await openExternalUrl(url);
    // Start polling for when the user completes OAuth in the other tab
    startJiraStatusPolling();
  } catch (err) {
    setJiraConnected(false, "", "", formatInvokeRejection(err));
  }
}

/**
 * Disconnect Jira OAuth — clears tokens on the backend and local state.
 */
export async function disconnectJira(): Promise<void> {
  try {
    await fetch(
      `/api/integrations/jira/disconnect${oauthIntegrationQuerySuffix()}`,
      { method: "POST" },
    );
  } catch {
    // Ignore network errors — we clear local state regardless
  }
  setJiraConnected(false, "", "", null);
  setJiraConfig({ baseUrl: "" });
  setConnectionTestStatus("idle");
}

// ---------------------------------------------------------------------------
// Jira status polling
// ---------------------------------------------------------------------------

let _jiraPollTimer: ReturnType<typeof setInterval> | null = null;
const JIRA_POLL_INTERVAL = 2000;
const JIRA_POLL_MAX_ATTEMPTS = 60; // 2 minutes max

/**
 * Fetch current Jira connection status from the backend and update store.
 * Returns the connected state.
 */
export async function fetchJiraStatus(): Promise<boolean> {
  try {
    const response = await fetch(
      `/api/integrations/jira/status${oauthIntegrationQuerySuffix()}`,
    );
    if (!response.ok) return false;
    const data = (await response.json()) as { connected: boolean; site_url?: string; site_name?: string };
    setSettingsState(
      produce((s) => {
        s.jiraConfig.connected = data.connected;
        if (data.site_name) s.jiraConfig.siteName = data.site_name;
        if (data.site_url) s.jiraConfig.baseUrl = data.site_url;
        if (data.connected) s.jiraConfig.lastError = null;
      }),
    );
    return data.connected;
  } catch {
    return false;
  }
}

/**
 * Start polling `/api/integrations/jira/status` every 2 seconds.
 * Stops automatically once connected or after max attempts.
 */
export function startJiraStatusPolling(): void {
  stopJiraStatusPolling();
  let attempts = 0;
  _jiraPollTimer = setInterval(async () => {
    attempts++;
    const connected = await fetchJiraStatus();
    if (connected || attempts >= JIRA_POLL_MAX_ATTEMPTS) {
      stopJiraStatusPolling();
    }
  }, JIRA_POLL_INTERVAL);
}

/**
 * Stop the Jira status polling interval if running.
 */
export function stopJiraStatusPolling(): void {
  if (_jiraPollTimer !== null) {
    clearInterval(_jiraPollTimer);
    _jiraPollTimer = null;
  }
}

/**
 * @deprecated Use connectJira() instead.
 * Kept for backward compatibility with any existing callers.
 */
export async function initiateOAuth(): Promise<void> {
  return connectJira();
}

/**
 * Handle the OAuth callback result (called after redirect back from Atlassian).
 * Expects a code query param to be exchanged for tokens by the backend.
 * @deprecated The new flow uses status polling instead of direct callback handling.
 */
export async function handleOAuthCallback(code: string): Promise<void> {
  setConnectionTestStatus("testing");
  try {
    const response = await fetch(`/api/integrations/jira/oauth/callback?code=${encodeURIComponent(code)}&state=`);
    if (response.ok) {
      const data = await response.json() as { site_url?: string; site_name?: string };
      if (data.site_url) setJiraConfig({ baseUrl: data.site_url });
      setJiraConnected(true, data.site_name ?? "", "", null);
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

// ---------------------------------------------------------------------------
// GitHub actions (OAuth App flow)
// ---------------------------------------------------------------------------

/**
 * Initiate GitHub OAuth: fetch the authorization URL from the backend,
 * then open it in a new window/tab so the user can authorize.
 * Starts polling for connection status so the UI updates once the
 * callback completes in the other tab.
 */
export async function connectGitHub(): Promise<void> {
  try {
    const url = await getOAuthAuthorizationUrl("github");
    await openExternalUrl(url);
    // Start polling for when the user completes OAuth in the other tab
    startGithubStatusPolling();
  } catch (err) {
    const message = formatInvokeRejection(err);
    setSettingsState(
      produce((s) => {
        s.githubConfig.lastError = message;
      }),
    );
  }
}

/**
 * Disconnect GitHub — clears local state and notifies the backend.
 */
export async function disconnectGitHub(): Promise<void> {
  try {
    await fetch(
      `/api/integrations/github/disconnect${oauthIntegrationQuerySuffix()}`,
      { method: "POST" },
    );
  } catch {
    // Ignore network errors — we clear local state regardless
  }
  setSettingsState(
    produce((s) => {
      s.githubConfig.connected = false;
      s.githubConfig.token = "";
      s.githubConfig.owner = "";
      s.githubConfig.lastError = null;
    }),
  );
}

// ---------------------------------------------------------------------------
// GitHub status polling
// ---------------------------------------------------------------------------

let _githubPollTimer: ReturnType<typeof setInterval> | null = null;
const GITHUB_POLL_INTERVAL = 2000;
const GITHUB_POLL_MAX_ATTEMPTS = 60; // 2 minutes max

/**
 * Fetch current GitHub connection status from the backend and update store.
 * Returns the connected state.
 */
export async function fetchGithubStatus(): Promise<boolean> {
  try {
    const response = await fetch(
      `/api/integrations/github/status${oauthIntegrationQuerySuffix()}`,
    );
    if (!response.ok) return false;
    const data = (await response.json()) as { connected: boolean; owner?: string };
    setSettingsState(
      produce((s) => {
        s.githubConfig.connected = data.connected;
        if (!data.connected) {
          s.githubConfig.owner = "";
        } else if (data.owner) {
          s.githubConfig.owner = data.owner;
        }
        if (data.connected) s.githubConfig.lastError = null;
      }),
    );
    return data.connected;
  } catch {
    return false;
  }
}

/**
 * Start polling `/api/integrations/github/status` every 2 seconds.
 * Stops automatically once connected or after max attempts.
 */
export function startGithubStatusPolling(): void {
  stopGithubStatusPolling();
  let attempts = 0;
  _githubPollTimer = setInterval(async () => {
    attempts++;
    const connected = await fetchGithubStatus();
    if (connected || attempts >= GITHUB_POLL_MAX_ATTEMPTS) {
      stopGithubStatusPolling();
    }
  }, GITHUB_POLL_INTERVAL);
}

/**
 * Stop the GitHub status polling interval if running.
 */
export function stopGithubStatusPolling(): void {
  if (_githubPollTimer !== null) {
    clearInterval(_githubPollTimer);
    _githubPollTimer = null;
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
// Notification actions
// ---------------------------------------------------------------------------

export function setAttentionLevel(level: AttentionLevel): void {
  setSettingsState("notifications", "attentionLevel", level);
}

// ---------------------------------------------------------------------------
// Agent defaults actions
// ---------------------------------------------------------------------------

export const TIMEOUT_MIN = 1;
export const TIMEOUT_MAX = 480;

export function setAgentTimeout(minutes: number): void {
  const clamped = Math.max(TIMEOUT_MIN, Math.min(TIMEOUT_MAX, Math.round(minutes)));
  setSettingsState("agentDefaults", "timeoutMinutes", clamped);
}

export function setAgentAdapter(adapter: AgentAdapter): void {
  setSettingsState("agentDefaults", "adapter", adapter);
}

// ---------------------------------------------------------------------------
// Sidebar width actions
// ---------------------------------------------------------------------------

export const NAV_SIDEBAR_MIN = 180;
export const NAV_SIDEBAR_MAX = 400;
export const INBOX_SIDEBAR_MIN = 250;
export const INBOX_SIDEBAR_MAX = 600;

export function setNavSidebarWidth(width: number): void {
  const clamped = Math.max(NAV_SIDEBAR_MIN, Math.min(NAV_SIDEBAR_MAX, width));
  setSettingsState("sidebarWidths", "navSidebar", clamped);
}

export function setInboxSidebarWidth(width: number): void {
  const clamped = Math.max(INBOX_SIDEBAR_MIN, Math.min(INBOX_SIDEBAR_MAX, width));
  setSettingsState("sidebarWidths", "inboxSidebar", clamped);
}

