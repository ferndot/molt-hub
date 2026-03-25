/**
 * SettingsView — main settings page with left sidebar nav + right detail panel.
 *
 * Sections: Appearance, Notifications, Agent Defaults, Integrations.
 * Route: /settings
 */

import type { JSX } from "solid-js";
import { createSignal, Show, For, type Component } from "solid-js";
import {
  TbOutlinePalette,
  TbOutlinePlug,
  TbOutlineSquareRotated,
  TbOutlineBrandGithub,
  TbOutlineArrowLeft,
  TbOutlineBell,
  TbOutlineRobot,
  TbOutlineClipboardList,
} from "solid-icons/tb";
import {
  settingsState,
  connectJira,
  disconnectJira,
  connectGitHub,
  disconnectGitHub,
  fetchGithubStatus,
  fetchJiraStatus,
  setTheme,
  setColorblindMode,
  setAttentionLevel,
  setAgentTimeout,
  setAgentAdapter,
  setHarnesses,
} from "./settingsStore";
import type { Theme, AttentionLevel, HarnessEntry } from "./settingsStore";
import AuditLog from "./AuditLog";
import PriorityBadge from "../../components/PriorityBadge/PriorityBadge";
import styles from "./Settings.module.css";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type SectionId =
  | "appearance"
  | "notifications"
  | "agent-defaults"
  | "integrations"
  | "jira"
  | "github"
  | "audit-log";

// ---------------------------------------------------------------------------
// Section: Appearance
// ---------------------------------------------------------------------------

