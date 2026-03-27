import type { Component } from "solid-js";
import { createEffect, onCleanup, onMount } from "solid-js";
import { Router, Route, Navigate } from "@solidjs/router";
import { WORKSPACE_ID } from "./lib/workspace";
import {
  handleBoardWsMessage,
  homeRedirectBoardPath,
  initBoardStages,
  boardState,
} from "./views/Board/boardStore";
import { subscribe, projectTopic } from "./lib/ws";
import { emitHookToast } from "./lib/hookToasts";
import AppLayout from "./layout/AppLayout";
import TriageView from "./views/Triage/TriageView";
import AgentDetailView from "./views/AgentDetail/AgentDetailView";
import TaskDetailView from "./views/TaskDetail/TaskDetailView";
import AgentsView from "./views/Agents/AgentsView";
import BoardsView from "./views/Boards/BoardsView";
import BoardPage from "./views/Board/BoardPage";
import SettingsView from "./views/Settings/SettingsView";
import {
  fetchGithubStatus,
  fetchJiraStatus,
} from "./views/Settings/settingsStore";
import { initAgents, startAgentRefresh, stopAgentRefresh } from "./layout/AgentList";
import HowItWorksView from "./views/HowItWorks/HowItWorksView";
import AiTutorView from "./views/AiTutor/AiTutorView";

// ---------------------------------------------------------------------------
// Route views
// ---------------------------------------------------------------------------

const TriagePage: Component = () => <TriageView />;

const RedirectHome: Component = () => (
  <Navigate href={homeRedirectBoardPath()} />
);

const AgentsPage: Component = () => <AgentsView />;

const BoardsPage: Component = () => <BoardsView />;

const SettingsPage: Component = () => <SettingsView />;

// ---------------------------------------------------------------------------
// App with persistent layout shell
// ---------------------------------------------------------------------------

const App: Component = () => {
  onMount(() => {
    void initBoardStages();
    // Reconcile integration flags with the server (tokens live in the OS keychain, not localStorage).
    void fetchJiraStatus();
    void fetchGithubStatus();
    // Bootstrap sidebar agent list and start polling.
    void initAgents();
    startAgentRefresh();
  });

  onCleanup(() => {
    stopAgentRefresh();
  });

  createEffect(() => {
    const topic = projectTopic(WORKSPACE_ID, "board:update");
    const unsub = subscribe(topic, handleBoardWsMessage);
    onCleanup(unsub);
  });

  createEffect(() => {
    const hooksTopic = projectTopic(WORKSPACE_ID, "hooks");
    const unsub = subscribe(hooksTopic, (msg) => {
      if (msg.type !== "event") return;
      const payload = msg.payload as Record<string, unknown>;
      if (payload.type !== "hook_fired") return;
      const taskId = payload.task_id as string | undefined;
      const stage = payload.stage as string | undefined;
      const trigger = payload.trigger as string | undefined;
      if (!stage || !trigger) return;
      // Map backend trigger to toast event label
      const event =
        trigger === "enter"
          ? "on_enter"
          : trigger === "exit"
            ? "on_exit"
            : "on_stall";
      // Resolve task name from board state
      const task = boardState.tasks.find((t) => t.id === taskId);
      const taskName = task?.name ?? taskId ?? "Unknown task";
      emitHookToast(stage, event as "on_enter" | "on_exit" | "on_stall", taskName);
    });
    onCleanup(unsub);
  });

  return (
    <Router root={AppLayout}>
      <Route path="/" component={RedirectHome} />
      <Route path="/board" component={RedirectHome} />
      <Route path="/triage" component={TriagePage} />
      <Route path="/boards/:id" component={BoardPage} />
      <Route path="/boards" component={BoardsPage} />
      <Route path="/agents" component={AgentsPage} />
      <Route path="/agents/:id" component={AgentDetailView} />
      <Route path="/tasks/:id" component={TaskDetailView} />
      <Route path="/settings" component={SettingsPage} />
      <Route path="/how-it-works" component={HowItWorksView} />
      <Route path="/tutor" component={AiTutorView} />
    </Router>
  );
};

export default App;
