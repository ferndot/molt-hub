/**
 * JiraImport — modal dialog for searching and importing Jira issues.
 *
 * Opens over any view. Controlled via isOpen / onClose props.
 */

import {
  createSignal,
  createResource,
  Show,
  For,
  type Component,
} from "solid-js";
import { Portal } from "solid-js/web";
import styles from "./JiraImport.module.css";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface JiraProject {
  key: string;
  name: string;
}

export interface JiraIssue {
  key: string;
  summary: string;
  status: string;
  priority: string;
}

export interface JiraImportProps {
  isOpen: boolean;
  onClose: () => void;
}

type ImportStatus = "idle" | "importing" | "success" | "error";

// ---------------------------------------------------------------------------
// API helpers
// ---------------------------------------------------------------------------

async function fetchProjects(): Promise<JiraProject[]> {
  const response = await fetch("/api/integrations/jira/projects");
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json() as Promise<JiraProject[]>;
}

async function searchIssues(
  projectKey: string,
  jql: string,
): Promise<JiraIssue[]> {
  const params = new URLSearchParams();
  if (projectKey) params.set("project", projectKey);
  if (jql) params.set("jql", jql);
  const response = await fetch(`/api/integrations/jira/search?${params}`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json() as Promise<JiraIssue[]>;
}

async function importIssues(keys: string[]): Promise<{ imported: number }> {
  const response = await fetch("/api/integrations/jira/import", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ keys }),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json() as Promise<{ imported: number }>;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const JiraImport: Component<JiraImportProps> = (props) => {
  // ---- State ----
  const [selectedProject, setSelectedProject] = createSignal("");
  const [jql, setJql] = createSignal("");
  const [searchTrigger, setSearchTrigger] = createSignal<{
    project: string;
    jql: string;
  } | null>(null);
  const [selectedKeys, setSelectedKeys] = createSignal<Set<string>>(new Set());
  const [importStatus, setImportStatus] = createSignal<ImportStatus>("idle");
  const [importError, setImportError] = createSignal<string | null>(null);
  const [importedCount, setImportedCount] = createSignal(0);

  // ---- Projects resource ----
  const [projects] = createResource<JiraProject[]>(fetchProjects);

  // ---- Search results resource ----
  const [searchResults, { refetch: refetchSearch }] = createResource(
    searchTrigger,
    async (trigger) => {
      if (!trigger) return [];
      return searchIssues(trigger.project, trigger.jql);
    },
  );

  // ---- Handlers ----
  const handleSearch = () => {
    setSelectedKeys(new Set<string>());
    setImportStatus("idle");
    setImportError(null);
    setSearchTrigger({ project: selectedProject(), jql: jql() });
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") handleSearch();
    if (e.key === "Escape") props.onClose();
  };

  const toggleSelect = (key: string) => {
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  };

  const toggleSelectAll = () => {
    const results = searchResults() ?? [];
    const allSelected = results.every((i) => selectedKeys().has(i.key));
    if (allSelected) {
      setSelectedKeys(new Set<string>());
    } else {
      setSelectedKeys(new Set(results.map((i) => i.key)));
    }
  };

  const handleImport = async () => {
    const keys = [...selectedKeys()];
    if (keys.length === 0) return;
    setImportStatus("importing");
    setImportError(null);
    try {
      const result = await importIssues(keys);
      setImportedCount(result.imported);
      setImportStatus("success");
      setSelectedKeys(new Set<string>());
    } catch (err) {
      setImportError(err instanceof Error ? err.message : "Import failed");
      setImportStatus("error");
    }
  };

  const results = () => searchResults() ?? [];
  const isSearching = () => searchResults.loading;
  const searchError = () => searchResults.error as Error | null;
  const selectedCount = () => selectedKeys().size;
  const allSelected = () =>
    results().length > 0 && results().every((i) => selectedKeys().has(i.key));

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
          aria-label="Import from Jira"
        >
          <div class={styles.dialog}>
            {/* Header */}
            <div class={styles.dialogHeader}>
              <span class={styles.dialogTitle}>Import from Jira</span>
              <button
                class={styles.closeBtn}
                onClick={props.onClose}
                aria-label="Close dialog"
              >
                ✕
              </button>
            </div>

            {/* Body */}
            <div class={styles.dialogBody}>
              {/* Error state */}
              <Show when={searchError()}>
                <div class={styles.errorState}>
                  Search failed: {searchError()?.message}
                </div>
              </Show>

              <Show when={importStatus() === "error" && importError()}>
                <div class={styles.errorState}>Import failed: {importError()}</div>
              </Show>

              <Show when={importStatus() === "success"}>
                <div class={styles.successBanner}>
                  Successfully imported {importedCount()} issue
                  {importedCount() !== 1 ? "s" : ""}.
                </div>
              </Show>

              {/* Search controls */}
              <div class={styles.searchRow}>
                <Show
                  when={!projects.loading}
                  fallback={
                    <select class={styles.projectSelect} disabled>
                      <option>Loading projects…</option>
                    </select>
                  }
                >
                  <select
                    class={styles.projectSelect}
                    value={selectedProject()}
                    onChange={(e) => setSelectedProject(e.currentTarget.value)}
                  >
                    <option value="">All projects</option>
                    <For each={projects() ?? []}>
                      {(p) => (
                        <option value={p.key}>
                          {p.key} — {p.name}
                        </option>
                      )}
                    </For>
                  </select>
                </Show>

                <input
                  class={styles.jqlInput}
                  type="text"
                  placeholder="JQL query (e.g. status = 'In Progress')"
                  value={jql()}
                  onInput={(e) => setJql(e.currentTarget.value)}
                  onKeyDown={handleKeyDown}
                />

                <button
                  class={styles.searchBtn}
                  disabled={isSearching()}
                  onClick={handleSearch}
                >
                  {isSearching() ? "Searching…" : "Search"}
                </button>
              </div>

              {/* Results */}
              <Show when={!isSearching()} fallback={<div class={styles.loadingState}>Searching Jira…</div>}>
                <Show
                  when={results().length > 0}
                  fallback={
                    <Show when={searchTrigger() !== null}>
                      <div class={styles.emptyState}>
                        No issues found. Try adjusting your JQL query.
                      </div>
                    </Show>
                  }
                >
                  {/* Select-all row */}
                  <div
                    style={{
                      "display": "flex",
                      "align-items": "center",
                      "gap": "0.5rem",
                      "margin-bottom": "0.5rem",
                      "font-size": "0.8125rem",
                      "color": "var(--text-muted)",
                    }}
                  >
                    <input
                      type="checkbox"
                      checked={allSelected()}
                      onChange={toggleSelectAll}
                      id="select-all-issues"
                    />
                    <label for="select-all-issues">
                      Select all ({results().length})
                    </label>
                  </div>

                  <ul class={styles.resultsList}>
                    <For each={results()}>
                      {(issue) => {
                        const isSelected = () => selectedKeys().has(issue.key);
                        return (
                          <li
                            class={`${styles.resultItem}${isSelected() ? ` ${styles.resultItemSelected}` : ""}`}
                            onClick={() => toggleSelect(issue.key)}
                          >
                            <input
                              type="checkbox"
                              class={styles.resultCheckbox}
                              checked={isSelected()}
                              onChange={() => toggleSelect(issue.key)}
                              onClick={(e) => e.stopPropagation()}
                            />
                            <div class={styles.resultContent}>
                              <div>
                                <span class={styles.resultKey}>{issue.key}</span>
                                <span class={styles.resultSummary}>{issue.summary}</span>
                              </div>
                              <div class={styles.resultMeta}>
                                <span class={styles.resultStatus}>{issue.status}</span>
                                <span class={styles.resultPriority}>{issue.priority}</span>
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

export default JiraImport;
