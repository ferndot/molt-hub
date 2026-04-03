/**
 * UnifiedCard — a board card that optionally shows triage attention info
 * with a glowing left-border accent and inline action buttons.
 */

import { Show, For, createMemo, type Component, type JSX } from "solid-js";
import { useNavigate, A } from "@solidjs/router";
import { TbOutlineCheck, TbOutlineX, TbOutlineArrowRight, TbOutlineClock } from "solid-icons/tb";
import type { MissionControlItem } from "./missionControlStore";
import { deleteTask } from "../Board/boardStore";
import { useAgentDetailStore } from "../AgentDetail/agentStore";
import styles from "./UnifiedCard.module.css";

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface UnifiedCardProps {
  item: MissionControlItem;
  highlighted?: boolean;
  focused?: boolean;
  onToggle?: (taskId: string) => void;
  onHoverEnter?: (taskId: string) => void;
  onHoverLeave?: () => void;
  onApprove?: (taskId: string, triageId: string) => void;
  onReject?: (taskId: string, triageId: string, reason: string) => void;
  onRedirect?: (taskId: string, triageId: string, stage: string) => void;
  onDefer?: (triageId: string) => void;
  onAcknowledge?: (triageId: string) => void;
  onDragStart?: (e: DragEvent, taskId: string, fromStage: string) => void;
  onDragEnd?: (e: DragEvent) => void;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Agent status helpers
// ---------------------------------------------------------------------------

const statusLabel = (s: string): string =>
  ({
    waiting: 'Waiting for agent',
    working: 'Agent working',
    succeeded: 'Completed',
    errored: 'Agent error',
    'needs-attention': 'Needs review',
  } as Record<string, string>)[s] ?? s;

const statusColor = (s: string): string =>
  ({
    waiting: '#f59e0b',
    working: '#6366f1',
    succeeded: '#22c55e',
    errored: '#e63946',
    'needs-attention': '#f4a261',
  } as Record<string, string>)[s] ?? 'transparent';

const priorityBadgeClass = (priority: string): string => {
  switch (priority) {
    case "p0":
      return styles.priorityP0;
    case "p1":
      return styles.priorityP1;
    case "p2":
      return styles.priorityP2;
    case "p3":
      return styles.priorityP3;
    default:
      return styles.priorityP3;
  }
};

const agentChipColor = (status: string): string =>
  ({
    running: '#6366f1',
    paused: '#f59e0b',
    terminated: '#e63946',
    idle: '#22c55e',
  } as Record<string, string>)[status] ?? '#94a3b8';

const attentionBorderClass = (priority: string): string => {
  switch (priority) {
    case "p1":
      return styles.attentionP1;
    case "p2":
      return styles.attentionP2;
    case "p3":
      return styles.attentionP3;
    default:
      return "";
  }
};

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const UnifiedCard: Component<UnifiedCardProps> = (props) => {
  const navigate = useNavigate();
  const { state: agentState } = useAgentDetailStore();
  const taskAgents = createMemo(() =>
    agentState.agents.filter((a) => a.taskId === props.item.id),
  );

  const cardClass = () => {
    const classes = [styles.card];
    if (props.item.attentionInfo) {
      classes.push(styles.attention);
      const borderCls = attentionBorderClass(props.item.priority);
      if (borderCls) classes.push(borderCls);
    }
    if (props.highlighted) classes.push(styles.highlighted);
    if (props.focused) classes.push(styles.focused);
    return classes.join(" ");
  };

  const handleDragStart = (e: DragEvent) => {
    props.onDragStart?.(e, props.item.id, props.item.stage);
  };

  const handleDragEnd = (e: DragEvent) => {
    props.onDragEnd?.(e);
  };

  const stopProp = (e: MouseEvent, fn: () => void) => {
    e.stopPropagation();
    fn();
  };

  return (
    <div
      class={cardClass()}
      draggable="true"
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onClick={(e) => {
        if ((e.target as HTMLElement).closest("button")) return;
        if (e.detail === 2) { navigate(`/tasks/${props.item.id}`); return; }
        props.onToggle?.(props.item.id);
      }}
      onMouseEnter={() => props.onHoverEnter?.(props.item.id)}
      onMouseLeave={() => props.onHoverLeave?.()}
      data-task-id={props.item.id}
      role="button"
      tabIndex={0}
    >
      {/* Header */}
      <div class={styles.header}>
        <span class={styles.taskName}>{props.item.name}</span>
        <Show when={props.item.agentStatus}>
          <span
            class={styles.agentStatusDot}
            data-status={props.item.agentStatus}
            title={statusLabel(props.item.agentStatus!)}
            style={{ "background-color": statusColor(props.item.agentStatus!) } as JSX.CSSProperties}
          />
        </Show>
        <span
          class={`${styles.priorityBadge} ${priorityBadgeClass(props.item.priority)}`}
        >
          {props.item.priority}
        </span>
        <button
          class={styles.deleteBtn}
          title="Delete task"
          onClick={(e) => stopProp(e, () => { void deleteTask(props.item.id); })}
        >
          <TbOutlineX size={11} />
        </button>
      </div>

      {/* Agent chips — one per running agent, each links to the agent detail */}
      <Show when={taskAgents().length > 0}>
        <div class={styles.agentChips}>
          <For each={taskAgents()}>
            {(agent) => (
              <A
                href={`/agents/${agent.id}`}
                class={styles.agentChip}
                data-status={agent.status}
                title={`${agent.name} — ${agent.status}`}
                onClick={(e) => e.stopPropagation()}
              >
                <span
                  class={styles.agentChipDot}
                  style={{ "background-color": agentChipColor(agent.status) } as JSX.CSSProperties}
                  data-status={agent.status}
                />
                {agent.name}
              </A>
            )}
          </For>
        </div>
      </Show>

      {/* Meta — stage chip omitted; column header already shows the stage */}
      <Show when={props.item.agentName && taskAgents().length === 0}>
        <div class={styles.meta}>
          <span class={styles.agentName}>{props.item.agentName}</span>
        </div>
      </Show>

      {/* Attention actions — always visible when attention info present */}
      <Show when={props.item.attentionInfo}>
        <div class={styles.actions}>
          <Show when={props.item.attentionInfo!.triageType === "decision"}>
            <button
              class={`${styles.actionBtn} ${styles.approveBtn}`}
              onClick={(e) =>
                stopProp(e, () =>
                  props.onApprove?.(
                    props.item.id,
                    props.item.attentionInfo!.triageId,
                  ),
                )
              }
            >
              <TbOutlineCheck size={12} /> Approve
            </button>
            <button
              class={`${styles.actionBtn} ${styles.rejectBtn}`}
              onClick={(e) =>
                stopProp(e, () => {
                  const reason =
                    typeof window !== "undefined"
                      ? window.prompt("Reason for rejection (optional):") ?? ""
                      : "";
                  props.onReject?.(
                    props.item.id,
                    props.item.attentionInfo!.triageId,
                    reason,
                  );
                })
              }
            >
              <TbOutlineX size={12} /> Reject
            </button>
            <button
              class={`${styles.actionBtn} ${styles.redirectBtn}`}
              onClick={(e) =>
                stopProp(e, () => {
                  const to =
                    typeof window !== "undefined"
                      ? window.prompt(
                          "Pipeline stage id to redirect to:",
                          "",
                        )?.trim() ?? ""
                      : "";
                  if (to) {
                    props.onRedirect?.(
                      props.item.id,
                      props.item.attentionInfo!.triageId,
                      to,
                    );
                  }
                })
              }
            >
              <TbOutlineArrowRight size={12} /> Redirect
            </button>
            <button
              class={`${styles.actionBtn} ${styles.deferBtn}`}
              onClick={(e) =>
                stopProp(e, () =>
                  props.onDefer?.(props.item.attentionInfo!.triageId),
                )
              }
            >
              <TbOutlineClock size={12} /> Defer
            </button>
          </Show>
          <Show when={props.item.attentionInfo!.triageType === "info"}>
            <button
              class={`${styles.actionBtn} ${styles.ackBtn}`}
              onClick={(e) =>
                stopProp(e, () =>
                  props.onAcknowledge?.(props.item.attentionInfo!.triageId),
                )
              }
            >
              <TbOutlineCheck size={12} /> Acknowledge
            </button>
          </Show>
        </div>
      </Show>

      {/* Expanded detail */}
      <Show when={props.item.expanded && props.item.summary}>
        <div class={styles.detail}>{props.item.summary}</div>
        <div class={styles.timeInStage}>{props.item.timeInStage}</div>
      </Show>
    </div>
  );
};

export default UnifiedCard;
