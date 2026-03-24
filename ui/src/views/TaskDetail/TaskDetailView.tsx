/**
 * TaskDetailView — detail page for a single task.
 *
 * Left pane: description + activity timeline.
 * Right pane: metadata (stage, priority, agent, timestamps).
 *
 * Route: /tasks/:id
 */

import type { Component } from "solid-js";
import { Show, For, onMount, onCleanup } from "solid-js";
import { useParams } from "@solidjs/router";
import { TbOutlineArrowLeft } from "solid-icons/tb";
import { task, events, loading, error, loadTask, clearTask } from "./taskDetailStore";
import { PRIORITY_COLORS } from "../Board/TaskCard";
import type { Priority } from "../../types/domain";
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

  onMount(() => {
    loadTask(params.id);
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
                <a href="/" class={styles.backBtn} data-testid="back-btn">
                  <TbOutlineArrowLeft size={14} style={{ "vertical-align": "middle" }} /> Board
                </a>
                <span class={styles.taskTitle}>{t().title}</span>
                <span
                  class={styles.priorityBadge}
                  style={{ background: PRIORITY_COLORS[t().priority as Priority] ?? "#6c757d" }}
                >
                  {t().priority.toUpperCase()}
                </span>
                <span class={styles.stagePill}>{stageLabel(t().current_stage)}</span>
              </div>

              {/* Body */}
              <div class={styles.body}>
                {/* Main pane — description + timeline */}
                <div class={styles.mainPane}>
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
