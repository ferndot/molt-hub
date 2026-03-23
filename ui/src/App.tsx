import type { Component } from "solid-js";
import { onMount, onCleanup } from "solid-js";
import { Router, Route, A } from "@solidjs/router";
import ConnectionStatusBadge from "./components/ConnectionStatus";
import { useWebSocket, connect, disconnect } from "./lib/ws";

// ---------------------------------------------------------------------------
// Placeholder route views
// ---------------------------------------------------------------------------

const TriagePage: Component = () => (
  <div style={{ padding: "2rem" }}>
    <h2 style={{ "margin-bottom": "1rem", "font-size": "1.25rem" }}>
      Triage Queue
    </h2>
    <p style={{ color: "#6b7280" }}>
      Decision queue — T23 will implement this view.
    </p>
  </div>
);

const BoardPage: Component = () => (
  <div style={{ padding: "2rem" }}>
    <h2 style={{ "margin-bottom": "1rem", "font-size": "1.25rem" }}>
      Kanban Board
    </h2>
    <p style={{ color: "#6b7280" }}>
      Passive dashboard — T24 will implement this view.
    </p>
  </div>
);

const AgentsPage: Component = () => (
  <div style={{ padding: "2rem" }}>
    <h2 style={{ "margin-bottom": "1rem", "font-size": "1.25rem" }}>Agents</h2>
    <p style={{ color: "#6b7280" }}>
      Agent detail — T28 will implement this view.
    </p>
  </div>
);

// ---------------------------------------------------------------------------
// App shell
// ---------------------------------------------------------------------------

const NAV_LINKS = [
  { href: "/triage", label: "Triage" },
  { href: "/board", label: "Board" },
  { href: "/agents", label: "Agents" },
];

const AppShell: Component = () => {
  const ws = useWebSocket();

  onMount(() => connect("/ws"));
  onCleanup(() => disconnect());

  return (
    <div
      style={{
        display: "flex",
        "flex-direction": "column",
        "min-height": "100vh",
      }}
    >
      {/* Header */}
      <header
        style={{
          display: "flex",
          "align-items": "center",
          "justify-content": "space-between",
          padding: "0 1.5rem",
          height: "48px",
          background: "#1a1a22",
          "border-bottom": "1px solid #2a2a36",
          position: "sticky",
          top: 0,
          "z-index": 100,
        }}
      >
        <div style={{ display: "flex", "align-items": "center", gap: "2rem" }}>
          <span
            style={{
              "font-size": "1rem",
              "font-weight": "600",
              "letter-spacing": "-0.01em",
            }}
          >
            Molt Hub
          </span>
          <nav style={{ display: "flex", gap: "1.25rem" }}>
            {NAV_LINKS.map((link) => (
              <A
                href={link.href}
                style={{
                  "font-size": "0.875rem",
                  color: "#9ca3af",
                  "text-decoration": "none",
                }}
                activeClass="nav-active"
              >
                {link.label}
              </A>
            ))}
          </nav>
        </div>
        <ConnectionStatusBadge status={ws.status()} />
      </header>

      {/* Main content */}
      <main style={{ flex: 1 }}>
        <Route path="/triage" component={TriagePage} />
        <Route path="/board" component={BoardPage} />
        <Route path="/agents" component={AgentsPage} />
        <Route path="/" component={TriagePage} />
      </main>
    </div>
  );
};

const App: Component = () => {
  return (
    <Router>
      <Route path="/*" component={AppShell} />
    </Router>
  );
};

export default App;