const AppearancePanel: Component = () => {
  const THEMES: { value: Theme; label: string; desc: string }[] = [
    { value: "system", label: "System", desc: "Follow OS setting" },
    { value: "light", label: "Light", desc: "" },
    { value: "dark", label: "Dark", desc: "" },
  ];

  return (
    <div>
      <h3 class={styles.sectionTitle}>Theme</h3>
      <div class={styles.formGroup}>
        <For each={THEMES}>
          {(t) => (
            <div class={styles.toggleRow}>
              <input
                type="radio"
                id={`theme-${t.value}`}
                name="theme"
                value={t.value}
                checked={settingsState.appearance.theme === t.value}
                onChange={() => setTheme(t.value)}
              />
              <label class={styles.toggleLabel} for={`theme-${t.value}`}>
                {t.label}
                <Show when={t.desc}>
                  <span class={styles.toggleSub}>({t.desc})</span>
                </Show>
              </label>
            </div>
          )}
        </For>
      </div>

      <h3 class={styles.sectionTitle}>Accessibility</h3>
      <div class={styles.toggleRow}>
        <input
          type="checkbox"
          id="colorblind-mode"
          checked={settingsState.appearance.colorblindMode}
          onChange={(e) => setColorblindMode(e.currentTarget.checked)}
        />
        <label class={styles.toggleLabel} for="colorblind-mode">
          Colorblind-safe mode
          <span class={styles.toggleSub}>(Okabe-Ito palette + shape encoding)</span>
        </label>
      </div>
      <div style={{ display: "flex", gap: "8px", "align-items": "center", "margin-top": "12px", "flex-wrap": "wrap" }}>
        <PriorityBadge priority="p0" />
        <PriorityBadge priority="p1" />
        <PriorityBadge priority="p2" />
        <PriorityBadge priority="p3" />
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Section: Notifications
// ---------------------------------------------------------------------------

const ATTENTION_LEVELS: { value: AttentionLevel; label: string; desc: string }[] = [
  { value: "p0", label: "P0 only", desc: "Critical issues only" },
  { value: "p0p1", label: "P0 + P1", desc: "Critical and high priority" },
  { value: "all", label: "All", desc: "All priority levels" },
];

const NotificationsPanel: Component = () => {
  return (
    <div>
      <h3 class={styles.sectionTitle}>Notifications</h3>
      <div class={styles.formGroup}>
        <label class={styles.label} for="attention-level">Attention level threshold</label>
        <select
          id="attention-level"
          class={styles.input}
          value={settingsState.notifications.attentionLevel}
          onChange={(e) => setAttentionLevel(e.currentTarget.value as AttentionLevel)}
        >
          <For each={ATTENTION_LEVELS}>
            {(level) => (
              <option value={level.value}>
                {level.label} — {level.desc}
              </option>
            )}
          </For>
        </select>
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Section: Agent Defaults
// ---------------------------------------------------------------------------

const KNOWN_HARNESS_TYPES = [
  { value: "claude", label: "Claude Code" },
  { value: "opencode", label: "OpenCode" },
  { value: "goose", label: "Goose" },
  { value: "gemini", label: "Gemini CLI" },
];

const AgentDefaultsPanel: Component = () => {
  const harnesses = () => settingsState.agentDefaults.harnesses ?? [];
  const enabledHarnesses = () => harnesses().filter((h) => h.enabled);

  const addHarness = (adapterType: string) => {
    const existing = harnesses();
    if (existing.some((h) => h.adapterType === adapterType)) return;
    const known = KNOWN_HARNESS_TYPES.find((k) => k.value === adapterType);
    const entry: HarnessEntry = {
      adapterType,
      label: known?.label ?? adapterType,
      enabled: true,
    };
    setHarnesses([...existing, entry]);
  };

  const removeHarness = (adapterType: string) => {
    setHarnesses(harnesses().filter((h) => h.adapterType !== adapterType));
    // If the removed harness was the default, switch to the first enabled one.
    if (settingsState.agentDefaults.adapter === adapterType) {
      const first = harnesses().find((h) => h.enabled && h.adapterType !== adapterType);
      if (first) setAgentAdapter(first.adapterType);
    }
  };

  const toggleHarness = (adapterType: string) => {
    setHarnesses(
      harnesses().map((h) =>
        h.adapterType === adapterType ? { ...h, enabled: !h.enabled } : h,
      ),
    );
  };

  const setCommand = (adapterType: string, command: string) => {
    setHarnesses(
      harnesses().map((h) =>
        h.adapterType === adapterType
          ? { ...h, command: command || undefined }
          : h,
      ),
    );
  };

  const addableTypes = () =>
    KNOWN_HARNESS_TYPES.filter(
      (k) => !harnesses().some((h) => h.adapterType === k.value),
    );

  return (
    <div>
      <h3 class={styles.sectionTitle}>Agent Defaults</h3>

      <div class={styles.formGroup}>
        <label class={styles.label} for="agent-timeout">Default timeout (minutes)</label>
        <input
          id="agent-timeout"
          class={styles.input}
          type="number"
          min="1"
          max="480"
          value={settingsState.agentDefaults.timeoutMinutes}
          onChange={(e) => setAgentTimeout(Number(e.currentTarget.value))}
          style={{ "max-width": "160px" }}
        />
      </div>

      <div class={styles.formGroup}>
        <label class={styles.label} for="agent-adapter">Default harness</label>
        <select
          id="agent-adapter"
          class={styles.input}
          value={settingsState.agentDefaults.adapter}
          onChange={(e) => setAgentAdapter(e.currentTarget.value)}
        >
          <For each={enabledHarnesses()}>
            {(h) => <option value={h.adapterType}>{h.label}</option>}
          </For>
        </select>
      </div>

      <h4 class={styles.label} style={{ "margin-top": "20px", "margin-bottom": "8px" }}>
        Configured harnesses
      </h4>

      <For each={harnesses()}>
        {(h) => (
          <div class={styles.harnessRow}>
            <label class={styles.checkboxLabel}>
              <input
                type="checkbox"
                checked={h.enabled}
                onChange={() => toggleHarness(h.adapterType)}
              />
              <span class={styles.harnessLabel}>{h.label}</span>
              <span class={styles.harnessType}>{h.adapterType}</span>
            </label>
            <input
              class={styles.input}
              type="text"
              placeholder="Custom binary path (optional)"
              value={h.command ?? ""}
              onChange={(e) => setCommand(h.adapterType, e.currentTarget.value)}
              style={{ flex: "1", "min-width": "0", "font-size": "0.8rem" }}
            />
            <button
              class={styles.removeBtn}
              onClick={() => removeHarness(h.adapterType)}
              title="Remove harness"
            >
              &times;
            </button>
          </div>
        )}
      </For>

      <Show when={addableTypes().length > 0}>
        <div class={styles.formGroup} style={{ "margin-top": "10px" }}>
          <select
            class={styles.input}
            onChange={(e) => {
              const val = e.currentTarget.value;
              if (val) {
                addHarness(val);
                e.currentTarget.value = "";
              }
            }}
            style={{ "max-width": "220px" }}
          >
            <option value="">Add harness…</option>
            <For each={addableTypes()}>
              {(k) => <option value={k.value}>{k.label}</option>}
            </For>
          </select>
        </div>
      </Show>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Section: Integrations overview
// ---------------------------------------------------------------------------

const IntegrationsPanel: Component<{ onSelect: (id: SectionId) => void }> = (props) => (
  <div>
    <h3 class={styles.sectionTitle}>Integrations</h3>
    <p class={styles.oauthDescription}>
      Connect Jira and GitHub. Import from the board when Jira or GitHub is connected.
    </p>
    <div class={styles.integrationsList}>
      <button class={styles.integrationCard} onClick={() => props.onSelect("jira")}>
        <span class={styles.integrationIcon}><TbOutlineSquareRotated size={16} /></span>
        <div>
          <span class={styles.integrationName}>Jira</span>
          <span class={styles.integrationStatus}>
            {settingsState.jiraConfig.connected ? "Connected" : "Not connected"}
          </span>
        </div>
      </button>
      <button class={styles.integrationCard} onClick={() => props.onSelect("github")}>
        <span class={styles.integrationIcon}><TbOutlineBrandGithub size={16} /></span>
        <div>
          <span class={styles.integrationName}>GitHub</span>
          <span class={styles.integrationStatus}>
            {settingsState.githubConfig.connected ? "Connected" : "Not connected"}
          </span>
        </div>
      </button>
    </div>
  </div>
);

// ---------------------------------------------------------------------------
// Section: Jira
// ---------------------------------------------------------------------------

const JiraPanel: Component<{ onBack: () => void }> = (props) => {
  const isConnected = () => settingsState.jiraConfig.connected;
  const [connecting, setConnecting] = createSignal(false);

  // Match GitHub: verify keychain-backed session when this panel is shown.
  fetchJiraStatus();

  const handleConnect = async () => {
    setConnecting(true);
    await connectJira();
    setConnecting(false);
  };

  return (
    <div>
      <button class={styles.backBtn} onClick={props.onBack}><TbOutlineArrowLeft size={14} style={{ "vertical-align": "middle" }} /> Integrations</button>
      <h3 class={styles.sectionTitle}>Jira</h3>

      <Show when={!isConnected()}>
        <div class={styles.oauthSection}>
          <p class={styles.oauthDescription}>
            Sign in with Atlassian to connect your Jira site.
          </p>
          <div class={styles.connectedRow}>
            <span class={`${styles.statusBadge} ${styles.statusIdle}`}>
              <span class={styles.statusDot} />
              Not connected
            </span>
          </div>
          <div class={styles.buttonRow}>
            <button
              class={styles.btnPrimary}
              disabled={connecting()}
              onClick={handleConnect}
            >
              {connecting() ? "Opening Jira..." : "Connect Jira"}
            </button>
          </div>
          <Show when={settingsState.jiraConfig.lastError}>
            <p class={styles.errorMsg}>{settingsState.jiraConfig.lastError}</p>
          </Show>
        </div>
      </Show>

      <Show when={isConnected()}>
        <div class={styles.connectedSection}>
          <div class={styles.connectedRow}>
            <span class={`${styles.statusBadge} ${styles.statusConnected}`}>
              <span class={styles.statusDot} />
              Connected
            </span>
            <Show when={settingsState.jiraConfig.siteName}>
              <span class={styles.siteName}>{settingsState.jiraConfig.siteName}</span>
            </Show>
          </div>
          <Show when={settingsState.jiraConfig.baseUrl}>
            <p class={styles.siteUrl}>{settingsState.jiraConfig.baseUrl}</p>
          </Show>
          <div class={styles.buttonRow}>
            <button class={styles.btnDanger} onClick={() => disconnectJira()}>
              Disconnect
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Section: GitHub
// ---------------------------------------------------------------------------

const GitHubPanel: Component<{ onBack: () => void }> = (props) => {
  const isConnected = () => settingsState.githubConfig.connected;
  const [connecting, setConnecting] = createSignal(false);

  // Check current status when the panel mounts
  fetchGithubStatus();

  const handleConnect = async () => {
    setConnecting(true);
    await connectGitHub();
    setConnecting(false);
  };

  return (
    <div>
      <button class={styles.backBtn} onClick={props.onBack}>
        <TbOutlineArrowLeft size={14} style={{ "vertical-align": "middle" }} /> Integrations
      </button>
      <h3 class={styles.sectionTitle}>GitHub</h3>

      <Show when={!isConnected()}>
        <div class={styles.oauthSection}>
          <p class={styles.oauthDescription}>
            Sign in with GitHub to enable issue import from the workboard. In the browser, use{" "}
            <strong>Finish in browser</strong> on the oauth page if the app does not open.
          </p>
          <div class={styles.connectedRow}>
            <span class={`${styles.statusBadge} ${styles.statusIdle}`}>
              <span class={styles.statusDot} />
              Not connected
            </span>
          </div>
          <div class={styles.buttonRow}>
            <button
              class={styles.btnPrimary}
              disabled={connecting()}
              onClick={handleConnect}
            >
              {connecting() ? "Opening GitHub..." : "Connect GitHub"}
            </button>
          </div>
          <Show when={settingsState.githubConfig.lastError}>
            <p class={styles.errorMsg}>{settingsState.githubConfig.lastError}</p>
          </Show>
        </div>
      </Show>

      <Show when={isConnected()}>
        <div class={styles.connectedSection}>
          <div class={styles.connectedRow}>
            <span class={`${styles.statusBadge} ${styles.statusConnected}`}>
              <span class={styles.statusDot} />
              Connected
            </span>
            <Show when={settingsState.githubConfig.owner}>
              <span class={styles.siteName}>{settingsState.githubConfig.owner}</span>
            </Show>
          </div>
          <div class={styles.buttonRow}>
            <button class={styles.btnDanger} onClick={() => disconnectGitHub()}>
              Disconnect
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
};

// ---------------------------------------------------------------------------
// SettingsView
// ---------------------------------------------------------------------------

const NAV_ITEMS: { id: SectionId; label: string; icon: () => JSX.Element }[] = [
  { id: "appearance", label: "Appearance", icon: () => <TbOutlinePalette size={16} /> },
  { id: "notifications", label: "Notifications", icon: () => <TbOutlineBell size={16} /> },
  { id: "agent-defaults", label: "Agent Defaults", icon: () => <TbOutlineRobot size={16} /> },
  { id: "integrations", label: "Integrations", icon: () => <TbOutlinePlug size={16} /> },
  { id: "audit-log", label: "Audit Log", icon: () => <TbOutlineClipboardList size={16} /> },
];

const SettingsView: Component = () => {
  const [activeSection, setActiveSection] = createSignal<SectionId>("appearance");

  // For integrations sub-pages, highlight the Integrations nav item
  const isIntegrationSection = (id: SectionId) =>
    id === "integrations" || id === "jira" || id === "github";

  const navActiveFor = (navId: SectionId) => {
    const current = activeSection();
    if (navId === "integrations") return isIntegrationSection(current);
    return current === navId;
  };

  return (
    <div class={styles.container}>
      <h2 class={styles.title}>Settings</h2>

      <div class={styles.splitLayout}>
        {/* Left nav */}
        <nav class={styles.navList}>
          <For each={NAV_ITEMS}>
            {(item) => (
              <button
                class={`${styles.navItem}${navActiveFor(item.id) ? ` ${styles.navItemActive}` : ""}`}
                onClick={() => setActiveSection(item.id)}
              >
                <span class={styles.navIcon}>{item.icon()}</span>
                <span>{item.label}</span>
              </button>
            )}
          </For>
        </nav>

        {/* Right detail */}
        <div class={styles.detail}>
          <Show when={activeSection() === "appearance"}>
            <AppearancePanel />
          </Show>
          <Show when={activeSection() === "notifications"}>
            <NotificationsPanel />
          </Show>
          <Show when={activeSection() === "agent-defaults"}>
            <AgentDefaultsPanel />
          </Show>
          <Show when={activeSection() === "integrations"}>
            <IntegrationsPanel onSelect={setActiveSection} />
          </Show>
          <Show when={activeSection() === "jira"}>
            <JiraPanel onBack={() => setActiveSection("integrations")} />
          </Show>
          <Show when={activeSection() === "github"}>
            <GitHubPanel onBack={() => setActiveSection("integrations")} />
          </Show>
          <Show when={activeSection() === "audit-log"}>
            <AuditLog />
          </Show>
        </div>
      </div>
    </div>
  );
};

export default SettingsView;
