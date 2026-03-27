/**
 * GitHubImport — modal dialog for searching and importing GitHub issues.
 *
 * Opens over any view. Controlled via isOpen / onClose props.
 * Mirrors the JiraImport pattern.
 */

import {
  createSignal,
  createResource,
  Show,
  For,
  type Component,
} from "solid-js";
import { Dialog } from "@kobalte/core/dialog";
import { TbOutlineX, TbOutlineCheck, TbOutlineAlertCircle } from "solid-icons/tb";
import { settingsState } from "./settingsStore";
import styles from "./GitHubImport.module.css";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface GitHubIssue {
  number: number;
  title: string;
  state: string;
  labels: string[];
}

export interface GitHubImportProps {
  isOpen: boolean;
  onClose: () => void;
  /** Active board column id for `TaskCreated.initial_stage` (first column). Omit for default `backlog`. */
  targetStageId?: string;
  /** Active board id so imported tasks appear on the correct board. */
  targetBoardId?: string;
}

type IssueStateFilter = "open" | "closed" | "all";
type ImportStatus = "idle" | "importing" | "success" | "error";

// ---------------------------------------------------------------------------
// API helpers
// ---------------------------------------------------------------------------

const FETCH_TIMEOUT_MS = 45_000;

async function readFetchErrorMessage(response: Response): Promise<string> {
  const text = await response.text();
  try {
    const body = JSON.parse(text) as { error?: string };
    if (body.error?.trim()) return body.error.trim();
  } catch {
    /* use raw text */
  }
  const t = text.trim();
  if (t) return t.slice(0, 200);
  return `HTTP ${response.status}`;
}

