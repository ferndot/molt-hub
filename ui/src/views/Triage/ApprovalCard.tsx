/**
 * ApprovalCard — card for approval requests shown in the triage queue.
 *
 * Displays task title, stage requiring approval, who requested it, and
 * how long it has been pending. Provides Approve (green) and Reject (red
 * with reason input) actions.
 */

import { createSignal, Show, type Component } from "solid-js";
import { api } from "../../lib/api";
import { boardState } from "../Board/boardStore";
import styles from "./ApprovalCard.module.css";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ApprovalRequest {
  /** Unique approval request ID */
  id: string;
  /** Task id (ULID) for persisting `HumanDecision` */
  taskId: string;
  /** Agent ID that needs approval (display / correlation) */
  agentId: string;
  /** Human-readable task title */
  taskTitle: string;
  /** Pipeline stage requiring approval */
  stage: string;
  /** Who / what requested the approval (agent name) */
  requestedBy: string;
  /** ISO timestamp of when the approval was requested */
  requestedAt: string;
  /** Optional summary of what is being approved */
  summary?: string;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function timeAgo(isoString: string): string {
  const elapsed = Date.now() - new Date(isoString).getTime();
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface Props {
  request: ApprovalRequest;
  onResolved?: (id: string) => void;
}

const ApprovalCard: Component<Props> = (props) => {
  const [showRejectInput, setShowRejectInput] = createSignal(false);
  const [rejectReason, setRejectReason] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);

  async function handleApprove(): Promise<void> {
    const boardId = boardState.activeBoardId?.trim() ?? "";
    if (!boardId) {
      console.error("Select a board on the workboard before approving.");
      return;
    }
    setSubmitting(true);
    try {
      await api.submitTaskHumanDecision(props.request.taskId, {
        boardId,
        kind: "approved",
      });
      props.onResolved?.(props.request.id);
    } catch {
      // Approval failed — keep card visible
    } finally {
      setSubmitting(false);
    }
  }

  function handleRejectClick(): void {
    setShowRejectInput(true);
  }

  async function handleConfirmReject(): Promise<void> {
    const reason = rejectReason().trim();
    if (!reason) return;
    const boardId = boardState.activeBoardId?.trim() ?? "";
    if (!boardId) {
      console.error("Select a board on the workboard before rejecting.");
      return;
    }
    setSubmitting(true);
    try {
      await api.submitTaskHumanDecision(props.request.taskId, {
        boardId,
        kind: "rejected",
        reason,
      });
      props.onResolved?.(props.request.id);
    } catch {
      // Rejection failed — keep card visible
    } finally {
      setSubmitting(false);
    }
  }

  function handleCancelReject(): void {
    setShowRejectInput(false);
    setRejectReason("");
  }

  return (
    <div class={styles.card} data-testid="approval-card">
      {/* Header row */}
      <div class={styles.header}>
        <span class={styles.approvalBadge}>Approval</span>
        <span class={styles.taskName} title={props.request.taskTitle}>
          {props.request.taskTitle}
        </span>
        <span class={styles.meta}>
          <span>{props.request.requestedBy}</span>
          <span class={styles.metaSep}>&middot;</span>
          <span>{props.request.stage}</span>
          <span class={styles.metaSep}>&middot;</span>
          <span>{timeAgo(props.request.requestedAt)}</span>
        </span>
      </div>

      {/* Summary */}
      <Show when={props.request.summary}>
        <div class={styles.detailRow}>
          <span>{props.request.summary}</span>
        </div>
      </Show>

      {/* Action buttons */}
      <Show when={!showRejectInput()}>
        <div class={styles.actions}>
          <button
            class={styles.btnApprove}
            onClick={handleApprove}
            disabled={submitting()}
            type="button"
          >
            {submitting() ? "Approving..." : "Approve"}
          </button>
          <button
            class={styles.btnReject}
            onClick={handleRejectClick}
            disabled={submitting()}
            type="button"
          >
            Reject
          </button>
        </div>
      </Show>

      {/* Reject reason input */}
      <Show when={showRejectInput()}>
        <div class={styles.rejectRow}>
          <input
            class={styles.reasonInput}
            type="text"
            placeholder="Reason for rejection..."
            value={rejectReason()}
            onInput={(e) => setRejectReason(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleConfirmReject();
              if (e.key === "Escape") handleCancelReject();
            }}
          />
          <button
            class={styles.btnConfirmReject}
            onClick={handleConfirmReject}
            disabled={submitting() || !rejectReason().trim()}
            type="button"
          >
            {submitting() ? "Rejecting..." : "Confirm"}
          </button>
          <button
            class={styles.btnCancel}
            onClick={handleCancelReject}
            type="button"
          >
            Cancel
          </button>
        </div>
      </Show>
    </div>
  );
};

export default ApprovalCard;
