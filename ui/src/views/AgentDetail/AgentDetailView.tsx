/**
 * AgentDetailView — split-pane detail page for a single running agent.
 *
 * Left pane: terminal-style output stream with steering input pinned at bottom.
 * Right pane: task metadata, stage history, action buttons.
 *
 * Route: /agents/:id
 */

import type { Component } from "solid-js";
import { Show, For, onCleanup, createSignal } from "solid-js";
import { useParams } from "@solidjs/router";
import { TbOutlineArrowLeft } from "solid-icons/tb";
import { getAgent, setupAgentSubscription, registerAgentPlaceholder, fetchAgents, clearAuthError, hydrateAgentOutput } from "./agentStore";
import type { ToolCallEntry } from "./agentStore";
import { hydrateMessages } from "./steerStore";
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

  // Ensure agent row exists immediately (synchronous placeholder so Show renders)
  registerAgentPlaceholder(params.id);
  // Kick off a fresh agent list fetch so status/name populate without waiting for next poll
  void fetchAgents();
  // Subscribe to live output (idempotent — safe even if fetchAgents also subscribes)
  const unsub = setupAgentSubscription(params.id);
  onCleanup(unsub);
  // Hydrate buffered output from the server
  void hydrateAgentOutput(params.id);
  // Hydrate persisted steer messages from the server
  void hydrateMessages(params.id);

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
              <AgentChat agentId={a().id} status={a().status} />
              <Show when={(a().fileDiffs?.length ?? 0) > 0}>
                <div class={styles.diffPanel}>
                  <div class={styles.diffPanelTitle}>Changed Files</div>
                  <For each={a().fileDiffs ?? []}>
                    {(diff) => (
                      <details class={styles.diffFile}>
                        <summary class={styles.diffFilePath}>{diff.path}</summary>
                        <pre class={styles.diffContent}>{diff.unifiedDiff}</pre>
                      </details>
                    )}
                  </For>
                </div>
              </Show>
            </div>

            <div class={styles.divider} />

            {/* Right — metadata + actions */}
            <div class={styles.rightPane}>
              <AgentMeta agent={a()} />
              <Show when={(a().toolCalls?.length ?? 0) > 0}>
                <div class={styles.toolCallLog}>
                  <div class={styles.toolCallLogTitle}>Tool Calls</div>
                  <For each={a().toolCalls ?? []}>
                    {(tc: ToolCallEntry) => (
                      <details class={styles.toolCallEntry}>
                        <summary class={styles.toolCallSummary}>
                          <span class={tc.isError ? styles.toolCallError : tc.completedAt ? styles.toolCallDone : styles.toolCallPending}>
                            {tc.completedAt ? (tc.isError ? "✗" : "✓") : "⋯"}
                          </span>
                          <span class={styles.toolCallName}>{tc.toolName}</span>
                        </summary>
                        <Show when={tc.input !== undefined && tc.input !== null}>
                          <pre class={styles.toolCallDetail}>{JSON.stringify(tc.input, null, 2)}</pre>
                        </Show>
                        <Show when={tc.completedAt && tc.output !== undefined}>
                          <pre class={styles.toolCallDetail}>{JSON.stringify(tc.output, null, 2)}</pre>
                        </Show>
                      </details>
                    )}
                  </For>
                </div>
              </Show>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
};

export default AgentDetailView;
