/**
 * TriageItem — a single card in the triage queue.
 *
 * Renders priority badge, task name, meta info, quick-action buttons,
 * and an expandable details panel.
 */

import type { Component } from "solid-js";
import { createSignal, Show } from "solid-js";
import type { Priority } from "../../types/domain";
import type { TriageItem as TriageItemType } from "./triageStore";
import { approve, reject, redirect, defer, acknowledge } from "./triageStore";
import styles from "./TriageItem.module.css";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const PRIORITY_LABELS: Record<Priority, string> = {
  p0: "P0",
  p1: "P1",
  p2: "P2",
  p3: "P3",
};

const PRIORITY_BADGE_CLASS: Record<Priority, string> = {
  p0: styles.badgeP0,
  p1: styles.badgeP1,
  p2: styles.badgeP2,
  p3: styles.badgeP3,
};

const PRIORITY_ITEM_CLASS: Record<Priority, string> = {
  p0: styles.p0,
  p1: styles.p1,
  p2: styles.p2,
  p3: styles.p3,
};

/** Formats a relative time string, e.g. "45m ago", "2h ago". */
function timeAgo(isoString: string): string {
  const elapsed = Date.now() - new Date(isoString).getTime();
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

const PIPELINE_STAGES = [
  "planning",
  "code-review",
  "testing",
  "integration",
  "deployment",
  "documentation",
  "dependency-update",
];

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface Props {
  item: TriageItemType;
}

const TriageItemCard: Component<Props> = (props) => {
  const [expanded, setExpanded] = createSignal(false);
  const [showRedirect, setShowRedirect] = createSignal(false);
  const [redirectTarget, setRedirectTarget] = createSignal("code-review");

  function handleApprove(e: MouseEvent): void {
    e.stopPropagation();
    approve(props.item.id);
  }

  function handleReject(e: MouseEvent): void {
    e.stopPropagation();
    const reason = window.prompt("Reason for rejection:");
    if (reason !== null) {
      reject(props.item.id, reason);
    }
  }

  function handleRedirectClick(e: MouseEvent): void {
    e.stopPropagation();
    setShowRedirect(true);
  }

  function handleConfirmRedirect(e: MouseEvent): void {
    e.stopPropagation();
    redirect(props.item.id, redirectTarget());
    setShowRedirect(false);
  }

  function handleDefer(e: MouseEvent): void {
    e.stopPropagation();
    defer(props.item.id);
  }

  function handleAcknowledge(e: MouseEvent): void {
    e.stopPropagation();
    acknowledge(props.item.id);
  }

  const isActionItem = () =>
    props.item.priority === "p0" || props.item.priority === "p1";

  return (
    <div
      class={`${styles.item} ${PRIORITY_ITEM_CLASS[props.item.priority]}`}
      onClick={() => setExpanded((v) => !v)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") setExpanded((v) => !v);
      }}
      aria-expanded={expanded()}
    >
      {/* Header row */}
      <div class={styles.header}>
        <span
          class={`${styles.priorityBadge} ${PRIORITY_BADGE_CLASS[props.item.priority]}`}
          title={`Priority ${PRIORITY_LABELS[props.item.priority]}`}
        >
          {PRIORITY_LABELS[props.item.priority]}
        </span>
        <span class={styles.taskName} title={props.item.taskName}>
          {props.item.taskName}
        </span>
        <span class={styles.meta}>
          <span>{props.item.agentName}</span>
          <span class={styles.metaSep}>·</span>
          <span>{props.item.stage}</span>
          <span class={styles.metaSep}>·</span>
          <span>{timeAgo(props.item.createdAt)}</span>
        </span>
      </div>

      {/* Quick-action buttons */}
      <Show when={isActionItem()}>
        <div class={styles.actions}>
          <button class={styles.btnApprove} onClick={handleApprove} type="button">
            ✓ Approve
          </button>
          <button class={styles.btnReject} onClick={handleReject} type="button">
            ✕ Reject
          </button>
          <button class={styles.btnRedirect} onClick={handleRedirectClick} type="button">
            → Redirect
          </button>
          <button class={styles.btnDefer} onClick={handleDefer} type="button">
            ⏱ Defer
          </button>
        </div>
      </Show>

      <Show when={!isActionItem()}>
        <div class={styles.actions}>
          <button class={styles.btnAck} onClick={handleAcknowledge} type="button">
            ✓ Acknowledge
          </button>
        </div>
      </Show>

      {/* Redirect stage picker */}
      <Show when={showRedirect()}>
        <div class={styles.redirectPicker} onClick={(e) => e.stopPropagation()}>
          <select
            class={styles.stageSelect}
            value={redirectTarget()}
            onChange={(e) => setRedirectTarget(e.currentTarget.value)}
          >
            {PIPELINE_STAGES.map((s) => (
              <option value={s}>{s}</option>
            ))}
          </select>
          <button
            class={styles.btnConfirmRedirect}
            onClick={handleConfirmRedirect}
            type="button"
          >
            Confirm
          </button>
        </div>
      </Show>

      {/* Expanded details */}
      <Show when={expanded()}>
        <div class={styles.expanded}>
          <p class={styles.summary}>{props.item.summary}</p>
          <div class={styles.detailMeta}>
            <span>
              <span class={styles.detailLabel}>Task ID:</span>
              {props.item.taskId}
            </span>
            <span>
              <span class={styles.detailLabel}>Stage:</span>
              {props.item.stage}
            </span>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default TriageItemCard;
