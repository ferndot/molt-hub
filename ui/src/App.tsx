import type { Component } from "solid-js";
import { Router, Route } from "@solidjs/router";
import AppLayout from "./layout/AppLayout";
import TriageView from "./views/Triage/TriageView";
import AgentDetailView from "./views/AgentDetail/AgentDetailView";
import BoardView from "./views/Board/BoardView";

// ---------------------------------------------------------------------------
// Route views
// ---------------------------------------------------------------------------

const TriagePage: Component = () => <TriageView />;

const BoardPage: Component = () => <BoardView />;

const AgentsPage: Component = () => (
  <div style={{ padding: "2rem" }}>
    <h2 style={{ "margin-bottom": "1rem", "font-size": "1.25rem" }}>Agents</h2>
    <p style={{ color: "#6b7280" }}>
      Agent detail list view.
    </p>
  </div>
);

// ---------------------------------------------------------------------------
// App with persistent layout shell
// ---------------------------------------------------------------------------

const App: Component = () => {
  return (
    <Router root={AppLayout}>
      <Route path="/triage" component={TriagePage} />
      <Route path="/board" component={BoardPage} />
      <Route path="/agents" component={AgentsPage} />
      <Route path="/agents/:id" component={AgentDetailView} />
      <Route path="/" component={TriagePage} />
    </Router>
  );
};

export default App;
