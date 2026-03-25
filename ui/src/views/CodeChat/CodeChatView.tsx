/**
 * CodeChatView — project-scoped Claude Code session inside Molt Hub.
 *
 * Spawns a long-lived agent with the Claude CLI adapter (same harness as task agents),
 * streams stdout to the Output panel, and uses the steering API for follow-up prompts.
 */

import {
  Show,
  createEffect,
  createSignal,
  onCleanup,
  onMount,
  type Component,
} from "solid-js";
import { TbOutlineArrowLeft } from "solid-icons/tb";
import { api } from "../../lib/api";
import {
  clearAuthError,
  fetchAgents,
  getAgent,
  hydrateAgentOutput,
  registerAgentPlaceholder,
  removeAgentFromStore,
  setupAgentSubscription,
  startAgentPolling,
} from "../AgentDetail/agentStore";
import OutputStream from "../AgentDetail/OutputStream";
import SteerChat from "../AgentDetail/SteerChat";
import { clearMessages } from "../AgentDetail/steerStore";
import adStyles from "../AgentDetail/AgentDetailView.module.css";
import styles from "./CodeChatView.module.css";

const STORAGE_KEY = "molt:project-code-chat-agent-id";

const BOOTSTRAP_INSTRUCTIONS = `You are Claude Code running inside Molt Hub. The working directory is the user's project repository.

Give a very brief ready message (a few sentences at most), then stop and wait. The user will send follow-up instructions through Molt Hub's steering channel. When they message you, help with coding, exploration, and changes in this repo.`;

function loadStoredAgentId(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function saveStoredAgentId(id: string): void {
  try {
    localStorage.setItem(STORAGE_KEY, id);
  } catch {
    /* ignore */
  }
}

function clearStoredAgentId(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore */
  }
}

type LeftTab = "output" | "chat";

