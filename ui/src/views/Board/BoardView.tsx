/**
 * Workboard — unified kanban: multiple boards per project, triage-aware columns,
 * focus filter, issue import, and column configuration.
 */

import { For, Show, createSignal, type Component } from "solid-js";
import {
  TbOutlineFocus,
  TbOutlineEye,
  TbOutlinePlus,
  TbOutlineSettings,
  TbOutlineTrash,
} from "solid-icons/tb";
import ImportIssuesMenu from "../../components/ImportIssuesMenu/ImportIssuesMenu";
import { useMissionControl } from "../MissionControl/missionControlStore";
import MissionColumn from "../MissionControl/MissionColumn";
import GitHubImport from "../Settings/GitHubImport";
import JiraImport from "../Settings/JiraImport";
import { settingsState } from "../Settings/settingsStore";
import { projectState } from "../../stores/projectStore";
import {
  boardState,
  createBoard,
  deleteBoard,
  getSortedStages,
  setActiveBoard,
} from "./boardStore";
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

  const onAddBoard = async () => {
    const raw = window.prompt(
      "New board id (letters, numbers, dashes, underscores):",
    );
    if (!raw?.trim()) return;
    try {
      await createBoard(raw.trim());
    } catch (e) {
      window.alert(e instanceof Error ? e.message : "Could not create board");
    }
  };

  const onDeleteBoard = async () => {
    const id = boardState.activeBoardId;
    if (id === "default") {
      window.alert("The default board cannot be deleted.");
      return;
    }
    if (!window.confirm(`Delete board "${id}"? This cannot be undone.`)) return;
    try {
      await deleteBoard(id);
    } catch (e) {
      window.alert(e instanceof Error ? e.message : "Could not delete board");
    }
  };

  return (
    <div class={mcStyles.container}>
      <div class={mcStyles.header}>
        <h2 class={mcStyles.title}>Boards</h2>
        <div class={styles.boardToolbar}>
          <label class={styles.boardSelectWrap}>
            <span class={styles.srOnly}>Active board</span>
            <select
              class={styles.boardSelect}
              value={boardState.activeBoardId}
              onChange={(e) => void setActiveBoard(e.currentTarget.value)}
              disabled={!boardState.stagesLoaded}
            >
              <For each={boardState.boards}>
                {(b) => (
                  <option value={b.id}>{b.name || b.id}</option>
                )}
              </For>
            </select>
          </label>
          <button
            type="button"
            class={styles.iconBtn}
            onClick={() => void onAddBoard()}
            title="Add board"
            aria-label="Add board"
          >
            <TbOutlinePlus size={16} />
          </button>
          <Show when={boardState.activeBoardId !== "default"}>
            <button
              type="button"
              class={styles.iconBtn}
              onClick={() => void onDeleteBoard()}
              title="Delete current board"
              aria-label="Delete current board"
            >
              <TbOutlineTrash size={16} />
            </button>
          </Show>
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
        <Show when={mc.totalAttentionCount() > 0}>
          <span class={mcStyles.attentionBadge}>
            {mc.totalAttentionCount()} need attention
          </span>
        </Show>
        <button
          class={mcStyles.filterToggle}
          classList={{ [mcStyles.filterToggleActive]: mc.globalFilterActive() }}
          onClick={mc.toggleGlobalFilter}
          title={mc.globalFilterActive() ? "Show all tasks" : "Focus on tasks needing attention"}
        >
          {mc.globalFilterActive()
            ? <><TbOutlineEye size={14} /> Show All</>
            : <><TbOutlineFocus size={14} /> Focus</>}
        </button>
      </div>

      <Show when={projectState.loaded && projectState.projects.length === 0}>
        <div class={mcStyles.onboarding} data-testid="mc-onboarding">
          <strong>No project yet.</strong> Add one under{" "}
          <a href="/settings">Settings → Projects</a> with a name and the path to a Git repo on
          disk. Integrations (Jira, GitHub) and repo-scoped features use the active project.
        </div>
      </Show>

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
                  stageDef.id === "backlog" && hasIssueIntegration()
                    ? () => (
                        <ImportIssuesMenu
                          jiraConnected={settingsState.jiraConfig.connected}
                          githubConnected={settingsState.githubConfig.connected}
                          onSelectJira={() => setJiraImportOpen(true)}
                          onSelectGitHub={() => setGitHubImportOpen(true)}
                        />
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
      />
      <GitHubImport
        isOpen={githubImportOpen()}
        onClose={() => setGitHubImportOpen(false)}
      />
    </div>
  );
};

export default BoardView;
