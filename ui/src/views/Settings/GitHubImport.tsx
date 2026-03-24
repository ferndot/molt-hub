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
import { Portal } from "solid-js/web";
import { TbOutlineX, TbOutlineCheck, TbOutlineAlertCircle } from "solid-icons/tb";
import { projectState } from "../../stores/projectStore";
import { settingsState } from "./settingsStore";
import styles from "./GitHubImport.module.css";

function appendActiveProjectId(params: URLSearchParams): void {
  const id = projectState.activeProjectId?.trim();
  if (id && id !== "default") params.set("projectId", id);
}

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
}

type IssueStateFilter = "open" | "closed" | "all";
type ImportStatus = "idle" | "importing" | "success" | "error";

// ---------------------------------------------------------------------------
// API helpers
// ---------------------------------------------------------------------------

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
  appendActiveProjectId(params);
  const response = await fetch(`/api/integrations/github/issues?${params}`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json() as Promise<GitHubIssue[]>;
}

/** `POST /import` body matches server `ImportRequest`: owner, repo, issues (numbers). */
async function importIssues(
  owner: string,
  repo: string,
  issueNumbers: number[],
): Promise<{ imported: number; message?: string }> {
  const body: Record<string, unknown> = { owner, repo, issues: issueNumbers };
  const id = projectState.activeProjectId?.trim();
  if (id && id !== "default") body.projectId = id;
  const response = await fetch("/api/integrations/github/import", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json() as Promise<{ imported: number; message?: string }>;
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
    const repoName = repo().trim();
    if (!repoName || !owner) return;
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
    if (e.key === "Escape") props.onClose();
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
      const result = await importIssues(trigger.owner, trigger.repo, nums);
      setImportedCount(result.imported);
      setImportStatus("success");
      setSelectedNumbers(new Set<number>());
    } catch (err) {
      setImportError(err instanceof Error ? err.message : "Import failed");
      setImportStatus("error");
    }
  };

  const results = () => searchResults() ?? [];
  const isSearching = () => searchResults.loading;
  const searchError = () => searchResults.error as Error | null;
  const selectedCount = () => selectedNumbers().size;
  const allSelected = () =>
    results().length > 0 && results().every((i) => selectedNumbers().has(i.number));

  // ---- Render ----
  return (
    <Show when={props.isOpen}>
      <Portal>
        <div
          class={styles.overlay}
          onClick={(e) => {
            if (e.target === e.currentTarget) props.onClose();
          }}
          role="dialog"
          aria-modal="true"
          aria-label="Import from GitHub"
        >
          <div class={styles.dialog}>
            {/* Header */}
            <div class={styles.dialogHeader}>
              <span class={styles.dialogTitle}>Import from GitHub</span>
              <button
                class={styles.closeBtn}
                onClick={props.onClose}
                aria-label="Close dialog"
              >
                <TbOutlineX size={14} />
              </button>
            </div>

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
          </div>
        </div>
      </Portal>
    </Show>
  );
};

export default GitHubImport;
