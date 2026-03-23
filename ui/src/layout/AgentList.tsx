import { createSignal, For, Show, type Component } from "solid-js";
import { A } from "@solidjs/router";
import { TbOutlineChevronDown, TbOutlineChevronRight } from "solid-icons/tb";
import {
  groupAgentsByStatus,
  MOCK_AGENTS,
  STATUS_COLOR,
  type AgentStatus,
} from "./agentListUtils";
import styles from "./AgentList.module.css";

// Re-export types for consumers
export type { AgentStatus, MockAgent, StatusGroup } from "./agentListUtils";
export { groupAgentsByStatus, STATUS_COLOR } from "./agentListUtils";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface Props {
  collapsed?: boolean;
}

const AgentList: Component<Props> = (props) => {
  const [query, setQuery] = createSignal("");

  // Track collapsed state per group. Running expanded by default, others collapsed.
  const [collapsedGroups, setCollapsedGroups] = createSignal<Record<string, boolean>>({
    running: false,
    paused: true,
    idle: true,
    terminated: true,
  });

  const filteredAgents = () => {
    const q = query().toLowerCase().trim();
    if (!q) return MOCK_AGENTS;
    return MOCK_AGENTS.filter(
      (a) =>
        a.name.toLowerCase().includes(q) ||
        a.stage.toLowerCase().includes(q),
    );
  };

  const groups = () => groupAgentsByStatus(filteredAgents());

  const toggleGroup = (status: string) => {
    setCollapsedGroups((prev) => ({
      ...prev,
      [status]: !prev[status],
    }));
  };

  const isGroupCollapsed = (status: string) => !!collapsedGroups()[status];

  return (
    <div class={styles.section} classList={{ [styles.collapsed]: props.collapsed }}>
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

      <Show
        when={!props.collapsed}
        fallback={
          /* Collapsed sidebar: flat list of status dots only */
          <For each={filteredAgents()}>
            {(agent) => (
              <A href={`/agents/${agent.id}`} class={styles.agentItem}>
                <span
                  class={styles.statusDot}
                  style={{ background: STATUS_COLOR[agent.status] }}
                  title={`${agent.name} (${agent.status})`}
                />
              </A>
            )}
          </For>
        }
      >
        {/* Expanded sidebar: grouped with collapsible sections */}
        <For each={groups()}>
          {(group) => (
            <div class={styles.statusGroup}>
              <button
                class={styles.groupHeader}
                onClick={() => toggleGroup(group.status)}
                aria-expanded={!isGroupCollapsed(group.status)}
                aria-label={`${group.label} agents, ${group.agents.length} items`}
              >
                <span class={styles.chevron}>
                  <Show
                    when={!isGroupCollapsed(group.status)}
                    fallback={<TbOutlineChevronRight size={12} />}
                  >
                    <TbOutlineChevronDown size={12} />
                  </Show>
                </span>
                <span class={styles.groupLabel}>{group.label}</span>
                <span class={styles.groupCount}>{group.agents.length}</span>
              </button>

              <div
                class={styles.groupContent}
                classList={{ [styles.groupContentCollapsed]: isGroupCollapsed(group.status) }}
              >
                <For each={group.agents}>
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
            </div>
          )}
        </For>
      </Show>
    </div>
  );
};

export default AgentList;
