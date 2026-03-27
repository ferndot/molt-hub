/**
 * AgentDetailView — split-pane detail page for a single running agent.
 *
 * Left pane: terminal-style output stream with steering input pinned at bottom.
 * Right pane: task metadata, stage history, action buttons.
 *
 * Route: /agents/:id
 */

import type { Component } from "solid-js";
import { Show, onCleanup, createSignal } from "solid-js";
import { useParams } from "@solidjs/router";
import { TbOutlineArrowLeft } from "solid-icons/tb";
import { getAgent, setupAgentSubscription, clearAuthError, hydrateAgentOutput } from "./agentStore";
import { api } from "../../lib/api";
import AgentChat from "../../components/AgentChat/AgentChat";
import AgentMeta from "./AgentMeta";
import styles from "./AgentDetailView.module.css";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function duration(isoString: string): string {
  const elapsed = Date.now() - new Date(isoString).getTime();
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return "< 1m";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const rem = minutes % 60;
  return rem > 0 ? `${hours}h ${rem}m` : `${hours}h`;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const AgentDetailView: Component = () => {
  const params = useParams<{ id: string }>();

  const agent = () => getAgent(params.id);

  // Wire up WebSocket subscription for real-time output
  const unsub = setupAgentSubscription(params.id);
  onCleanup(unsub);
  // Fetch buffered output from the server for this agent
  void hydrateAgentOutput(params.id);

  return (
    <Show
      when={agent()}
      fallback={
        <div class={styles.notFound}>
          <p class={styles.notFoundTitle}>Agent not found</p>
          <p class={styles.notFoundSub}>No agent with ID "{params.id}"</p>
        </div>
      }
    >
      {(a) => (
        <div class={styles.container}>
          {/* Auth error banner */}
          <Show when={a().authError}>
            {(_err) => {
              const [loggingIn, setLoggingIn] = createSignal(false);
              const [loginErr, setLoginErr] = createSignal<string>();

              const handleLogin = async () => {
                setLoggingIn(true);
                setLoginErr(undefined);
                try {
                  await api.loginAgent();
                  clearAuthError(params.id);
                } catch (e: unknown) {
                  // Strip the "POST /agents/login failed: 500 — " prefix if present.
                  const raw = e instanceof Error ? e.message : String(e);
                  const dashIdx = raw.indexOf(" — ");
                  setLoginErr(dashIdx >= 0 ? raw.slice(dashIdx + 3) : raw);
                } finally {
                  setLoggingIn(false);
                }
              };

              return (
                <div class={styles.authErrorBanner}>
                  <span>Session expired — re-authenticate to continue.</span>
                  <button
                    class={styles.loginBtn}
                    disabled={loggingIn()}
                    onClick={handleLogin}
                  >
                    {loggingIn() ? "Logging in\u2026" : "Login"}
                  </button>
                  <Show when={loginErr()}>
                    <span class={styles.loginError}>{loginErr()}</span>
                  </Show>
                </div>
              );
            }}
          </Show>

          {/* Header */}
          <div class={styles.header}>
            <a href="/agents" class={styles.backBtn}>
              <TbOutlineArrowLeft size={14} style={{ "vertical-align": "middle" }} /> Agents
            </a>
            <span class={styles.agentName}>{a().name}</span>
            <span class={styles.stagePill}>{a().currentStage}</span>
            <span class={styles.timeRunning}>
              Running {duration(a().assignedAt)}
            </span>
          </div>

          {/* Split-pane body */}
          <div class={styles.body}>
            {/* Left — unified output + steering */}
            <div class={styles.leftPane}>
              <AgentChat agentId={a().id} />
            </div>

            <div class={styles.divider} />

            {/* Right — metadata + actions */}
            <div class={styles.rightPane}>
              <AgentMeta agent={a()} />
            </div>
          </div>
        </div>
      )}
    </Show>
  );
};

export default AgentDetailView;
