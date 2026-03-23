import type { Component, ParentComponent } from "solid-js";
import { createSignal, onMount, onCleanup } from "solid-js";
import { useWebSocket, connect, disconnect } from "../lib/ws";
import ConnectionStatusBadge from "../components/ConnectionStatus";
import Sidebar from "./Sidebar";
import KeyboardManager from "../keyboard/KeyboardManager";
import styles from "./AppLayout.module.css";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const AppLayout: ParentComponent = (props) => {
  const ws = useWebSocket();
  const [collapsed, setCollapsed] = createSignal(false);

  onMount(() => connect("/ws"));
  onCleanup(() => disconnect());

  return (
    <KeyboardManager>
      <div class={styles.shell}>
        {/* Top bar */}
        <header class={styles.topBar}>
          <span class={styles.appTitle}>Molt Hub</span>
          <ConnectionStatusBadge status={ws.status()} />
        </header>

        {/* Body: sidebar + main */}
        <div class={styles.body}>
          <Sidebar
            collapsed={collapsed()}
            onToggle={() => setCollapsed((v) => !v)}
          />
          <main class={styles.main}>
            {props.children}
          </main>
        </div>
      </div>
    </KeyboardManager>
  );
};

export default AppLayout;
