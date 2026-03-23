import type { Component } from "solid-js";
import { Show } from "solid-js";
import { A, useLocation } from "@solidjs/router";
import { attentionCount } from "./attentionStore";
import AgentList from "./AgentList";
import styles from "./Sidebar.module.css";

// ---------------------------------------------------------------------------
// Nav item icons (simple text/emoji placeholders until icon lib added)
// ---------------------------------------------------------------------------

const NAV_ICONS: Record<string, string> = {
  "/triage": "⚡",
  "/board": "▦",
  "/agents": "◉",
};

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

interface Props {
  collapsed: boolean;
  onToggle: () => void;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const Sidebar: Component<Props> = (props) => {
  const location = useLocation();

  const isActive = (href: string) => location.pathname === href || (href !== "/" && location.pathname.startsWith(href));

  const navItems = [
    { href: "/triage", label: "Triage" },
    { href: "/board", label: "Board" },
    { href: "/agents", label: "Agents" },
  ];

  return (
    <aside
      class={styles.sidebar}
      classList={{ [styles.collapsed]: props.collapsed }}
    >
      {/* Toggle button */}
      <button
        class={styles.collapseBtn}
        onClick={props.onToggle}
        title={props.collapsed ? "Expand sidebar" : "Collapse sidebar"}
        aria-label={props.collapsed ? "Expand sidebar" : "Collapse sidebar"}
      >
        {props.collapsed ? "→" : "←"}
      </button>

      {/* Nav links */}
      <nav class={styles.nav}>
        {navItems.map((item) => {
          const count = () => item.href === "/triage" ? attentionCount() : 0;
          return (
            <A
              href={item.href}
              class={styles.navItem}
              classList={{ [styles.active]: isActive(item.href) }}
            >
              <span class={styles.navIcon}>{NAV_ICONS[item.href]}</span>
              <Show when={!props.collapsed}>
                <span class={styles.navLabel}>{item.label}</span>
              </Show>
              <Show when={count() > 0}>
                <span
                  class={styles.badge}
                  title={`${count()} item(s) needing attention`}
                >
                  {count()}
                </span>
              </Show>
            </A>
          );
        })}
      </nav>

      {/* Agent list */}
      <Show when={!props.collapsed}>
        <AgentList />
      </Show>
    </aside>
  );
};

export default Sidebar;
