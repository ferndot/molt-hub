import type { Component } from "solid-js";
import { lazy, Suspense } from "solid-js";
import { Router, Route } from "@solidjs/router";
import AppLayout from "./layout/AppLayout";
import TriageView from "./views/Triage/TriageView";
import AgentDetailView from "./views/AgentDetail/AgentDetailView";
import AgentsView from "./views/Agents/AgentsView";
import BoardView from "./views/Board/BoardView";
import Settings from "./views/Settings/Settings";

const MissionControlView = lazy(() => import("./views/MissionControl/MissionControlView"));

// ---------------------------------------------------------------------------
// Route views
// ---------------------------------------------------------------------------

const TriagePage: Component = () => <TriageView />;

const BoardPage: Component = () => <BoardView />;

const MissionControlPage: Component = () => (
  <Suspense fallback={<div style={{ padding: "2rem", color: "var(--text-muted)" }}>Loading...</div>}>
    <MissionControlView />
  </Suspense>
);

const AgentsPage: Component = () => <AgentsView />;

const SettingsPage: Component = () => <Settings />;

// ---------------------------------------------------------------------------
// App with persistent layout shell
// ---------------------------------------------------------------------------

const App: Component = () => {
  return (
    <Router root={AppLayout}>
      <Route path="/" component={MissionControlPage} />
      <Route path="/mission-control" component={MissionControlPage} />
      <Route path="/triage" component={TriagePage} />
      <Route path="/board" component={BoardPage} />
      <Route path="/agents" component={AgentsPage} />
      <Route path="/agents/:id" component={AgentDetailView} />
      <Route path="/settings" component={SettingsPage} />
    </Router>
  );
};

export default App;
