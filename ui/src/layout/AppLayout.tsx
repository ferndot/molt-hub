import type { ParentComponent } from "solid-js";
import { createSignal, onMount, onCleanup } from "solid-js";
import { useWebSocket, connect, disconnect } from "../lib/ws";
import { useMissionControl } from "../views/MissionControl/missionControlStore";
import { connectNotificationsWs, initNotificationsFromTriage } from "../views/MissionControl/notificationStore";
import { unreadCount, setP0Count, setP1Count } from "./attentionStore";
import { initAgents } from "./agentListUtils";
import Sidebar from "./Sidebar";
import TopBar from "./TopBar";
import InboxSidebar from "../views/MissionControl/InboxSidebar";
import StatusBar from "./StatusBar";
import KeyboardManager from "../keyboard/KeyboardManager";
import styles from "./AppLayout.module.css";
import HookActivityToast from "../components/HookActivityToast/HookActivityToast";
import { initMetrics } from "../stores/metricsStore";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function syncAttentionCounts(): Promise<void> {
  try {
    const res = await fetch("/api/tasks/triage");
    if (!res.ok) return;
    const data = await res.json() as { items: Array<{ priority: string }> };
    if (!Array.isArray(data.items)) return;
    setP0Count(data.items.filter(i => i.priority === "p0").length);
    setP1Count(data.items.filter(i => i.priority === "p1").length);
  } catch { /* ignore */ }
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const AppLayout: ParentComponent = (props) => {
  const ws = useWebSocket();
  const [sidebarCollapsed, setSidebarCollapsed] = createSignal(false);
  const [inboxOpen, setInboxOpen] = createSignal(false);
  const mc = useMissionControl();

  onMount(() => {
    connect("/ws");
    void initAgents();
    void initMetrics();
    void initNotificationsFromTriage();
    void syncAttentionCounts();
  });

  // Subscribe to WS notifications topic; clean up on unmount
  const unsubNotifications = connectNotificationsWs();
  onCleanup(() => {
    unsubNotifications();
    disconnect();
  });

  return (
    <KeyboardManager>
      <div class={styles.shell}>
        {/* Body: left sidebar + main + inbox sidebar */}
        <div class={styles.body}>
          <Sidebar collapsed={sidebarCollapsed()} />
          <div class={styles.mainColumn}>
            <TopBar
              sidebarCollapsed={sidebarCollapsed()}
              onToggleSidebar={() => setSidebarCollapsed((v) => !v)}
              inboxOpen={inboxOpen()}
              onToggleInbox={() => setInboxOpen((v) => !v)}
              inboxCount={unreadCount()}
            />
            <main class={styles.main}>
              <div class={styles.pageEnter}>
                {props.children}
              </div>
            </main>
          </div>
          <InboxSidebar collapsed={!inboxOpen()} />
        </div>

        {/* Bottom status bar */}
        <StatusBar status={ws.status()} />
      </div>
      <HookActivityToast />
    </KeyboardManager>
  );
};

export default AppLayout;
