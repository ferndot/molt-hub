import { createSignal, For, type Component } from "solid-js";
import { A } from "@solidjs/router";
import styles from "./AgentList.module.css";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type AgentStatus = "running" | "waiting" | "blocked" | "done";

interface MockAgent {
  id: string;
  name: string;
  status: AgentStatus;
  stage: string;
}

// ---------------------------------------------------------------------------
// Mock data (replaced by real data from T25/T29)
// ---------------------------------------------------------------------------

const MOCK_AGENTS: MockAgent[] = [
  { id: "agent-001", name: "frontend", status: "running", stage: "Working" },
  { id: "agent-002", name: "backend-api", status: "waiting", stage: "Needs Review" },
  { id: "agent-003", name: "core-engine", status: "blocked", stage: "Blocked" },
  { id: "agent-004", name: "infra", status: "done", stage: "Completed" },
];

const STATUS_COLOR: Record<AgentStatus, string> = {
  running: "#22c55e",
  waiting: "#f59e0b",
  blocked: "#e63946",
  done: "#6b7280",
};

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface Props {
  collapsed?: boolean;
}

const AgentList: Component<Props> = (props) => {
  const [query, setQuery] = createSignal("");

  const filteredAgents = () => {
    const q = query().toLowerCase().trim();
    if (!q) return MOCK_AGENTS;
    return MOCK_AGENTS.filter(
      (a) =>
        a.name.toLowerCase().includes(q) ||
        a.stage.toLowerCase().includes(q),
    );
  };

  return (
    <div class={styles.section}>
      <div class={styles.sectionTitle}>Agents</div>
      <div class={styles.searchWrapper}>
        <input
          class={styles.searchInput}
          type="search"
          placeholder="Search agents..."
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          aria-label="Search agents"
        />
      </div>
      <For each={filteredAgents()}>
        {(agent) => (
          <A href={`/agents/${agent.id}`} class={styles.agentItem}>
            <span
              class={styles.statusDot}
              style={{ background: STATUS_COLOR[agent.status] }}
              title={agent.status}
            />
            <div class={styles.agentInfo}>
              <div class={styles.agentName}>{agent.name}</div>
              <div class={styles.agentStage}>{agent.stage}</div>
            </div>
          </A>
        )}
      </For>
    </div>
  );
};

export default AgentList;
