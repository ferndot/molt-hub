/**
 * TaskDetailView — detail page for a single task.
 *
 * Left pane: description + activity timeline.
 * Right pane: metadata (stage, priority, agent, timestamps).
 *
 * Route: /tasks/:id
 */

import type { Component } from "solid-js";
import { Show, For, onMount, onCleanup, createSignal } from "solid-js";
import { useParams } from "@solidjs/router";
import { TbOutlineArrowLeft } from "solid-icons/tb";
import { task, events, loading, error, loadTask, clearTask } from "./taskDetailStore";
import { boardKanbanPath, boardState } from "../Board/boardStore";
import type { Priority } from "../../types/domain";

const PRIORITY_COLORS: Record<Priority, string> = {
  p0: "#e63946",
  p1: "#f4a261",
  p2: "#2a9d8f",
  p3: "#6c757d",
};
import styles from "./TaskDetailView.module.css";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

function stageLabel(stage: string): string {
  return stage.replace(/-/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const TaskDetailView: Component = () => {
  const params = useParams<{ id: string }>();

  const [taskAgents, setTaskAgents] = createSignal<Array<{agent_id: string; task_id: string; status: string}>>([]);

  onMount(() => {
    loadTask(params.id);
    fetch("/api/agents")
      .then(r => r.json())
      .then((data: { agents: Array<{agent_id: string; task_id: string; status: string}> }) => {
        setTaskAgents(data.agents.filter(a => a.task_id === params.id));
      })
      .catch(() => {});
  });

  onCleanup(() => {
    clearTask();
  });

  return (
    <Show when={!loading()} fallback={<div class={styles.loading}>Loading task...</div>}>
      <Show
        when={!error()}
        fallback={
          <div class={styles.notFound}>
            <p class={styles.notFoundTitle}>Task not found</p>
            <p class={styles.notFoundSub}>{error()}</p>
          </div>
        }
      >
        <Show when={task()}>
          {(t) => (
            <div class={styles.container}>
              {/* Header */}
              <div class={styles.header}>
                <a
                  href={
                    boardState.activeBoardId
                      ? boardKanbanPath(boardState.activeBoardId)
                      : "/boards"
                  }
                  class={styles.backBtn}
                  data-testid="back-btn"
                >
                  <TbOutlineArrowLeft size={14} style={{ "vertical-align": "middle" }} /> Board
                </a>
                <span class={styles.taskTitle}>{t().title}</span>
                <span
                  class={styles.priorityBadge}
                  style={{ background: PRIORITY_COLORS[t().priority as Priority] ?? "#6c757d" }}
                >
                  {t().priority.toUpperCase()}
                </span>
                <span
                  class={styles.stagePill}
                  style={(() => {
                    const c = boardState.pipelineStages.find(s => s.id === t().current_stage)?.color;
                    return c ? { "--stage-color": c, background: `color-mix(in srgb, ${c} 15%, transparent)`, color: c, "border-color": `color-mix(in srgb, ${c} 40%, transparent)` } : {};
                  })()}
                >{stageLabel(t().current_stage)}</span>
              </div>

              {/* Body */}
              <div class={styles.body}>
                {/* Main pane — description + timeline */}
                <div class={styles.mainPane}>
                  {/* Active Agents */}
                  <Show when={taskAgents().length > 0}>
                    <section class={styles.agentsSection}>
                      <h3 class={styles.sectionTitle}>Active Agents</h3>
                      <For each={taskAgents()}>
                        {(agent) => (
                          <div class={styles.agentRow}>
                            <span class={styles.agentDot} />
                            <a href={`/agents/${agent.agent_id}`} class={styles.agentIdText}>{agent.agent_id.slice(-8)}</a>
                            <span class={styles.agentStatusBadge}>{agent.status}</span>
                            <a href={`/agents/${agent.agent_id}`} class={styles.steerBtn}>Steer</a>
                          </div>
                        )}
                      </For>
                    </section>
                  </Show>

                  {/* Overview */}
                  <section>
                    <h3 class={styles.sectionTitle}>Overview</h3>
                    <Show
                      when={t().description}
                      fallback={<p class={styles.descriptionEmpty}>No description provided.</p>}
                    >
                      <p class={styles.description}>{t().description}</p>
                    </Show>
                  </section>

                  {/* Activity Log */}
                  <section>
                    <h3 class={styles.sectionTitle}>Activity Log</h3>
                    <Show
                      when={events().length > 0}
                      fallback={<p class={styles.timelineEmpty}>No activity recorded yet.</p>}
                    >
                      <div class={styles.timeline} data-testid="activity-timeline">
                        <For each={events()}>
                          {(evt) => (
                            <div class={styles.timelineEntry} data-testid="timeline-entry">
                              <span class={styles.timelineDot} />
                              <div class={styles.timelineContent}>
                                <p class={styles.timelineDesc}>{evt.description}</p>
                                <div class={styles.timelineMeta}>
                                  <span>{evt.actor}</span>
                                  <span>{formatTimestamp(evt.timestamp)}</span>
                                </div>
                              </div>
                            </div>
                          )}
                        </For>
                      </div>
                    </Show>
                  </section>
                </div>

                {/* Side pane — metadata */}
                <div class={styles.sidePane}>
                  <h3 class={styles.sectionTitle}>Details</h3>
                  <div class={styles.metaList}>
                    <div class={styles.metaItem}>
                      <span class={styles.metaLabel}>Status</span>
                      <span class={styles.metaValue}>{t().state_type.replace(/_/g, " ")}</span>
                    </div>
                    <div class={styles.metaItem}>
                      <span class={styles.metaLabel}>Stage</span>
                      <span class={styles.metaValue}>{stageLabel(t().current_stage)}</span>
                    </div>
                    <div class={styles.metaItem}>
                      <span class={styles.metaLabel}>Priority</span>
                      <span class={styles.metaValue}>{t().priority.toUpperCase()}</span>
                    </div>
                    <div class={styles.metaItem}>
                      <span class={styles.metaLabel}>Assigned Agent</span>
                      <span class={styles.metaValue}>{t().agent_name ?? "Unassigned"}</span>
                    </div>
                    <div class={styles.metaItem}>
                      <span class={styles.metaLabel}>Created</span>
                      <span class={styles.metaValue}>{formatTimestamp(t().created_at)}</span>
                    </div>
                    <div class={styles.metaItem}>
                      <span class={styles.metaLabel}>Updated</span>
                      <span class={styles.metaValue}>{formatTimestamp(t().updated_at)}</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          )}
        </Show>
      </Show>
    </Show>
  );
};

export default TaskDetailView;
