/**
 * SettingsView — main settings page with left sidebar nav + right detail panel.
 *
 * Sections: Appearance, Notifications, Agent Defaults, Integrations.
 * Route: /settings
 */

import type { JSX } from "solid-js";
import { createSignal, onMount, Show, For, type Component } from "solid-js";
import { open as openNativeDialog } from "@tauri-apps/plugin-dialog";
import {
  TbOutlinePalette,
  TbOutlinePlug,
  TbOutlineSquareRotated,
  TbOutlineBrandGithub,
  TbOutlineArrowLeft,
  TbOutlineBell,
  TbOutlineRobot,
  TbOutlineClipboardList,
  TbOutlineFolders,
} from "solid-icons/tb";
import {
  settingsState,
  initiateOAuth,
  connectJira,
  disconnectJira,
  connectGitHub,
  disconnectGitHub,
  fetchGithubStatus,
  setTheme,
  setColorblindMode,
  setAttentionLevel,
  setAgentTimeout,
  setAgentAdapter,
} from "./settingsStore";
import type { Theme, AttentionLevel, AgentAdapter } from "./settingsStore";
import {
  projectState,
  loadProjects,
  createProject,
  switchProject,
} from "../../stores/projectStore";
import GitHubImport from "./GitHubImport";
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
  | "projects"
  | "integrations"
  | "jira"
  | "github"
  | "audit-log";

function isTauriShell(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

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

const ADAPTER_OPTIONS: { value: AgentAdapter; label: string }[] = [
  { value: "claude-code", label: "Claude Code" },
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
// Section: Projects
// ---------------------------------------------------------------------------

const ProjectsPanel: Component = () => {
  const [name, setName] = createSignal("");
  const [repoPath, setRepoPath] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);
  const [formError, setFormError] = createSignal<string | null>(null);

  onMount(() => {
    void loadProjects();
  });

  const pickFolder = async () => {
    if (!isTauriShell()) {
      return;
    }
    try {
      // Static import: a dynamic `import()` before `open()` yields and can drop the
      // user-activation chain, so WebKit/macOS may refuse to show the folder picker.
      const chosen = await openNativeDialog({
        directory: true,
        multiple: false,
        title: "Choose Git repository folder",
      });
      if (typeof chosen === "string") {
        setRepoPath(chosen);
      }
    } catch (err) {
      console.error("folder dialog failed", err);
      setFormError(
        "Could not open the folder picker. Enter the repository path manually.",
      );
    }
  };

  const onSubmit = async (e: Event) => {
    e.preventDefault();
    setFormError(null);
    setSubmitting(true);
    const result = await createProject(name(), repoPath());
    setSubmitting(false);
    if (result.ok) {
      setName("");
      setRepoPath("");
      await loadProjects();
    } else {
      setFormError(result.error);
    }
  };

  return (
    <div>
      <h3 class={styles.sectionTitle}>Projects</h3>
      <p class={styles.oauthDescription}>
        Add a project with a display name and the path to its Git repository on disk.
        In the desktop app you can choose a folder; in the browser, enter the path manually.
      </p>

      <form class={styles.formGroup} onSubmit={onSubmit}>
        <label class={styles.label} for="project-name">Name</label>
        <input
          id="project-name"
          class={styles.input}
          type="text"
          placeholder="My app"
          value={name()}
          onInput={(e) => setName(e.currentTarget.value)}
          autocomplete="off"
        />
        <label class={styles.label} for="project-repo-path">Repository path</label>
        <div style={{ display: "flex", gap: "8px", "flex-wrap": "wrap", "align-items": "center" }}>
          <input
            id="project-repo-path"
            class={styles.input}
            type="text"
            placeholder="/path/to/repo"
            value={repoPath()}
            onInput={(e) => setRepoPath(e.currentTarget.value)}
            autocomplete="off"
            style={{ flex: "1", "min-width": "200px" }}
          />
          <Show when={isTauriShell()}>
            <button type="button" class={styles.btnPrimary} onClick={() => void pickFolder()}>
              Choose folder…
            </button>
          </Show>
        </div>
        <div class={styles.buttonRow} style={{ "margin-top": "12px" }}>
          <button type="submit" class={styles.btnPrimary} disabled={submitting()}>
            {submitting() ? "Adding…" : "Add project"}
          </button>
        </div>
        <Show when={formError()}>
          <p class={styles.errorMsg}>{formError()}</p>
        </Show>
      </form>

      <h3 class={styles.sectionTitle} style={{ "margin-top": "24px" }}>Existing projects</h3>
      <Show
        when={projectState.loaded && projectState.projects.length > 0}
        fallback={
          <p class={styles.oauthDescription}>
            {projectState.loaded ? "No projects yet." : "Loading projects…"}
          </p>
        }
      >
        <ul class={styles.integrationsList} style={{ "flex-direction": "column", gap: "6px" }}>
          <For each={projectState.projects}>
            {(p) => (
              <li>
                <button
                  type="button"
                  class={styles.integrationCard}
                  style={{ width: "100%", "text-align": "left" }}
                  onClick={() => switchProject(p.id)}
                >
                  <span class={styles.integrationName}>{p.name}</span>
                  <span class={styles.integrationStatus}>{p.repo_path}</span>
                  <Show when={projectState.activeProjectId === p.id}>
                    <span class={styles.statusBadge} style={{ "margin-left": "8px" }}>Active</span>
                  </Show>
                </button>
              </li>
            )}
          </For>
        </ul>
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
  const [connecting, setConnecting] = createSignal(false);

  const handleConnect = async () => {
    setConnecting(true);
    await connectJira();
    setConnecting(false);
  };

  return (
    <div>
      <button class={styles.backBtn} onClick={props.onBack}><TbOutlineArrowLeft size={14} style={{ "vertical-align": "middle" }} /> Integrations</button>
      <h3 class={styles.sectionTitle}>Jira Integration</h3>

      <Show when={!isConnected()}>
        <div class={styles.oauthSection}>
          <p class={styles.oauthDescription}>
            Connect your Atlassian account via OAuth to import Jira issues.
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
// Section: GitHub Integration
// ---------------------------------------------------------------------------

const GitHubPanel: Component<{ onBack: () => void }> = (props) => {
  const isConnected = () => settingsState.githubConfig.connected;
  const [importOpen, setImportOpen] = createSignal(false);
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
      <h3 class={styles.sectionTitle}>GitHub Integration</h3>

      <Show when={!isConnected()}>
        <div class={styles.oauthSection}>
          <p class={styles.oauthDescription}>
            Connect your GitHub account via OAuth to import issues and pull requests. The server
            needs <code>MOLTHUB_GITHUB_CLIENT_SECRET</code> or <code>GITHUB_CLIENT_SECRET</code> in
            the environment for the token step. After authorizing in the browser, use{" "}
            <strong>Finish in browser (local API)</strong> on the bridge page if you are not using
            the desktop app.
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
  { id: "projects", label: "Projects", icon: () => <TbOutlineFolders size={16} /> },
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
          <Show when={activeSection() === "projects"}>
            <ProjectsPanel />
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
