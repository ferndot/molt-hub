import type { Component } from "solid-js";
import { lazy, Suspense, onMount } from "solid-js";
import { Router, Route } from "@solidjs/router";
import { loadProjects } from "./stores/projectStore";
import AppLayout from "./layout/AppLayout";
import TriageView from "./views/Triage/TriageView";
import AgentDetailView from "./views/AgentDetail/AgentDetailView";
import TaskDetailView from "./views/TaskDetail/TaskDetailView";
import AgentsView from "./views/Agents/AgentsView";
import BoardView from "./views/Board/BoardView";
import SettingsView from "./views/Settings/SettingsView";

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

const SettingsPage: Component = () => <SettingsView />;

// ---------------------------------------------------------------------------
// App with persistent layout shell
// ---------------------------------------------------------------------------

const App: Component = () => {
  onMount(() => {
    void loadProjects();
  });

  return (
    <Router root={AppLayout}>
      <Route path="/" component={MissionControlPage} />
      <Route path="/mission-control" component={MissionControlPage} />
      <Route path="/triage" component={TriagePage} />
      <Route path="/board" component={BoardPage} />
      <Route path="/agents" component={AgentsPage} />
      <Route path="/agents/:id" component={AgentDetailView} />
      <Route path="/tasks/:id" component={TaskDetailView} />
      <Route path="/settings" component={SettingsPage} />
      {/* Project-scoped routes */}
      <Route path="/projects/:pid/board" component={BoardPage} />
      <Route path="/projects/:pid/triage" component={TriagePage} />
      <Route path="/projects/:pid/settings" component={SettingsPage} />
    </Router>
  );
};

export default App;
