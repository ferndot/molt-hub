import type { Component } from "solid-js";
import { createEffect, onCleanup, onMount } from "solid-js";
import { Router, Route, Navigate } from "@solidjs/router";
import { WORKSPACE_ID } from "./lib/workspace";
import { initBoardStages, handleBoardWsMessage } from "./views/Board/boardStore";
import { subscribe, projectTopic } from "./lib/ws";
import AppLayout from "./layout/AppLayout";
import TriageView from "./views/Triage/TriageView";
import AgentDetailView from "./views/AgentDetail/AgentDetailView";
import TaskDetailView from "./views/TaskDetail/TaskDetailView";
import AgentsView from "./views/Agents/AgentsView";
import BoardsView from "./views/Boards/BoardsView";
import BoardView from "./views/Board/BoardView";
import SettingsView from "./views/Settings/SettingsView";

// ---------------------------------------------------------------------------
// Route views
// ---------------------------------------------------------------------------

const TriagePage: Component = () => <TriageView />;

const WorkboardPage: Component = () => <BoardView />;

const RedirectToBoard: Component = () => <Navigate href="/board" />;

const AgentsPage: Component = () => <AgentsView />;

const BoardsPage: Component = () => <BoardsView />;

const SettingsPage: Component = () => <SettingsView />;

// ---------------------------------------------------------------------------
// App with persistent layout shell
// ---------------------------------------------------------------------------

const App: Component = () => {
  onMount(() => {
    void initBoardStages();
  });

  createEffect(() => {
    const topic = projectTopic(WORKSPACE_ID, "board:*");
    const unsub = subscribe(topic, handleBoardWsMessage);
    onCleanup(unsub);
  });

  return (
    <Router root={AppLayout}>
      <Route path="/" component={RedirectToBoard} />
      <Route path="/mission-control" component={RedirectToBoard} />
      <Route path="/triage" component={TriagePage} />
      <Route path="/board" component={WorkboardPage} />
      <Route path="/boards" component={BoardsPage} />
      <Route path="/agents" component={AgentsPage} />
      <Route path="/agents/:id" component={AgentDetailView} />
      <Route path="/tasks/:id" component={TaskDetailView} />
      <Route path="/settings" component={SettingsPage} />
    </Router>
  );
};

export default App;
