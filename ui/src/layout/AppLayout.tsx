import type { ParentComponent } from "solid-js";
import { createSignal, onMount, onCleanup } from "solid-js";
import { useWebSocket, connect, disconnect } from "../lib/ws";
import { useMissionControl } from "../views/MissionControl/missionControlStore";
import { unreadCount } from "./attentionStore";
import Sidebar from "./Sidebar";
import TopBar from "./TopBar";
import InboxSidebar from "../views/MissionControl/InboxSidebar";
import StatusBar from "./StatusBar";
import KeyboardManager from "../keyboard/KeyboardManager";
import styles from "./AppLayout.module.css";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const AppLayout: ParentComponent = (props) => {
  const ws = useWebSocket();
  const [sidebarCollapsed, setSidebarCollapsed] = createSignal(false);
  const [inboxOpen, setInboxOpen] = createSignal(false);
  const mc = useMissionControl();

  onMount(() => connect("/ws"));
  onCleanup(() => disconnect());

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
              {props.children}
            </main>
          </div>
          <InboxSidebar collapsed={!inboxOpen()} />
        </div>

        {/* Bottom status bar */}
        <StatusBar status={ws.status()} />
      </div>
    </KeyboardManager>
  );
};

export default AppLayout;
