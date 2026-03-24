/**
 * AgentDetailView — split-pane detail page for a single running agent.
 *
 * Left pane: terminal-style output stream.
 * Right pane: task metadata, stage history, action buttons.
 *
 * Route: /agents/:id
 */

import type { Component } from "solid-js";
import { Show, onCleanup, createSignal } from "solid-js";
import { useParams } from "@solidjs/router";
import { TbOutlineArrowLeft } from "solid-icons/tb";
import { getAgent, setupAgentSubscription } from "./agentStore";
import OutputStream from "./OutputStream";
import AgentMeta from "./AgentMeta";
import SteerChat from "./SteerChat";
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

type LeftTab = "output" | "chat";

const AgentDetailView: Component = () => {
  const params = useParams<{ id: string }>();

  const agent = () => getAgent(params.id);
  const [leftTab, setLeftTab] = createSignal<LeftTab>("output");

  // Wire up WebSocket subscription for real-time output
  const unsub = setupAgentSubscription(params.id);
  onCleanup(unsub);

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
            {/* Left — output stream / steering chat */}
            <div class={styles.leftPane}>
              <div class={styles.tabBar}>
                <button
                  class={`${styles.tab} ${leftTab() === "output" ? styles.tabActive : ""}`}
                  onClick={() => setLeftTab("output")}
                  type="button"
                >
                  Output
                </button>
                <button
                  class={`${styles.tab} ${leftTab() === "chat" ? styles.tabActive : ""}`}
                  onClick={() => setLeftTab("chat")}
                  type="button"
                >
                  Chat
                </button>
              </div>
              <Show when={leftTab() === "output"}>
                <OutputStream
                  lines={a().outputLines}
                  status={a().status}
                />
              </Show>
              <Show when={leftTab() === "chat"}>
                <SteerChat agentId={a().id} />
              </Show>
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
