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
import { Dialog } from "@kobalte/core/dialog";
import { TbOutlineX, TbOutlineCheck, TbOutlineAlertCircle } from "solid-icons/tb";
import styles from "./JiraImport.module.css";
import { fetchJiraStatus } from "./settingsStore";

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
  /** Server may omit or null when Jira has no priority. */
  priority?: string | null;
}

export interface JiraImportProps {
  isOpen: boolean;
  onClose: () => void;
  /** Active board column id for `TaskCreated.initial_stage` (first column). Omit for default `backlog`. */
  targetStageId?: string;
}

type ImportStatus = "idle" | "importing" | "success" | "error";

// ---------------------------------------------------------------------------
// API helpers
// ---------------------------------------------------------------------------

/** Server returns `{ error: string }` on failure; surface that instead of bare status codes. */
function formatJiraHttpError(status: number, bodyText: string): string {
  let detail: string | null = null;
  const t = bodyText.trim();
  if (t) {
    try {
      const j = JSON.parse(t) as { error?: unknown };
      if (typeof j.error === "string" && j.error.length > 0) detail = j.error;
    } catch {
      detail = t.length > 200 ? `${t.slice(0, 200)}…` : t;
    }
  }
  const core = detail ?? `HTTP ${status}`;
  if (status === 401 || status === 403) {
    return `${core} — Connect Jira in Settings → Integrations, or disconnect and sign in again if the token expired.`;
  }
  return core;
}

async function parseJsonOrThrow<T>(response: Response): Promise<T> {
  const text = await response.text();
  if (!response.ok) {
    throw new Error(formatJiraHttpError(response.status, text));
  }
  return JSON.parse(text) as T;
}

async function fetchProjects(): Promise<JiraProject[]> {
  const response = await fetch("/api/integrations/jira/projects");
  return parseJsonOrThrow<JiraProject[]>(response);
}

/** Server `GET /search` expects `jql` (optional `cloud_id`). */
function buildJiraSearchJql(projectKey: string, userJql: string): string {
  const j = userJql.trim();
  const esc = (k: string) => `"${k.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
  if (projectKey) {
    const proj = `project = ${esc(projectKey)}`;
    if (j) return `${proj} AND (${j})`;
    return `${proj} ORDER BY updated DESC`;
  }
  if (j) return j;
  return "created >= -30d ORDER BY updated DESC";
}

async function searchIssues(
  projectKey: string,
  userJql: string,
): Promise<JiraIssue[]> {
  const jql = buildJiraSearchJql(projectKey, userJql);
  const params = new URLSearchParams({ jql });
  const response = await fetch(`/api/integrations/jira/search?${params}`);
  return parseJsonOrThrow<JiraIssue[]>(response);
}

async function importIssues(
  issueKeys: string[],
  targetStageId?: string,
): Promise<{ imported: string[] }> {
  const body: Record<string, unknown> = { issue_keys: issueKeys };
  const stage = targetStageId?.trim();
  if (stage) body.initialStage = stage;
  const response = await fetch("/api/integrations/jira/import", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return parseJsonOrThrow<{ imported: string[] }>(response);
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

  // ---- Projects resource (only while dialog is open; avoids eager fetch + crash on error) ----
  const [projects] = createResource(
    () => props.isOpen,
    async (open) => {
      if (!open) return [] as JiraProject[];
      // After a server restart, localStorage may still say "connected" while the in-memory
      // token cache is empty until /status runs — refresh before listing projects.
      await fetchJiraStatus();
      return fetchProjects();
    },
  );

  /** Safe list: never call `projects()` when state is `errored` — Solid rethrows. */
  const projectOptions = (): JiraProject[] => {
    const st = projects.state;
    if (st === "errored") return [];
    if (st !== "ready" && st !== "refreshing") return [];
    return projects() ?? [];
  };

  const projectsFetchError = () =>
    projects.state === "errored" ? (projects.error as Error) : null;

  // ---- Search results resource ----
  const [searchResults] = createResource(
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
      const result = await importIssues(keys, props.targetStageId);
      setImportedCount(result.imported.length);
      setImportStatus("success");
      setSelectedKeys(new Set<string>());
    } catch (err) {
      setImportError(err instanceof Error ? err.message : "Import failed");
      setImportStatus("error");
    }
  };

  const results = (): JiraIssue[] => {
    const st = searchResults.state;
    if (st === "errored") return [];
    if (st !== "ready" && st !== "refreshing") return [];
    return searchResults() ?? [];
  };
  const isSearching = () => searchResults.loading;
  const searchError = () => searchResults.error as Error | null;
  const selectedCount = () => selectedKeys().size;
  const allSelected = () =>
    results().length > 0 && results().every((i) => selectedKeys().has(i.key));

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
            <Dialog.Title class={styles.dialogTitle}>Import from Jira</Dialog.Title>
            <Dialog.CloseButton class={styles.closeBtn} aria-label="Close dialog">
              <TbOutlineX size={14} />
            </Dialog.CloseButton>
          </div>
          <Dialog.Description class={styles.srOnly}>
            Search Jira with optional project and JQL, then import selected issues.
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

              <Show when={importStatus() === "success"}>
                <div class={styles.successBanner}>
                  <TbOutlineCheck size={14} /> Imported {importedCount()} issue{importedCount() !== 1 ? "s" : ""}
                </div>
              </Show>

              {/* Search controls */}
              <div class={styles.searchRow}>
                <Show
                  when={projects.loading}
                  fallback={
                    <Show
                      when={projectsFetchError()}
                      fallback={
                        <select
                          class={styles.projectSelect}
                          value={selectedProject()}
                          onChange={(e) => setSelectedProject(e.currentTarget.value)}
                        >
                          <option value="">All projects</option>
                          <For each={projectOptions()}>
                            {(p) => (
                              <option value={p.key}>
                                {p.key} — {p.name}
                              </option>
                            )}
                          </For>
                        </select>
                      }
                    >
                      <div class={styles.errorState}>
                        <TbOutlineAlertCircle size={14} />{" "}
                        {projectsFetchError()?.message ?? "Could not load Jira projects"}
                      </div>
                    </Show>
                  }
                >
                  <select class={styles.projectSelect} disabled>
                    <option>Loading projects…</option>
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
                  <div class={styles.selectAllRow}>
                    <input
                      type="checkbox"
                      checked={allSelected()}
                      onChange={toggleSelectAll}
                      id="jira-select-all-issues"
                    />
                    <label for="jira-select-all-issues">
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
                                <span class={styles.resultPriority}>
                                  {issue.priority ?? "—"}
                                </span>
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

export default JiraImport;
