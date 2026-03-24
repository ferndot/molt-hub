import type { Component } from "solid-js";
import { createEffect, onCleanup, onMount } from "solid-js";
import { Router, Route } from "@solidjs/router";
import { loadProjects, projectState } from "./stores/projectStore";
import { initBoardStages, handleBoardWsMessage } from "./views/Board/boardStore";
import { subscribe, projectTopic } from "./lib/ws";
import AppLayout from "./layout/AppLayout";
import TriageView from "./views/Triage/TriageView";
import AgentDetailView from "./views/AgentDetail/AgentDetailView";
import TaskDetailView from "./views/TaskDetail/TaskDetailView";
import AgentsView from "./views/Agents/AgentsView";
import BoardView from "./views/Board/BoardView";
import SettingsView from "./views/Settings/SettingsView";

// ---------------------------------------------------------------------------
// Route views
// ---------------------------------------------------------------------------

const TriagePage: Component = () => <TriageView />;

const WorkboardPage: Component = () => <BoardView />;

const AgentsPage: Component = () => <AgentsView />;

const SettingsPage: Component = () => <SettingsView />;

// ---------------------------------------------------------------------------
// App with persistent layout shell
// ---------------------------------------------------------------------------

const App: Component = () => {
  onMount(() => {
    void loadProjects();
  });

  createEffect(() => {
    projectState.activeProjectId;
    void initBoardStages();
  });

  createEffect(() => {
    const topic = projectTopic(projectState.activeProjectId, "board:*");
    const unsub = subscribe(topic, handleBoardWsMessage);
    onCleanup(unsub);
  });

  return (
    <Router root={AppLayout}>
      <Route path="/" component={WorkboardPage} />
      <Route path="/mission-control" component={WorkboardPage} />
      <Route path="/triage" component={TriagePage} />
      <Route path="/board" component={WorkboardPage} />
      <Route path="/agents" component={AgentsPage} />
      <Route path="/agents/:id" component={AgentDetailView} />
      <Route path="/tasks/:id" component={TaskDetailView} />
      <Route path="/settings" component={SettingsPage} />
      {/* Project-scoped routes */}
      <Route path="/projects/:pid/board" component={WorkboardPage} />
      <Route path="/projects/:pid/triage" component={TriagePage} />
      <Route path="/projects/:pid/settings" component={SettingsPage} />
    </Router>
  );
};

export default App;
