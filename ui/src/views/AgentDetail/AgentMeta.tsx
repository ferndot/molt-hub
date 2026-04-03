/**
 * AgentMeta — right panel of the Agent Detail view.
 *
 * Shows task name/description, priority badge, status badge, stage history,
 * timing metadata, and stub action buttons (Pause, Terminate, Reassign).
 */

import type { Component } from "solid-js";
import { For, Show } from "solid-js";
import type { AgentDetail, StageEntry } from "./agentStore";
import type { Priority } from "../../types/domain";
import styles from "./AgentMeta.module.css";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const PRIORITY_LABELS: Record<Priority, string> = {
  p0: "P0 — Critical",
  p1: "P1 — High",
  p2: "P2 — Medium",
  p3: "P3 — Low",
};

const PRIORITY_BADGE_CLASS: Record<Priority, string> = {
  p0: styles.badgeP0,
  p1: styles.badgeP1,
  p2: styles.badgeP2,
  p3: styles.badgeP3,
};

const STATUS_BADGE_CLASS: Record<AgentDetail["status"], string> = {
  running: styles.statusRunning,
  paused: styles.statusPaused,
  terminated: styles.statusTerminated,
  idle: styles.statusIdle,
};

function timeAgo(isoString: string): string {
  const elapsed = Date.now() - new Date(isoString).getTime();
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

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

interface Props {
  agent: AgentDetail;
}

const AgentMeta: Component<Props> = (props) => {
  const currentStageEntry = (): StageEntry | undefined =>
    [...props.agent.stageHistory]
      .reverse()
      .find((e) => e.stage === props.agent.currentStage);

  return (
    <div class={styles.panel}>
      {/* Task info */}
      <div class={styles.card}>
        <h3 class={styles.taskName}>{props.agent.taskName}</h3>
        <p class={styles.taskDescription}>{props.agent.taskDescription}</p>

        <div class={styles.badgeRow}>
          <span
            class={`${styles.priorityBadge} ${PRIORITY_BADGE_CLASS[props.agent.priority]}`}
          >
            {PRIORITY_LABELS[props.agent.priority]}
          </span>
          <span
            class={`${styles.statusBadge} ${STATUS_BADGE_CLASS[props.agent.status]}`}
          >
            {props.agent.status}
          </span>
        </div>
      </div>

      {/* Timing metadata */}
      <div class={styles.card}>
        <p class={styles.sectionTitle}>Timing</p>
        <div class={styles.metaGrid}>
          <span class={styles.metaLabel}>Assigned</span>
          <span class={styles.metaValue}>{timeAgo(props.agent.assignedAt)}</span>

          <span class={styles.metaLabel}>Running</span>
          <span class={styles.metaValue}>{duration(props.agent.assignedAt)}</span>

          <span class={styles.metaLabel}>Current stage</span>
          <span class={styles.metaValue}>
            {currentStageEntry()
              ? duration(currentStageEntry()!.enteredAt)
              : "—"}
          </span>
        </div>
      </div>

      {/* Token usage */}
      <Show when={props.agent.inputTokens > 0}>
        <div class={styles.card}>
          <p class={styles.sectionTitle}>Token Usage</p>
          <div class={styles.tokenBadge}>
            <span>{props.agent.inputTokens.toLocaleString()} in</span>
            <span class={styles.tokenSep}>/</span>
            <span>{props.agent.outputTokens.toLocaleString()} out</span>
          </div>
        </div>
      </Show>

      {/* Stage history */}
      <div class={styles.card}>
        <p class={styles.sectionTitle}>Stage History</p>
        <ul class={styles.stageList}>
          <For each={props.agent.stageHistory}>
            {(entry: StageEntry) => {
              const isCurrent = entry.stage === props.agent.currentStage;
              return (
                <li class={styles.stageEntry}>
                  <span
                    class={`${styles.stageDot} ${isCurrent ? styles.stageDotCurrent : ""}`}
                  />
                  <span
                    class={`${styles.stageName} ${isCurrent ? styles.stageNameCurrent : ""}`}
                  >
                    {entry.stage}
                  </span>
                  <span class={styles.stageTime}>{timeAgo(entry.enteredAt)}</span>
                </li>
              );
            }}
          </For>
        </ul>
      </div>

    </div>
  );
};

export default AgentMeta;
