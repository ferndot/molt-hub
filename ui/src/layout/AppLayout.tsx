import type { ParentComponent } from "solid-js";
import { createSignal, onMount, onCleanup } from "solid-js";
import { useWebSocket, connect, disconnect } from "../lib/ws";
import Sidebar from "./Sidebar";
import StatusBar from "./StatusBar";
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

        {/* Bottom status bar */}
        <StatusBar status={ws.status()} />
      </div>
    </KeyboardManager>
  );
};

export default AppLayout;
