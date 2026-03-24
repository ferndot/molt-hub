/**
 * MissionControlView — the main board view with attention-annotated cards.
 * The inbox sidebar is now in AppLayout (available across all views).
 */

import { Component, For, Show, createSignal } from "solid-js";
import { TbOutlineFocus, TbOutlineEye } from "solid-icons/tb";
import { useMissionControl } from "./missionControlStore";
import MissionColumn from "./MissionColumn";
import GitHubImport from "../Settings/GitHubImport";
import JiraImport from "../Settings/JiraImport";
import { settingsState } from "../Settings/settingsStore";
import { projectState } from "../../stores/projectStore";
import styles from "./MissionControlView.module.css";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const MissionControlView: Component = () => {
  const mc = useMissionControl();
  const [jiraImportOpen, setJiraImportOpen] = createSignal(false);
  const [githubImportOpen, setGitHubImportOpen] = createSignal(false);

  const hasIssueIntegration = () =>
    settingsState.jiraConfig.connected || settingsState.githubConfig.connected;

  return (
    <div class={styles.container}>
      {/* Header bar */}
      <div class={styles.header}>
        <h2 class={styles.title}>Mission Control</h2>
        <Show when={mc.totalAttentionCount() > 0}>
          <span class={styles.attentionBadge}>
            {mc.totalAttentionCount()} need attention
          </span>
        </Show>
        <button
          class={styles.filterToggle}
          classList={{ [styles.filterToggleActive]: mc.globalFilterActive() }}
          onClick={mc.toggleGlobalFilter}
          title={mc.globalFilterActive() ? "Show all tasks" : "Focus on tasks needing attention"}
        >
          {mc.globalFilterActive()
            ? <><TbOutlineEye size={14} /> Show All</>
            : <><TbOutlineFocus size={14} /> Focus</>}
        </button>
      </div>

      <Show when={projectState.loaded && projectState.projects.length === 0}>
        <div class={styles.onboarding} data-testid="mc-onboarding">
          <strong>No project yet.</strong> Add one under{" "}
          <a href="/settings">Settings → Projects</a> with a name and the path to a Git repo on
          disk. Integrations (Jira, GitHub) and repo-scoped features use the active project.
        </div>
      </Show>

      {/* Board columns */}
      <div class={styles.body}>
        <div class={styles.boardRegion}>
          <For each={mc.stages()}>
            {(stage) => (
              <MissionColumn
                stage={stage}
                items={mc.visibleItemsForStage(stage)}
                attentionCount={mc.attentionCountForStage(stage)}
                filterActive={mc.globalFilterActive()}
                hiddenCount={mc.hiddenCountForStage(stage)}
                hoveredItemId={mc.hoveredItemId()}
                onHoverItem={mc.setHoveredItemId}
                onApprove={(triageId) => mc.approve(triageId)}
                onReject={(triageId) => mc.reject(triageId, "")}
                onRedirect={(triageId, stage) => mc.redirect(triageId, stage)}
                onDefer={(triageId) => mc.defer(triageId)}
                onAcknowledge={(triageId) => mc.acknowledge(triageId)}
                onDrop={(taskId, from, to) => mc.moveTask(taskId, from, to)}
                onToggle={(taskId) => mc.toggleCard(taskId)}
                footer={
                  stage === "backlog" && hasIssueIntegration()
                    ? () => {
                        let detailsEl: HTMLDetailsElement | undefined;
                        const closeMenu = () => {
                          if (detailsEl) detailsEl.open = false;
                        };
                        return (
                          <details
                            class={styles.importDropdown}
                            ref={(el) => {
                              detailsEl = el;
                            }}
                          >
                            <summary class={styles.importSummary}>Import issues</summary>
                            <div class={styles.importMenu} role="menu">
                              <Show when={settingsState.jiraConfig.connected}>
                                <button
                                  type="button"
                                  role="menuitem"
                                  class={styles.importMenuItem}
                                  onClick={() => {
                                    closeMenu();
                                    setJiraImportOpen(true);
                                  }}
                                >
                                  Jira
                                </button>
                              </Show>
                              <Show when={settingsState.githubConfig.connected}>
                                <button
                                  type="button"
                                  role="menuitem"
                                  class={styles.importMenuItem}
                                  onClick={() => {
                                    closeMenu();
                                    setGitHubImportOpen(true);
                                  }}
                                >
                                  GitHub
                                </button>
                              </Show>
                            </div>
                          </details>
                        );
                      }
                    : undefined
                }
              />
            )}
          </For>
        </div>
      </div>

      <JiraImport
        isOpen={jiraImportOpen()}
        onClose={() => setJiraImportOpen(false)}
      />
      <GitHubImport
        isOpen={githubImportOpen()}
        onClose={() => setGitHubImportOpen(false)}
      />
    </div>
  );
};

export default MissionControlView;