async function searchIssues(
  owner: string,
  repo: string,
  state: IssueStateFilter,
  labels: string,
): Promise<GitHubIssue[]> {
  const params = new URLSearchParams();
  params.set("owner", owner);
  params.set("repo", repo);
  params.set("state", state);
  if (labels) params.set("labels", labels);
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  try {
    const response = await fetch(`/api/integrations/github/issues?${params}`, {
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(await readFetchErrorMessage(response));
    return response.json() as Promise<GitHubIssue[]>;
  } catch (err) {
    if (err instanceof Error && err.name === "AbortError") {
      throw new Error("GitHub search timed out. Check your network and try again.");
    }
    throw err;
  } finally {
    window.clearTimeout(timer);
  }
}

/** `POST /import` body matches server `ImportRequest`: owner, repo, issues (numbers). */
async function importIssues(
  owner: string,
  repo: string,
  issueNumbers: number[],
  targetStageId?: string,
  boardId?: string,
): Promise<{ imported: number; message?: string }> {
  const body: Record<string, unknown> = { owner, repo, issues: issueNumbers };
  const stage = targetStageId?.trim();
  if (stage) body.initialStage = stage;
  if (boardId) body.boardId = boardId;
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  try {
    const response = await fetch("/api/integrations/github/import", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(await readFetchErrorMessage(response));
    return response.json() as Promise<{ imported: number; message?: string }>;
  } catch (err) {
    if (err instanceof Error && err.name === "AbortError") {
      throw new Error("GitHub import timed out. Try again with fewer issues.");
    }
    throw err;
  } finally {
    window.clearTimeout(timer);
  }
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const GitHubImport: Component<GitHubImportProps> = (props) => {
  const githubOwner = () => settingsState.githubConfig.owner?.trim() ?? "";
  const canSearch = () =>
    settingsState.githubConfig.connected && githubOwner().length > 0;

  // ---- State ----
  const [repo, setRepo] = createSignal("");
  const [stateFilter, setStateFilter] = createSignal<IssueStateFilter>("open");
  const [labelFilter, setLabelFilter] = createSignal("");
  const [searchTrigger, setSearchTrigger] = createSignal<{
    owner: string;
    repo: string;
    state: IssueStateFilter;
    labels: string;
  } | null>(null);
  const [selectedNumbers, setSelectedNumbers] = createSignal<Set<number>>(new Set());
  const [importStatus, setImportStatus] = createSignal<ImportStatus>("idle");
  const [importError, setImportError] = createSignal<string | null>(null);
  const [importedCount, setImportedCount] = createSignal(0);

  // ---- Search results resource ----
  const [searchResults] = createResource(
    searchTrigger,
    async (trigger) => {
      if (!trigger) return [];
      return searchIssues(trigger.owner, trigger.repo, trigger.state, trigger.labels);
    },
  );

  // ---- Handlers ----
  const handleSearch = () => {
    const owner = githubOwner();
    let repoName = repo().trim();
    if (!repoName || !owner) return;
    // Avoid `repo:owner/owner/name` when the user pastes a full `owner/repo` path.
    const prefix = `${owner}/`;
    if (repoName.toLowerCase().startsWith(prefix.toLowerCase())) {
      repoName = repoName.slice(prefix.length);
    }
    if (!repoName) return;
    setSelectedNumbers(new Set<number>());
    setImportStatus("idle");
    setImportError(null);
    setSearchTrigger({
      owner,
      repo: repoName,
      state: stateFilter(),
      labels: labelFilter().trim(),
    });
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") handleSearch();
  };

  const toggleSelect = (num: number) => {
    setSelectedNumbers((prev) => {
      const next = new Set(prev);
      if (next.has(num)) {
        next.delete(num);
      } else {
        next.add(num);
      }
      return next;
    });
  };

  const toggleSelectAll = () => {
    const items = results();
    const allSelected = items.every((i) => selectedNumbers().has(i.number));
    if (allSelected) {
      setSelectedNumbers(new Set<number>());
    } else {
      setSelectedNumbers(new Set(items.map((i) => i.number)));
    }
  };

  const handleImport = async () => {
    const nums = [...selectedNumbers()];
    if (nums.length === 0) return;
    const trigger = searchTrigger();
    if (!trigger) return;
    setImportStatus("importing");
    setImportError(null);
    try {
      const result = await importIssues(
        trigger.owner,
        trigger.repo,
        nums,
        props.targetStageId,
        props.targetBoardId,
      );
      setImportedCount(result.imported);
      setImportStatus("success");
      setSelectedNumbers(new Set<number>());
    } catch (err) {
      setImportError(err instanceof Error ? err.message : "Import failed");
      setImportStatus("error");
    }
  };

  // Avoid calling the resource accessor when errored — Solid's read() rethrows.
  const results = (): GitHubIssue[] => {
    const st = searchResults.state;
    if (st === "errored") return [];
    if (st !== "ready" && st !== "refreshing") return [];
    return searchResults() ?? [];
  };
  const isSearching = () => searchResults.loading;
  const searchError = () => searchResults.error as Error | null;
  const selectedCount = () => selectedNumbers().size;
  const allSelected = () =>
    results().length > 0 && results().every((i) => selectedNumbers().has(i.number));

  // ---- Render ----
  return (
    <Dialog
      open={props.isOpen}
      onOpenChange={(isOpen: boolean) => {
        if (!isOpen) props.onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay class={styles.overlay} />
        <Dialog.Content class={styles.dialog}>
          <div class={styles.dialogHeader}>
            <Dialog.Title class={styles.dialogTitle}>Import from GitHub</Dialog.Title>
            <Dialog.CloseButton class={styles.closeBtn} aria-label="Close dialog">
              <TbOutlineX size={14} />
            </Dialog.CloseButton>
          </div>
          <Dialog.Description class={styles.srOnly}>
            Search GitHub issues by repository and filters, then import selected issues.
          </Dialog.Description>

            {/* Body */}
            <div class={styles.dialogBody}>
              {/* Error state */}
              <Show when={searchError()}>
                <div class={styles.errorState}>
                  <TbOutlineAlertCircle size={14} /> {searchError()?.message}
                </div>
              </Show>

              <Show when={importStatus() === "error" && importError()}>
                <div class={styles.errorState}><TbOutlineAlertCircle size={14} /> {importError()}</div>
              </Show>

              <Show
                when={
                  settingsState.githubConfig.connected && githubOwner().length === 0
                }
              >
                <div class={styles.errorState}>
                  <TbOutlineAlertCircle size={14} /> GitHub login not loaded yet.
                  Open Settings → GitHub or wait for connection status to refresh.
                </div>
              </Show>

              <Show when={importStatus() === "success"}>
                <div class={styles.successBanner}>
                  <TbOutlineCheck size={14} /> Imported {importedCount()} issue{importedCount() !== 1 ? "s" : ""}
                </div>
              </Show>

              {/* Search controls */}
              <div class={styles.searchRow}>
                <input
                  class={styles.repoInput}
                  type="text"
                  placeholder="Repository name (e.g. my-project)"
                  value={repo()}
                  onInput={(e) => setRepo(e.currentTarget.value)}
                  onKeyDown={handleKeyDown}
                />

                <button
                  class={styles.searchBtn}
                  disabled={isSearching() || !repo().trim() || !canSearch()}
                  onClick={handleSearch}
                >
                  {isSearching() ? "Searching…" : "Search"}
                </button>
              </div>

              {/* Filters */}
              <div class={styles.filterRow}>
                <span class={styles.filterLabel}>State:</span>
                <select
                  class={styles.filterSelect}
                  value={stateFilter()}
                  onChange={(e) => setStateFilter(e.currentTarget.value as IssueStateFilter)}
                >
                  <option value="open">Open</option>
                  <option value="closed">Closed</option>
                  <option value="all">All</option>
                </select>

                <span class={styles.filterLabel}>Labels:</span>
                <input
                  class={styles.labelInput}
                  type="text"
                  placeholder="e.g. bug, enhancement"
                  value={labelFilter()}
                  onInput={(e) => setLabelFilter(e.currentTarget.value)}
                  onKeyDown={handleKeyDown}
                />
              </div>

              {/* Results */}
              <Show when={!isSearching()} fallback={<div class={styles.loadingState}>Searching GitHub…</div>}>
                <Show
                  when={results().length > 0}
                  fallback={
                    <Show when={searchTrigger() !== null}>
                      <div class={styles.emptyState}>
                        No issues found. Try adjusting your filters.
                      </div>
                    </Show>
                  }
                >
                  <div class={styles.selectAllRow}>
                    <input
                      type="checkbox"
                      checked={allSelected()}
                      onChange={toggleSelectAll}
                      id="gh-select-all-issues"
                    />
                    <label for="gh-select-all-issues">
                      Select all ({results().length})
                    </label>
                  </div>

                  <ul class={styles.resultsList}>
                    <For each={results()}>
                      {(issue) => {
                        const isSelected = () => selectedNumbers().has(issue.number);
                        return (
                          <li
                            class={`${styles.resultItem}${isSelected() ? ` ${styles.resultItemSelected}` : ""}`}
                            onClick={() => toggleSelect(issue.number)}
                          >
                            <input
                              type="checkbox"
                              class={styles.resultCheckbox}
                              checked={isSelected()}
                              onChange={() => toggleSelect(issue.number)}
                              onClick={(e) => e.stopPropagation()}
                            />
                            <div class={styles.resultContent}>
                              <div>
                                <span class={styles.resultNumber}>#{issue.number}</span>
                                <span class={styles.resultTitle}>{issue.title}</span>
                              </div>
                              <div class={styles.resultMeta}>
                                <span class={styles.resultState}>{issue.state}</span>
                                <Show when={issue.labels.length > 0}>
                                  <span class={styles.resultLabels}>
                                    {issue.labels.join(", ")}
                                  </span>
                                </Show>
                              </div>
                            </div>
                          </li>
                        );
                      }}
                    </For>
                  </ul>
                </Show>
              </Show>

              {/* Import progress */}
              <Show when={importStatus() === "importing"}>
                <div class={styles.progress}>
                  <div class={styles.progressBar}>
                    <div class={styles.progressFill} style={{ width: "60%" }} />
                  </div>
                  <div class={styles.progressLabel}>Importing issues…</div>
                </div>
              </Show>
            </div>

            {/* Footer */}
            <div class={styles.dialogFooter}>
              <span class={styles.footerLeft}>
                {selectedCount() > 0
                  ? `${selectedCount()} issue${selectedCount() !== 1 ? "s" : ""} selected`
                  : "No issues selected"}
              </span>
              <div class={styles.footerRight}>
                <button class={styles.btnCancel} onClick={props.onClose}>
                  Cancel
                </button>
                <button
                  class={styles.btnImport}
                  disabled={
                    selectedCount() === 0 || importStatus() === "importing"
                  }
                  onClick={handleImport}
                >
                  {importStatus() === "importing"
                    ? "Importing…"
                    : `Import Selected (${selectedCount()})`}
                </button>
              </div>
            </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  );
};

export default GitHubImport;
