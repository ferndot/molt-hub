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
} from "solid-icons/tb";
import {
  settingsState,
  initiateOAuth,
  disconnectJira,
  connectGitHub,
  disconnectGitHub,
  setTheme,
  setColorblindMode,
  setAttentionLevel,
  setAgentTimeout,
  setAgentAdapter,
} from "./settingsStore";
import type { Theme, AttentionLevel, AgentAdapter } from "./settingsStore";
import GitHubImport from "./GitHubImport";
import styles from "./Settings.module.css";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type SectionId = "appearance" | "notifications" | "agent-defaults" | "integrations" | "jira" | "github";

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

const ADAPTER_OPTIONS: { value: AgentAdapter; label: string }[] = [
  { value: "claude-code", label: "Claude Code" },
  { value: "mock", label: "Mock" },
];

const AgentDefaultsPanel: Component = () => {
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
        <label class={styles.label} for="agent-adapter">Default adapter</label>
        <select
          id="agent-adapter"
          class={styles.input}
          value={settingsState.agentDefaults.adapter}
          onChange={(e) => setAgentAdapter(e.currentTarget.value as AgentAdapter)}
        >
          <For each={ADAPTER_OPTIONS}>
            {(opt) => (
              <option value={opt.value}>{opt.label}</option>
            )}
          </For>
        </select>
      </div>
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
      Connect external services to import issues and sync data.
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
// Section: Jira Integration
// ---------------------------------------------------------------------------

const JiraPanel: Component<{ onBack: () => void }> = (props) => {
  const isConnected = () => settingsState.jiraConfig.connected;

  return (
    <div>
      <button class={styles.backBtn} onClick={props.onBack}><TbOutlineArrowLeft size={14} style={{ "vertical-align": "middle" }} /> Integrations</button>
      <h3 class={styles.sectionTitle}>Jira Integration</h3>

      <Show when={!isConnected()}>
        <div class={styles.oauthSection}>
          <p class={styles.oauthDescription}>
            Connect your Atlassian account to import issues and sync with Jira.
          </p>
          <div class={styles.buttonRow}>
            <button class={styles.btnPrimary} onClick={() => initiateOAuth()}>
              Connect to Jira
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
// Section: GitHub Integration
// ---------------------------------------------------------------------------

const GitHubPanel: Component<{ onBack: () => void }> = (props) => {
  const isConnected = () => settingsState.githubConfig.connected;
  const [token, setToken] = createSignal("");
  const [owner, setOwner] = createSignal("");
  const [importOpen, setImportOpen] = createSignal(false);
  const [connecting, setConnecting] = createSignal(false);

  const handleConnect = async () => {
    const t = token().trim();
    const o = owner().trim();
    if (!t || !o) return;
    setConnecting(true);
    await connectGitHub(t, o);
    setConnecting(false);
    if (settingsState.githubConfig.connected) {
      setToken("");
    }
  };

  return (
    <div>
      <button class={styles.backBtn} onClick={props.onBack}>
        <TbOutlineArrowLeft size={14} style={{ "vertical-align": "middle" }} /> Integrations
      </button>
      <h3 class={styles.sectionTitle}>GitHub Integration</h3>

      <Show when={!isConnected()}>
        <div class={styles.oauthSection}>
          <p class={styles.oauthDescription}>
            Connect your GitHub account to import issues and pull requests.
            Provide a personal access token with <code>repo</code> scope.
          </p>
          <div class={styles.formGroup}>
            <label class={styles.label} for="gh-token">Personal Access Token</label>
            <input
              id="gh-token"
              class={styles.input}
              type="password"
              placeholder="ghp_..."
              value={token()}
              onInput={(e) => setToken(e.currentTarget.value)}
            />
          </div>
          <div class={styles.formGroup}>
            <label class={styles.label} for="gh-owner">Owner / Organization</label>
            <input
              id="gh-owner"
              class={styles.input}
              type="text"
              placeholder="my-org or username"
              value={owner()}
              onInput={(e) => setOwner(e.currentTarget.value)}
            />
          </div>
          <div class={styles.buttonRow}>
            <button
              class={styles.btnPrimary}
              disabled={!token().trim() || !owner().trim() || connecting()}
              onClick={handleConnect}
            >
              {connecting() ? "Connecting..." : "Connect"}
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
            <button class={styles.btnPrimary} onClick={() => setImportOpen(true)}>
              Import Issues
            </button>
            <button class={styles.btnDanger} onClick={() => disconnectGitHub()}>
              Disconnect
            </button>
          </div>
        </div>
        <GitHubImport isOpen={importOpen()} onClose={() => setImportOpen(false)} />
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
        </div>
      </div>
    </div>
  );
};

export default SettingsView;