const CodeChatView: Component = () => {
  const [sessionAgentId, setSessionAgentId] = createSignal<string | null>(null);
  const [repoPath, setRepoPath] = createSignal<string>("");
  const [repoLabel, setRepoLabel] = createSignal<string>("");
  const [projectsError, setProjectsError] = createSignal<string | null>(null);
  const [startError, setStartError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [leftTab, setLeftTab] = createSignal<LeftTab>("output");
  const [ready, setReady] = createSignal(false);

  const agent = () => {
    const id = sessionAgentId();
    return id ? getAgent(id) : undefined;
  };

  async function loadProjects(): Promise<void> {
    setProjectsError(null);
    try {
      const res = await api.listProjects();
      const projects = res.projects ?? [];
      const first = projects[0];
      if (first?.repo_path) {
        setRepoPath(first.repo_path);
        setRepoLabel(`${first.name} (${first.repo_path})`);
      } else {
        setProjectsError(
          "No project with a repository path found. Register a project (server projects config) so Claude Code has a working directory.",
        );
      }
    } catch (e) {
      setProjectsError(
        e instanceof Error ? e.message : "Could not load projects from the API.",
      );
    }
  }

  async function tryResumeSession(): Promise<void> {
    const stored = loadStoredAgentId();
    if (!stored) {
      setReady(true);
      return;
    }
    try {
      const data = await api.getAgents();
      const hit = (data.agents ?? []).find((a) => a.agent_id === stored);
      if (!hit) {
        clearStoredAgentId();
        setReady(true);
        return;
      }
      registerAgentPlaceholder(stored, { taskName: "Project chat" });
      await fetchAgents();
      await hydrateAgentOutput(stored);
      setSessionAgentId(stored);
    } catch {
      /* API unreachable — keep stored id for next visit */
    } finally {
      setReady(true);
    }
  }

  onMount(() => {
    const stopPoll = startAgentPolling(4000);
    void loadProjects().then(() => tryResumeSession());
    onCleanup(stopPoll);
  });

  createEffect(() => {
    const id = sessionAgentId();
    if (!id) return;
    const unsub = setupAgentSubscription(id);
    onCleanup(unsub);
  });

  createEffect(() => {
    const id = sessionAgentId();
    if (!id) return;
    const a = getAgent(id);
    if (a?.status === "terminated") {
      clearStoredAgentId();
      clearMessages(id);
      setSessionAgentId(null);
    }
  });

  const startSession = async () => {
    const wd = repoPath().trim();
    if (!wd) {
      setStartError("Pick a project with a repository path first.");
      return;
    }
    setBusy(true);
    setStartError(null);
    try {
      const res = await api.spawnAgent({
        instructions: BOOTSTRAP_INSTRUCTIONS,
        workingDir: wd,
        adapterType: "claude",
      });
      const id = res.agentId;
      if (!id) throw new Error("Server did not return agentId");
      saveStoredAgentId(id);
      clearMessages(id);
      registerAgentPlaceholder(id, { taskName: "Project chat" });
      await fetchAgents();
      await hydrateAgentOutput(id);
      setSessionAgentId(id);
      setLeftTab("output");
    } catch (e) {
      setStartError(
        e instanceof Error ? e.message : "Failed to start Claude Code session.",
      );
    } finally {
      setBusy(false);
    }
  };

  const endSession = async () => {
    const id = sessionAgentId();
    if (!id) return;
    setBusy(true);
    try {
      await api.terminateAgent(id);
    } catch {
      /* best-effort */
    }
    clearStoredAgentId();
    clearMessages(id);
    removeAgentFromStore(id);
    setSessionAgentId(null);
    await fetchAgents();
    setBusy(false);
  };

  return (
    <div class={adStyles.container}>
      <div class={adStyles.header}>
        <a href="/boards" class={adStyles.backBtn}>
          <TbOutlineArrowLeft size={14} style={{ "vertical-align": "middle" }} /> Home
        </a>
        <span class={adStyles.agentName}>Claude Code</span>
        <span class={adStyles.stagePill}>Project chat</span>
        <Show when={sessionAgentId()}>
          <button
            type="button"
            class={styles.endBtn}
            disabled={busy()}
            onClick={() => void endSession()}
          >
            End session
          </button>
        </Show>
      </div>

      {/* Auth error banner */}
      <Show when={agent()?.authError}>
        {(_err) => {
          const [loggingIn, setLoggingIn] = createSignal(false);
          const [loginErr, setLoginErr] = createSignal<string>();
          const handleLogin = async () => {
            setLoggingIn(true);
            setLoginErr(undefined);
            try {
              await api.loginAgent();
              const id = sessionAgentId();
              if (id) clearAuthError(id);
            } catch (e: unknown) {
              const raw = e instanceof Error ? e.message : String(e);
              const dashIdx = raw.indexOf(" — ");
              setLoginErr(dashIdx >= 0 ? raw.slice(dashIdx + 3) : raw);
            } finally {
              setLoggingIn(false);
            }
          };
          return (
            <div class={adStyles.authErrorBanner}>
              <span>Session expired — re-authenticate to continue.</span>
              <button
                class={adStyles.loginBtn}
                disabled={loggingIn()}
                onClick={() => void handleLogin()}
              >
                {loggingIn() ? "Logging in\u2026" : "Login"}
              </button>
              <Show when={loginErr()}>
                <span class={adStyles.loginError}>{loginErr()}</span>
              </Show>
            </div>
          );
        }}
      </Show>

      <Show when={!ready()}>
        <div class={styles.loadingBanner}>Loading…</div>
      </Show>

      <Show when={ready() && !sessionAgentId()}>
        <div class={styles.landing}>
          <h2 class={styles.landingTitle}>Claude Code in Molt Hub</h2>
          <p class={styles.landingBody}>
            Start a session to run the Claude CLI in your project directory. Streamed output
            appears in the Output tab; use Chat to send follow-up instructions (same steering
            channel as task agents).
          </p>
          <Show when={projectsError()}>
            {(msg) => <p class={styles.errorText}>{msg()}</p>}
          </Show>
          <Show when={repoPath() && !projectsError()}>
            <p class={styles.repoLine}>
              <span class={styles.repoLabel}>Working directory</span>
              <span class={styles.repoPath}>{repoLabel() || repoPath()}</span>
            </p>
          </Show>
          <Show when={startError()}>
            {(msg) => <p class={styles.errorText}>{msg()}</p>}
          </Show>
          <button
            type="button"
            class={styles.startBtn}
            disabled={busy() || !repoPath().trim() || !!projectsError()}
            onClick={() => void startSession()}
          >
            {busy() ? "Starting…" : "Start Claude Code session"}
          </button>
        </div>
      </Show>

      <Show when={sessionAgentId() && agent()}>
        <div class={adStyles.body}>
          <div class={adStyles.leftPane}>
            <div class={adStyles.tabBar}>
              <button
                type="button"
                class={`${adStyles.tab} ${leftTab() === "output" ? adStyles.tabActive : ""}`}
                onClick={() => setLeftTab("output")}
              >
                Output
              </button>
              <button
                type="button"
                class={`${adStyles.tab} ${leftTab() === "chat" ? adStyles.tabActive : ""}`}
                onClick={() => setLeftTab("chat")}
              >
                Chat
              </button>
            </div>
            <Show when={leftTab() === "output"}>
              <OutputStream
                lines={getAgent(sessionAgentId()!)?.outputLines ?? []}
                status={getAgent(sessionAgentId()!)?.status ?? "idle"}
              />
            </Show>
            <Show when={leftTab() === "chat"}>
              <SteerChat agentId={sessionAgentId()!} />
            </Show>
          </div>
          <div class={adStyles.divider} />
          <div class={adStyles.rightPane}>
            <section class={styles.sideSection}>
              <h3 class={styles.sideTitle}>Session</h3>
              <p class={styles.sideMuted}>
                Agent id <code class={styles.mono}>{sessionAgentId()}</code>
              </p>
              <p class={styles.sideMuted}>
                This is the same Claude CLI harness as agents on the Agents page. Open{" "}
                <a
                  class={styles.inlineLink}
                  href={`/agents/${sessionAgentId()!}`}
                >
                  full agent detail
                </a>{" "}
                for pause, approve, and metadata.
              </p>
            </section>
            <section class={styles.sideSection}>
              <h3 class={styles.sideTitle}>Repository</h3>
              <p class={styles.sideMuted}>{repoPath() || "—"}</p>
            </section>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default CodeChatView;
