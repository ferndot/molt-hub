/**
 * Workboard — unified kanban: multiple boards, triage-aware columns,
 * focus filter, issue import, and column configuration.
 */

import { For, Show, createSignal, type Component } from "solid-js";
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
  const [jiraImportOpen, setJiraImportOpen] = createSignal(false);
  const [githubImportOpen, setGitHubImportOpen] = createSignal(false);

  const hasIssueIntegration = () =>
    settingsState.jiraConfig.connected || settingsState.githubConfig.connected;

  const sortedStages = () => getSortedStages();
  const firstStageId = () => sortedStages()[0]?.id;

  const activeBoardTitle = () => {
    const id = boardState.activeBoardId;
    const b = boardState.boards.find((x) => x.id === id);
    return b?.name ?? id;
  };

  const onAddManualIssue = async () => {
    const stageId = firstStageId();
    if (!stageId) return;
    const title = window.prompt("Issue title:");
    if (!title?.trim()) return;
    try {
      await api.createTask({
        title: title.trim(),
        initialStage: stageId,
      });
    } catch (e) {
      window.alert(e instanceof Error ? e.message : "Could not create issue");
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
                        <div class={styles.columnFooter}>
                          <Show when={hasIssueIntegration()}>
                            <ImportIssuesMenu
                              jiraConnected={settingsState.jiraConfig.connected}
                              githubConnected={settingsState.githubConfig.connected}
                              onSelectJira={() => setJiraImportOpen(true)}
                              onSelectGitHub={() => setGitHubImportOpen(true)}
                            />
                          </Show>
                          <button
                            type="button"
                            class={styles.addIssueBtn}
                            onClick={() => void onAddManualIssue()}
                          >
                            Add issue
                          </button>
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
