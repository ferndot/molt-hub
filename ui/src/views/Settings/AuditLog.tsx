/**
 * AuditLog — table of recent audit entries, shown as a tab in SettingsView.
 *
 * Fetches from GET /api/audit and displays entries in a filterable table.
 */

import { createSignal, createMemo, onMount, For, Show, type Component } from "solid-js";
import { api } from "../../lib/api";
import type { AuditEntry } from "../../lib/api";
import styles from "./AuditLog.module.css";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString("en-US", {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    });
  } catch {
    return iso;
  }
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const AuditLog: Component = () => {
  const [entries, setEntries] = createSignal<AuditEntry[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);
  const [actionFilter, setActionFilter] = createSignal("all");

  onMount(async () => {
    try {
      const data = await api.getAuditLog(200);
      setEntries(data.entries ?? []);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load audit log");
    } finally {
      setLoading(false);
    }
  });

  // Unique action types for the filter dropdown
  const actionTypes = createMemo(() => {
    const types = new Set<string>();
    for (const e of entries()) {
      if (e.action) types.add(e.action);
    }
    return Array.from(types).sort();
  });

  const filteredEntries = createMemo(() => {
    const filter = actionFilter();
    if (filter === "all") return entries();
    return entries().filter((e) => e.action === filter);
  });

  return (
    <div class={styles.container}>
      <h3>Audit Log</h3>

      <div class={styles.headerRow}>
        <label>
          Filter by action:{" "}
          <select
            class={styles.filterSelect}
            value={actionFilter()}
            onChange={(e) => setActionFilter(e.currentTarget.value)}
          >
            <option value="all">All</option>
            <For each={actionTypes()}>
              {(action) => <option value={action}>{action}</option>}
            </For>
          </select>
        </label>
      </div>

      <Show when={loading()}>
        <div class={styles.loadingState}>Loading audit log...</div>
      </Show>

      <Show when={error()}>
        <div class={styles.errorState}>{error()}</div>
      </Show>

      <Show when={!loading() && !error()}>
        <Show
          when={filteredEntries().length > 0}
          fallback={<div class={styles.emptyState}>No audit entries found.</div>}
        >
          <table class={styles.table}>
            <thead>
              <tr>
                <th>Timestamp</th>
                <th>Action</th>
                <th>Actor</th>
                <th>Details</th>
              </tr>
            </thead>
            <tbody>
              <For each={filteredEntries()}>
                {(entry) => (
                  <tr>
                    <td class={styles.timestampCell}>
                      {formatTimestamp(entry.timestamp)}
                    </td>
                    <td class={styles.actionCell}>{entry.action}</td>
                    <td class={styles.actorCell}>{entry.actor}</td>
                    <td class={styles.detailsCell} title={entry.details}>
                      {entry.details}
                    </td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </Show>
      </Show>
    </div>
  );
};

export default AuditLog;
