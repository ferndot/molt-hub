import { createSignal, createMemo, For, Show, onMount, onCleanup, type Component } from "solid-js";
import { A } from "@solidjs/router";
import { TbOutlineSearch } from "solid-icons/tb";
import { useAgentDetailStore, startAgentPolling, type AgentDetail } from "../AgentDetail/agentStore";
import styles from "./AgentsView.module.css";

// ---------------------------------------------------------------------------
// Status mapping — agentStore uses running/paused/terminated/idle;
// the list view normalises these to the UI status vocabulary.
// ---------------------------------------------------------------------------

type ListStatus = "running" | "waiting" | "blocked" | "done";

const STATUS_COLOR: Record<ListStatus, string> = {
  running: "#22c55e",
  waiting: "#f59e0b",
  blocked: "#e63946",
  done: "#6b7280",
};

function toListStatus(s: AgentDetail["status"]): ListStatus {
  switch (s) {
    case "running":
      return "running";
    case "paused":
      return "waiting";
    case "terminated":
      return "blocked";
    case "idle":
      return "done";
  }
}

// ---------------------------------------------------------------------------
// Duration helper
// ---------------------------------------------------------------------------

function formatDuration(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  if (ms < 0) return "0m";
  const totalMinutes = Math.floor(ms / 60_000);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

// ---------------------------------------------------------------------------
// Filter tabs config
// ---------------------------------------------------------------------------

const TABS: { label: string; value: ListStatus | "all" }[] = [
  { label: "All", value: "all" },
  { label: "Running", value: "running" },
  { label: "Waiting", value: "waiting" },
  { label: "Blocked", value: "blocked" },
  { label: "Done", value: "done" },
];

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const AgentsView: Component = () => {
  const { state } = useAgentDetailStore();

  // Poll real agent data from the backend every 3 seconds
  onMount(() => {
    const stopPolling = startAgentPolling(3000);
    onCleanup(stopPolling);
  });

  const [query, setQuery] = createSignal("");
  const [activeTab, setActiveTab] = createSignal<ListStatus | "all">("all");

  const filtered = createMemo(() => {
    const q = query().toLowerCase().trim();
    const tab = activeTab();

    return state.agents.filter((a) => {
      // Status tab filter
      if (tab !== "all" && toListStatus(a.status) !== tab) return false;

      // Text search filter
      if (q) {
        const haystack = `${a.name} ${a.taskName} ${a.currentStage} ${a.status}`.toLowerCase();
        if (!haystack.includes(q)) return false;
      }

      return true;
    });
  });

  const countByStatus = createMemo(() => {
    const counts: Record<ListStatus | "all", number> = {
      all: state.agents.length,
      running: 0,
      waiting: 0,
      blocked: 0,
      done: 0,
    };
    for (const a of state.agents) {
      counts[toListStatus(a.status)]++;
    }
    return counts;
  });

  return (
    <div class={styles.container}>
      {/* Header */}
      <div class={styles.header}>
        <h2 class={styles.title}>Agents</h2>
        <span class={styles.countBadge}>{state.agents.length}</span>
        <input
          class={styles.searchInput}
          type="search"
          placeholder="Filter agents..."
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          aria-label="Filter agents"
        />
      </div>

      {/* Status filter tabs */}
      <div class={styles.tabs}>
        <For each={TABS}>
          {(tab) => (
            <button
              class={`${styles.tab} ${activeTab() === tab.value ? styles.tabActive : ""}`}
              onClick={() => setActiveTab(tab.value)}
            >
              {tab.label}
              <span class={styles.tabCount}>{countByStatus()[tab.value]}</span>
            </button>
          )}
        </For>
      </div>

      {/* Agent list */}
      <div class={styles.list}>
        <Show
          when={filtered().length > 0}
          fallback={
            <div class={styles.emptyState}>
              <div class={styles.emptyIcon}><TbOutlineSearch size={32} /></div>
              <span>No agents match the current filters.</span>
            </div>
          }
        >
          <For each={filtered()}>
            {(agent) => {
              const ls = () => toListStatus(agent.status);
              return (
                <A href={`/agents/${agent.id}`} class={styles.agentCard}>
                  <span
                    class={styles.statusDot}
                    style={{ background: STATUS_COLOR[ls()] }}
                    title={ls()}
                  />
                  <span class={styles.agentName}>{agent.name}</span>
                  <span class={styles.taskName}>{agent.taskName}</span>
                  <span class={styles.stagePill}>{agent.currentStage}</span>
                  <span class={styles.duration}>{formatDuration(agent.assignedAt)}</span>
                </A>
              );
            }}
          </For>
        </Show>
      </div>
    </div>
  );
};

export default AgentsView;
