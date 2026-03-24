/**
 * Workboard — unified kanban: multiple boards, triage-aware columns,
 * focus filter, issue import, and column configuration.
 */

import { For, Show, createEffect, createSignal, type Component } from "solid-js";
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
import { boardState, getSortedStages } from "./boardStore";
import ColumnEditor from "./ColumnEditor";
import mcStyles from "../MissionControl/MissionControlView.module.css";
import styles from "./BoardView.module.css";

const BoardView: Component = () => {
  const mc = useMissionControl();
  const [editorOpen, setEditorOpen] = createSignal(false);
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

  /** First line → title; following lines → description (optional). */
  const parseIssueBody = (raw: string): { title: string; description?: string } => {
    const t = raw.trim();
    const nl = t.indexOf("\n");
    if (nl === -1) {
      return { title: t };
    }
    const title = t.slice(0, nl).trim();
    const rest = t.slice(nl + 1).trim();
    return {
      title,
      ...(rest ? { description: rest } : {}),
    };
  };

  const submitAddIssue = async () => {
    const stageId = firstStageId();
    if (!stageId) {
      setAddIssueError(
        "Board columns are still loading. Try again in a moment.",
      );
      return;
    }
    const { title, description } = parseIssueBody(addIssueBody());
    if (!title) {
      setAddIssueError("Write a title on the first line (or a single line of text).");
      return;
    }
    setAddIssueBusy(true);
    setAddIssueError(null);
    try {
      await api.createTask({
        title,
        ...(description ? { description } : {}),
        initialStage: stageId,
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
                items={mc.visibleItemsForStage(stageDef.id)}
                attentionCount={mc.attentionCountForStage(stageDef.id)}
                filterActive={mc.globalFilterActive()}
                hiddenCount={mc.hiddenCountForStage(stageDef.id)}
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
                              <div class={styles.addIssueInstructions}>
                                <p class={styles.addIssueLead}>
                                  New tasks land in this column. Use the box below to describe the work.
                                </p>
                                <ul class={styles.addIssueList}>
                                  <li>
                                    <strong>First line</strong> becomes the task title (keep it short and actionable).
                                  </li>
                                  <li>
                                    <strong>Additional lines</strong> become the description: context, acceptance criteria, links, or notes for whoever picks it up.
                                  </li>
                                  <li>
                                    Submit with <strong>Create</strong> or{" "}
                                    <strong>Ctrl+Enter</strong> (⌘+Enter on Mac).
                                  </li>
                                </ul>
                              </div>
                              <label class={styles.addIssueFieldLabel} for="add-issue-body">
                                Issue
                              </label>
                              <textarea
                                id="add-issue-body"
                                class={styles.addIssueTextarea}
                                rows={6}
                                placeholder={`Example:\nFix login redirect on Safari\n\nRepro: …\nExpected: …`}
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
                                  disabled={addIssueBusy() || !firstStageId()}
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
      />
      <GitHubImport
        isOpen={githubImportOpen()}
        onClose={() => setGitHubImportOpen(false)}
        targetStageId={firstStageId()}
      />
    </div>
  );
};

export default BoardView;
