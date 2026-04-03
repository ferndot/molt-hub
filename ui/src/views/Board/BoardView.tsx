/**
 * Workboard — unified kanban: multiple boards, triage-aware columns,
 * focus filter, issue import, and column configuration.
 */

import { For, Show, createEffect, createSignal, onMount, onCleanup, type Component } from "solid-js";
import {
  TbOutlineFocus,
  TbOutlineEye,
  TbOutlineSettings,
} from "solid-icons/tb";
import ImportIssuesMenu from "../../components/ImportIssuesMenu/ImportIssuesMenu";
import { useMissionControl } from "../MissionControl/missionControlStore";
import MissionColumn from "../MissionControl/MissionColumn";
import GitHubImport from "../Settings/GitHubImport";
import JiraImport from "../Settings/JiraImport";
import { settingsState } from "../Settings/settingsStore";
import { api } from "../../lib/api";
import { startAgentPolling } from "../AgentDetail/agentStore";
import { boardState, getSortedStages } from "./boardStore";
import ColumnEditor from "./ColumnEditor";
import mcStyles from "../MissionControl/MissionControlView.module.css";
import styles from "./BoardView.module.css";

const BoardView: Component = () => {
  const mc = useMissionControl();
  const [editorOpen, setEditorOpen] = createSignal(false);

  onMount(() => {
    const stop = startAgentPolling(5000);
    onCleanup(stop);
  });
  const [addIssueExpanded, setAddIssueExpanded] = createSignal(false);
  const [addIssueBody, setAddIssueBody] = createSignal("");
  const [addIssueError, setAddIssueError] = createSignal<string | null>(null);
  const [addIssueBusy, setAddIssueBusy] = createSignal(false);
  const [jiraImportOpen, setJiraImportOpen] = createSignal(false);
  const [githubImportOpen, setGitHubImportOpen] = createSignal(false);

  const hasIssueIntegration = () =>
    settingsState.jiraConfig.connected || settingsState.githubConfig.connected;

  const sortedStages = () => getSortedStages();
  const firstStageId = () => sortedStages()[0]?.id;

  createEffect(() => {
    if (!addIssueExpanded() || !firstStageId()) return;
    if (
      addIssueError() ===
      "Board columns are still loading. Try again in a moment."
    ) {
      setAddIssueError(null);
    }
  });

  const activeBoardTitle = () => {
    const id = boardState.activeBoardId;
    if (!id) return "Board";
    const b = boardState.boards.find((x) => x.id === id);
    return b?.name ?? id;
  };

  const collapseAddIssue = () => {
    setAddIssueExpanded(false);
    setAddIssueBody("");
    setAddIssueError(null);
  };

  const expandAddIssue = () => {
    setAddIssueBody("");
    setAddIssueError(
      firstStageId()
        ? null
        : "Board columns are still loading. Try again in a moment.",
    );
    setAddIssueExpanded(true);
  };

  const submitAddIssue = async () => {
    const stageId = firstStageId();
    if (!stageId) {
      setAddIssueError(
        "Board columns are still loading. Try again in a moment.",
      );
      return;
    }
    const description = addIssueBody().trim();
    if (!description) {
      setAddIssueError("Describe the issue first.");
      return;
    }
    setAddIssueBusy(true);
    setAddIssueError(null);
    try {
      const { title } = await api.suggestTaskTitle({ text: description });
      await api.createTask({
        title,
        description,
        initialStage: stageId,
        boardId: boardState.activeBoardId || undefined,
      });
      collapseAddIssue();
    } catch (e) {
      setAddIssueError(e instanceof Error ? e.message : String(e));
    } finally {
      setAddIssueBusy(false);
    }
  };

  return (
    <div class={mcStyles.container}>
      <div class={`${mcStyles.header} ${styles.boardPageHeader}`}>
        <div class={styles.headerLeft}>
          <h2 class={`${mcStyles.title} ${styles.boardTitleBar}`}>
            {activeBoardTitle()}
          </h2>
          <Show when={mc.totalAttentionCount() > 0}>
            <span class={mcStyles.attentionBadge}>
              {mc.totalAttentionCount()} need attention
            </span>
          </Show>
        </div>
        <div class={styles.headerActions}>
          <button
            type="button"
            class={mcStyles.filterToggle}
            classList={{ [mcStyles.filterToggleActive]: mc.globalFilterActive() }}
            onClick={mc.toggleGlobalFilter}
            title={mc.globalFilterActive() ? "Show all tasks" : "Focus on tasks needing attention"}
          >
            {mc.globalFilterActive()
              ? <><TbOutlineEye size={14} /> Show All</>
              : <><TbOutlineFocus size={14} /> Focus</>}
          </button>
          <button
            type="button"
            class={`${styles.iconBtn}${editorOpen() ? ` ${styles.iconBtnActive}` : ""}`}
            onClick={() => setEditorOpen((v) => !v)}
            title="Configure columns"
            aria-label="Configure board columns"
            aria-expanded={editorOpen()}
          >
            <TbOutlineSettings size={16} />
          </button>
        </div>
      </div>

      <ColumnEditor open={editorOpen()} onOpenChange={setEditorOpen} />

      <div class={mcStyles.body}>
        <div class={mcStyles.boardRegion}>
          <For each={sortedStages()}>
            {(stageDef) => (
              <MissionColumn
                stage={stageDef.id}
                color={stageDef.color ?? undefined}
                items={mc.visibleItemsForStage(stageDef.id)}
                attentionCount={mc.attentionCountForStage(stageDef.id)}
                filterActive={mc.globalFilterActive()}
                hiddenCount={mc.hiddenCountForStage(stageDef.id)}
                hoveredItemId={mc.hoveredItemId()}
                onHoverItem={mc.setHoveredItemId}
                onApprove={(taskId, triageId) => void mc.approveAttention(taskId, triageId)}
                onReject={(taskId, triageId, reason) =>
                  void mc.rejectAttention(taskId, triageId, reason)
                }
                onRedirect={(taskId, triageId, stage) =>
                  void mc.redirectAttention(taskId, triageId, stage)
                }
                onDefer={(triageId) => mc.defer(triageId)}
                onAcknowledge={(triageId) => mc.acknowledge(triageId)}
                onDrop={(taskId, from, to) => mc.moveTask(taskId, from, to)}
                onToggle={(taskId) => mc.toggleCard(taskId)}
                footer={
                  stageDef.id === firstStageId()
                    ? () => (
                        <div
                          class={styles.columnFooter}
                          onPointerDown={(e) => e.stopPropagation()}
                        >
                          <Show when={hasIssueIntegration()}>
                            <ImportIssuesMenu
                              jiraConnected={settingsState.jiraConfig.connected}
                              githubConnected={settingsState.githubConfig.connected}
                              onSelectJira={() => setJiraImportOpen(true)}
                              onSelectGitHub={() => setGitHubImportOpen(true)}
                            />
                          </Show>
                          <Show
                            when={addIssueExpanded()}
                            fallback={
                              <button
                                type="button"
                                class={styles.addIssueBtn}
                                onClick={() => expandAddIssue()}
                              >
                                Add issue
                              </button>
                            }
                          >
                            <div class={styles.addIssuePanel}>
                              <label class={styles.addIssueFieldLabel} for="add-issue-body">
                                Description
                              </label>
                              <textarea
                                id="add-issue-body"
                                class={styles.addIssueTextarea}
                                rows={6}
                                placeholder="What needs to be done? A short title is generated when you create the task. ⌘/Ctrl+Enter to create."
                                value={addIssueBody()}
                                onInput={(e) => setAddIssueBody(e.currentTarget.value)}
                                disabled={addIssueBusy()}
                                onKeyDown={(e) => {
                                  if (
                                    e.key === "Enter" &&
                                    (e.metaKey || e.ctrlKey)
                                  ) {
                                    e.preventDefault();
                                    void submitAddIssue();
                                  }
                                }}
                              />
                              <Show when={addIssueError()}>
                                {(msg) => (
                                  <p class={styles.addIssueError} role="alert">
                                    {msg()}
                                  </p>
                                )}
                              </Show>
                              <div class={styles.addIssueActions}>
                                <button
                                  type="button"
                                  class={styles.addIssueCancel}
                                  onClick={() => collapseAddIssue()}
                                  disabled={addIssueBusy()}
                                >
                                  Cancel
                                </button>
                                <button
                                  type="button"
                                  class={styles.addIssueSubmit}
                                  onClick={() => void submitAddIssue()}
                                  disabled={
                                    addIssueBusy() ||
                                    !firstStageId() ||
                                    !addIssueBody().trim()
                                  }
                                >
                                  {addIssueBusy() ? "Creating…" : "Create"}
                                </button>
                              </div>
                            </div>
                          </Show>
                        </div>
                      )
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
        targetStageId={firstStageId()}
        targetBoardId={boardState.activeBoardId || undefined}
      />
      <GitHubImport
        isOpen={githubImportOpen()}
        onClose={() => setGitHubImportOpen(false)}
        targetStageId={firstStageId()}
        targetBoardId={boardState.activeBoardId || undefined}
      />
    </div>
  );
};

export default BoardView;
