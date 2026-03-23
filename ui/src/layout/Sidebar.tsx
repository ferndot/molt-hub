import type { Component, JSX } from "solid-js";
import { Show, createSignal } from "solid-js";
import { A, useLocation } from "@solidjs/router";
import { TbOutlineLayoutDashboard, TbOutlineRobot, TbOutlineSettings } from "solid-icons/tb";
import { attentionCount } from "./attentionStore";
import AgentList from "./AgentList";
import {
  settingsState,
  setNavSidebarWidth,
  NAV_SIDEBAR_MIN,
  NAV_SIDEBAR_MAX,
} from "../views/Settings/settingsStore";
import styles from "./Sidebar.module.css";

// ---------------------------------------------------------------------------
// Nav item icons
// ---------------------------------------------------------------------------

const NAV_ICONS: Record<string, () => JSX.Element> = {
  "/": () => <TbOutlineLayoutDashboard size={16} />,
  "/agents": () => <TbOutlineRobot size={16} />,
  "/settings": () => <TbOutlineSettings size={16} />,
};

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

interface Props {
  collapsed: boolean;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const Sidebar: Component<Props> = (props) => {
  const location = useLocation();
  const [isDragging, setIsDragging] = createSignal(false);

  const isActive = (href: string) => {
    if (href === "/") return location.pathname === "/" || location.pathname === "/mission-control";
    return location.pathname.startsWith(href);
  };

  const navItems = [
    { href: "/", label: "Mission Control" },
  ];

  // ---- Drag-to-resize logic ------------------------------------------------
  // Update the DOM directly during drag to avoid the store → effect →
  // localStorage chain on every mousemove. Commit to the store on mouseup.

  let sidebarRef: HTMLElement | undefined;

  const startResize = (e: MouseEvent) => {
    e.preventDefault();
    setIsDragging(true);
    const startX = e.clientX;
    const startWidth = settingsState.sidebarWidths.navSidebar;

    const onMove = (moveEvent: MouseEvent) => {
      const delta = moveEvent.clientX - startX;
      const clamped = Math.max(NAV_SIDEBAR_MIN, Math.min(NAV_SIDEBAR_MAX, startWidth + delta));
      if (sidebarRef) sidebarRef.style.width = `${clamped}px`;
    };

    const onUp = (upEvent: MouseEvent) => {
      setIsDragging(false);
      const delta = upEvent.clientX - startX;
      setNavSidebarWidth(startWidth + delta);
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };

    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  const sidebarWidth = () =>
    props.collapsed ? 56 : settingsState.sidebarWidths.navSidebar;

  return (
    <aside
      ref={sidebarRef}
      class={styles.sidebar}
      classList={{ [styles.collapsed]: props.collapsed, [styles.dragging]: isDragging() }}
      style={{ width: `${sidebarWidth()}px` }}
    >
      {/* Traffic light area — pure drag region */}
      <div class={styles.trafficLightSpacer} />

      {/* Nav links */}
      <nav class={styles.nav}>
        {navItems.map((item) => {
          const count = () => item.href === "/" ? attentionCount() : 0;
          return (
            <A
              href={item.href}
              class={styles.navItem}
              classList={{ [styles.active]: isActive(item.href) }}
            >
              <span class={styles.navIcon}>{NAV_ICONS[item.href]?.()}</span>
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

      {/* Agent list — full list when expanded, icon-only nav when collapsed */}
      <Show
        when={!props.collapsed}
        fallback={
          <nav class={styles.nav}>
            <A
              href="/agents"
              class={styles.navItem}
              classList={{ [styles.active]: isActive("/agents") }}
            >
              <span class={styles.navIcon}>{NAV_ICONS["/agents"]?.()}</span>
            </A>
          </nav>
        }
      >
        <div class={styles.agentListWrapper}>
          <AgentList collapsed={false} />
        </div>
      </Show>

      {/* Settings — pinned to bottom */}
      <div class={styles.bottomNav}>
        <A
          href="/settings"
          class={styles.navItem}
          classList={{ [styles.active]: isActive("/settings") }}
        >
          <span class={styles.navIcon}>{NAV_ICONS["/settings"]?.()}</span>
          <Show when={!props.collapsed}>
            <span class={styles.navLabel}>Settings</span>
          </Show>
        </A>
      </div>

      {/* Drag handle */}
      <Show when={!props.collapsed}>
        <div
          class={styles.resizeHandle}
          classList={{ [styles.dragging]: isDragging() }}
          onMouseDown={startResize}
          title="Drag to resize sidebar"
          aria-hidden="true"
        />
      </Show>
    </aside>
  );
};

export default Sidebar;
