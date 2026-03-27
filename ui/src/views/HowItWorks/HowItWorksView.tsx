import type { Component } from "solid-js";
import { createSignal, For, Show } from "solid-js";
import styles from "./HowItWorksView.module.css";

// ---------------------------------------------------------------------------
// Pipeline stages
// ---------------------------------------------------------------------------

interface Stage {
  name: string;
  icon: string;
  color: string;
  count: number;
}

const STAGES: Stage[] = [
  { name: "Backlog",     icon: "📋", color: "var(--stage-grey)",   count: 3 },
  { name: "Planning",   icon: "🗺️", color: "var(--stage-purple)", count: 2 },
  { name: "In Progress",icon: "⚡", color: "var(--stage-blue)",   count: 3 },
  { name: "Review",     icon: "🔍", color: "var(--stage-amber)",  count: 1 },
  { name: "Testing",    icon: "🧪", color: "var(--stage-emerald)", count: 2 },
  { name: "Deployment", icon: "🚀", color: "var(--stage-green)",  count: 1 },
];

// ---------------------------------------------------------------------------
// Stage detail panels
// ---------------------------------------------------------------------------

interface StageDetail {
  title: string;
  steps: { icon: string; label: string }[];
}

const STAGE_DETAILS: StageDetail[] = [
  {
    title: "Tasks Waiting to Be Picked Up",
    steps: [
      { icon: "📋", label: "Task created via UI, GitHub, or Jira sync" },
      { icon: "🔗", label: "Linked to a repo, project, or epic" },
      { icon: "⏳", label: "Waits until an agent team has capacity" },
      { icon: "🚦", label: "WIP limits prevent queue overflows" },
    ],
  },
  {
    title: "Agent Planning & Human Sign-off",
    steps: [
      { icon: "🗺️", label: "Agents run automatic prioritization" },
      { icon: "🤖", label: "Agent team holds a planning meeting" },
      { icon: "📋", label: "Plan surfaced to human for review" },
      { icon: "✅", label: "Human approves key details before work begins" },
    ],
  },
  {
    title: "Implementation by Agent Teams",
    steps: [
      { icon: "🤖", label: "Agent teams implement and test the work" },
      { icon: "💬", label: "Open questions surface as Inbox notifications" },
      { icon: "🎯", label: "Human can steer implementation at any time" },
      { icon: "✅", label: "Agent completes  OR  🚫 Blocked" },
    ],
  },
  {
    title: "Human & Agent Code Review",
    steps: [
      { icon: "🔍", label: "Human reviews key code areas and decisions" },
      { icon: "⚠️", label: "Risks are surfaced and highlighted" },
      { icon: "🤖", label: "Agents review for code quality and bugs" },
      { icon: "✅", label: "Approve — task moves to Testing" },
      { icon: "↩️", label: "Reject — task sent back with feedback" },
    ],
  },
  {
    title: "Automated Testing & Preview",
    steps: [
      { icon: "🧪", label: "Automated agent testing runs against changes" },
      { icon: "🌐", label: "Preview environment available for human review" },
      { icon: "🚫", label: "Failures block — task returns to In Progress" },
      { icon: "✅", label: "Pass — ready to deploy" },
    ],
  },
  {
    title: "Shipped 🎉",
    steps: [
      { icon: "🚀", label: "PR opened and merged to production" },
      { icon: "📜", label: "Full audit trail preserved in event log" },
      { icon: "📊", label: "Metrics updated: cycle time, throughput" },
      { icon: "🏆", label: "Agent team credited in task history" },
    ],
  },
];

// ---------------------------------------------------------------------------
// Feature cards
// ---------------------------------------------------------------------------

interface FeatureCard {
  icon: string;
  title: string;
  description: string;
}

const FEATURE_CARDS: FeatureCard[] = [
  {
    icon: "🎯",
    title: "Triage Inbox",
    description: "P0–P3 priority queue. Critical issues surface instantly.",
  },
  {
    icon: "🤖",
    title: "Agent Supervision",
    description:
      "Real-time output streaming. See exactly what each agent is doing.",
  },
  {
    icon: "✅",
    title: "Approval Workflows",
    description:
      "Human-in-the-loop decisions before agents ship code.",
  },
  {
    icon: "📊",
    title: "Multi-Project",
    description:
      "Manage dozens of agents across multiple repos from one dashboard.",
  },
];

// ---------------------------------------------------------------------------
// Tech highlight cards
// ---------------------------------------------------------------------------

interface TechCard {
  icon: string;
  title: string;
  description: string;
}

const TECH_CARDS: TechCard[] = [
  {
    icon: "🗄️",
    title: "Event Sourcing",
    description: "Every action is an immutable event in SQLite. Full replay, time-travel debugging.",
  },
  {
    icon: "⚡",
    title: "Reactive by Default",
    description: "SolidJS fine-grained reactivity + WebSocket push = zero-poll, instant updates.",
  },
  {
    icon: "🔌",
    title: "Pluggable Agents",
    description: "Swap Claude for any LLM or custom CLI. The harness abstracts the runner.",
  },
];

// ---------------------------------------------------------------------------
// SVG Arrow between stages
// ---------------------------------------------------------------------------

const PipelineArrow: Component<{ index: number }> = (props) => (
  <svg
    class={styles.arrow}
    style={{ "animation-delay": `${0.1 * props.index + 0.5}s` }}
    width="40"
    height="20"
    viewBox="0 0 40 20"
    fill="none"
    aria-hidden="true"
  >
    <line
      x1="0"
      y1="10"
      x2="32"
      y2="10"
      stroke="currentColor"
      stroke-width="2"
      stroke-dasharray="4 3"
      class={styles.arrowLine}
      style={{ "animation-delay": `${0.1 * props.index + 0.5}s` }}
    />
    <polygon points="32,5 40,10 32,15" fill="currentColor" class={styles.arrowHead} />
  </svg>
);

// ---------------------------------------------------------------------------
// Tech box component
// ---------------------------------------------------------------------------

interface TechBoxProps {
  icon: string;
  title: string;
  sub?: string;
  delay?: number;
}

const TechBox: Component<TechBoxProps> = (props) => (
  <div
    class={styles.techBox}
    style={{ "animation-delay": `${props.delay ?? 0}s` }}
  >
    <span class={styles.techBoxIcon}>{props.icon}</span>
    <p class={styles.techBoxTitle}>{props.title}</p>
    {props.sub && <p class={styles.techBoxSub}>{props.sub}</p>}
  </div>
);

// ---------------------------------------------------------------------------
// Main view
// ---------------------------------------------------------------------------

const HowItWorksView: Component = () => {
  const [active, setActive] = createSignal<number | null>(2);
  const [tab, setTab] = createSignal<"pipeline" | "technology" | "customization" | "future">("pipeline");

  return (
    <div class={styles.container}>
      <header class={styles.header}>
        <h1 class={styles.title}>How Molt Hub Works</h1>
        <p class={styles.subtitle}>
          Mission Control for AI coding agents — supervise dozens of concurrent
          agents across every stage of your workflow.
        </p>
      </header>

      {/* ------------------------------------------------------------------ */}
      {/* Tab bar                                                              */}
      {/* ------------------------------------------------------------------ */}
      <div class={styles.tabBar}>
        <button
          class={`${styles.tab} ${tab() === "pipeline" ? styles.tabActive : ""}`}
          onClick={() => setTab("pipeline")}
        >
          Workflow
        </button>
        <button
          class={`${styles.tab} ${tab() === "technology" ? styles.tabActive : ""}`}
          onClick={() => setTab("technology")}
        >
          Technology
        </button>
        <button
          class={`${styles.tab} ${tab() === "customization" ? styles.tabActive : ""}`}
          onClick={() => setTab("customization")}
        >
          Customization
        </button>
        <button
          class={`${styles.tab} ${tab() === "future" ? styles.tabActive : ""}`}
          onClick={() => setTab("future")}
        >
          Future
        </button>
      </div>

      {/* ------------------------------------------------------------------ */}
      {/* Pipeline tab                                                         */}
      {/* ------------------------------------------------------------------ */}
      <Show when={tab() === "pipeline"}>
        {/* Section 1: Pipeline infographic */}
        <section class={styles.section}>
          <h2 class={styles.sectionTitle}>The Task Pipeline</h2>

          <div class={styles.pipeline}>
            <For each={STAGES}>
              {(stage, i) => (
                <>
                  <div
                    class={`${styles.stageNode} ${active() === i() ? styles.stageNodeActive : ""}`}
                    style={{
                      "--stage-color": stage.color,
                      "animation-delay": `${i() * 0.1}s`,
                    }}
                    onClick={() => setActive(active() === i() ? null : i())}
                    role="button"
                    aria-expanded={active() === i()}
                  >
                    <span class={styles.stageIcon}>{stage.icon}</span>
                    <span class={styles.stageName}>{stage.name}</span>
                    <span class={styles.stageBadge}>{stage.count}</span>
                  </div>
                  {i() < STAGES.length - 1 && (
                    <PipelineArrow index={i()} />
                  )}
                </>
              )}
            </For>
          </div>

          {/* Per-stage detail panel */}
          <Show when={active() !== null}>
            <For each={[active()]}>
              {(idx) => {
                const detail = STAGE_DETAILS[idx as number];
                const stage = STAGES[idx as number];
                return (
                  <div
                    class={styles.stageDetailPanel}
                    style={{ "--panel-color": stage.color }}
                  >
                    <h3 class={styles.panelTitle}>{detail.title}</h3>
                    <div class={styles.agentLoop}>
                      <For each={detail.steps}>
                        {(step, si) => (
                          <>
                            <div
                              class={styles.loopStep}
                              style={{ "animation-delay": `${si() * 0.1}s` }}
                            >
                              <span class={styles.loopIcon}>{step.icon}</span>
                              <span class={styles.loopLabel}>{step.label}</span>
                            </div>
                            {si() < detail.steps.length - 1 && (
                              <div
                                class={styles.loopConnector}
                                style={{ "animation-delay": `${0.05 + si() * 0.1}s` }}
                              >
                                ↓
                              </div>
                            )}
                          </>
                        )}
                      </For>
                    </div>
                  </div>
                );
              }}
            </For>
          </Show>
        </section>

        {/* Section 2: Feature cards */}
        <section class={styles.section}>
          <h2 class={styles.sectionTitle}>Key Features</h2>
          <div class={styles.featureGrid}>
            <For each={FEATURE_CARDS}>
              {(card, i) => (
                <div
                  class={styles.featureCard}
                  style={{ "animation-delay": `${i() * 0.15}s` }}
                >
                  <span class={styles.featureIcon}>{card.icon}</span>
                  <h3 class={styles.featureTitle}>{card.title}</h3>
                  <p class={styles.featureDescription}>{card.description}</p>
                </div>
              )}
            </For>
          </div>
        </section>
      </Show>

      {/* ------------------------------------------------------------------ */}
      {/* Technology tab                                                       */}
      {/* ------------------------------------------------------------------ */}
      <Show when={tab() === "technology"}>
        <section class={styles.techSection}>
          <header>
            <h2 class={styles.title} style={{ "font-size": "1.5rem" }}>Technology Stack</h2>
            <p class={styles.subtitle}>
              An event-sourced Rust backend, reactive SolidJS frontend, and Claude agents — connected end-to-end.
            </p>
          </header>

          {/* Flowchart diagram */}
          <div class={styles.techDiagram}>
            {/* Left column — User & UI layer */}
            <div class={styles.techColumn}>
              <TechBox icon="👤" title="Human" delay={0.05} />
              <TechBox icon="🖥️" title="SolidJS UI" sub="board, triage, chat" delay={0.1} />
              <TechBox icon="🔌" title="WebSocket" sub="real-time updates" delay={0.15} />
            </div>

            {/* Connector: UI ↔ Backend */}
            <div class={styles.techConnector}>
              <div class={styles.techConnectorLabel}>REST /api</div>
              <div class={styles.techConnectorLine} />
              <span class={styles.techArrow}>⇄</span>
              <div class={styles.techConnectorLine} />
              <div class={styles.techConnectorLabel}>WS board:*</div>
            </div>

            {/* Center column — Backend */}
            <div class={styles.techColumn}>
              <TechBox icon="⚙️" title="Axum API Server" sub="Rust" delay={0.1} />
              <TechBox icon="📚" title="SQLite Event Store" sub="append-only event log" delay={0.15} />
              <TechBox icon="🔀" title="Pipeline Engine" sub="hooks, state machine, WIP limits" delay={0.2} />
            </div>

            {/* Connector: Backend ↔ Agents */}
            <div class={styles.techConnector}>
              <div class={styles.techConnectorLabel}>spawn / steer</div>
              <div class={styles.techConnectorLine} />
              <span class={styles.techArrow}>⇄</span>
              <div class={styles.techConnectorLine} />
              <div class={styles.techConnectorLabel}>output / complete</div>
            </div>

            {/* Right column — Agents & Integrations */}
            <div class={styles.techColumn}>
              <TechBox icon="🤖" title="Agent Harness" delay={0.15} />
              <TechBox icon="🧠" title="Claude CLI" delay={0.2} />
              <TechBox icon="✨" title="Anthropic API" delay={0.25} />
              <TechBox icon="🔗" title="Integrations" sub="GitHub · Jira · Webhooks" delay={0.3} />
            </div>
          </div>

          {/* Tech highlight cards */}
          <div>
            <h2 class={styles.sectionTitle}>Tech Highlights</h2>
            <div class={styles.featureGrid} style={{ "margin-top": "16px" }}>
              <For each={TECH_CARDS}>
                {(card, i) => (
                  <div
                    class={styles.featureCard}
                    style={{ "animation-delay": `${i() * 0.15}s` }}
                  >
                    <span class={styles.featureIcon}>{card.icon}</span>
                    <h3 class={styles.featureTitle}>{card.title}</h3>
                    <p class={styles.featureDescription}>{card.description}</p>
                  </div>
                )}
              </For>
            </div>
          </div>
        </section>
      </Show>

      {/* ------------------------------------------------------------------ */}
      {/* Customization tab                                                   */}
      {/* ------------------------------------------------------------------ */}
      <Show when={tab() === "customization"}>
        <section class={styles.techSection}>

          {/* ── Multiple Boards ─────────────────────────────────── */}
          <div class={styles.vizBlock}>
            <div class={styles.vizLabel}>📋 Multiple Boards</div>
            <div class={styles.boardsDemo}>
              {[
                { name: "Team Alpha", cols: ["Backlog","Planning","In Progress","Review"], color: "#6366f1" },
                { name: "Release Train", cols: ["Backlog","Testing","Deployment"], color: "#22c55e" },
                { name: "Security", cols: ["Triage","In Progress","Sign-off"], color: "#f59e0b" },
              ].map((b, bi) => (
                <div class={styles.boardMini} style={{ "--bcolor": b.color, "animation-delay": `${bi * 0.1}s` }}>
                  <div class={styles.boardMiniTitle}>{b.name}</div>
                  <div class={styles.boardMiniCols}>
                    {b.cols.map(c => <span class={styles.boardMiniCol}>{c}</span>)}
                  </div>
                </div>
              ))}
            </div>
            <ul class={styles.vizBullets}>
              <li>One board per team, project, or release</li>
              <li>Shared event store — tasks flow freely</li>
              <li>Switch instantly from the sidebar</li>
            </ul>
          </div>

          {/* ── Custom Columns ──────────────────────────────────── */}
          <div class={styles.vizBlock}>
            <div class={styles.vizLabel}>🗂️ Custom Columns</div>
            <div class={styles.colSettingsDemo}>
              <div class={styles.colCard}>
                <div class={styles.colCardName}>Review</div>
                <div class={styles.colBadges}>
                  <span class={styles.colBadge} style={{ "--bc": "#f59e0b" }}>✅ Approval required</span>
                  <span class={styles.colBadge} style={{ "--bc": "#6366f1" }}>⏱ 48h timeout</span>
                  <span class={styles.colBadge} style={{ "--bc": "#94a3b8" }}>WIP 3</span>
                </div>
              </div>
              <div class={styles.colCard}>
                <div class={styles.colCardName}>Testing</div>
                <div class={styles.colBadges}>
                  <span class={styles.colBadge} style={{ "--bc": "#10b981" }}>🧪 Auto-run</span>
                  <span class={styles.colBadge} style={{ "--bc": "#6366f1" }}>⏱ 24h timeout</span>
                  <span class={styles.colBadge} style={{ "--bc": "#94a3b8" }}>WIP 5</span>
                </div>
              </div>
              <div class={styles.colCard} style={{ "border-style": "dashed", opacity: "0.6" }}>
                <div class={styles.colCardName}>+ Add column</div>
                <div class={styles.colBadges}>
                  <span class={styles.colBadge} style={{ "--bc": "#94a3b8" }}>name · WIP · approval · timeout</span>
                </div>
              </div>
            </div>
            <ul class={styles.vizBullets}>
              <li>Drag to reorder · set WIP limits · require approval</li>
              <li>Stalled tasks auto-surface in Triage</li>
            </ul>
          </div>

          {/* ── Hooks ───────────────────────────────────────────── */}
          <div class={styles.vizBlock}>
            <div class={styles.vizLabel}>🪝 Lifecycle Hooks</div>
            <div class={styles.hooksGrid}>
              <div class={styles.hookTile} style={{ "--ht": "#6366f1" }}>
                <span class={styles.hookTileEvent}>enter</span>
                <span class={styles.hookTileArrow}>→</span>
                <span class={styles.hookTileAction}>🤖 Spawn agent</span>
              </div>
              <div class={styles.hookTile} style={{ "--ht": "#f59e0b" }}>
                <span class={styles.hookTileEvent}>exit</span>
                <span class={styles.hookTileArrow}>→</span>
                <span class={styles.hookTileAction}>📡 Webhook</span>
              </div>
              <div class={styles.hookTile} style={{ "--ht": "#ef4444" }}>
                <span class={styles.hookTileEvent}>on_stall</span>
                <span class={styles.hookTileArrow}>→</span>
                <span class={styles.hookTileAction}>🔔 Triage alert</span>
              </div>
            </div>
            <div class={styles.hookFlow}>
              <div class={styles.hookFlowStep}>📦 Task moves</div>
              <div class={styles.hookFlowArrow}>→</div>
              <div class={styles.hookFlowStep}>🪝 Hook fires</div>
              <div class={styles.hookFlowArrow}>→</div>
              <div class={styles.hookFlowStep}>🤖 Agent / script / webhook</div>
              <div class={styles.hookFlowArrow}>→</div>
              <div class={styles.hookFlowStep}>📡 Output → UI</div>
            </div>
            <ul class={styles.vizBullets}>
              <li>Fires on enter, exit, or stall</li>
              <li>Runs agents, scripts, or webhooks</li>
              <li>Configured per column as JSON</li>
            </ul>
          </div>

        </section>
      </Show>

      {/* ------------------------------------------------------------------ */}
      {/* Future tab                                                          */}
      {/* ------------------------------------------------------------------ */}
      <Show when={tab() === "future"}>
        <section class={styles.techSection}>

          {/* ── Agent Teams ─────────────────────────────────────── */}
          <div class={styles.vizBlock}>
            <div class={styles.vizLabel}>🧑‍🤝‍🧑 Agent Team Management</div>
            <div class={styles.teamDiagram}>
              <div class={styles.teamOrchestrator}>
                <span class={styles.teamRoleIcon}>🎯</span>
                <span class={styles.teamRoleName}>Orchestrator</span>
                <span class={styles.teamModel}>Claude Opus</span>
              </div>
              <div class={styles.teamSpokes}>
                {[
                  { icon: "🏗️", role: "Architect",    model: "Claude Opus" },
                  { icon: "⚡", role: "Implementer",  model: "Claude Sonnet" },
                  { icon: "🔍", role: "Reviewer",     model: "Claude Sonnet" },
                  { icon: "🧪", role: "QA",           model: "Claude Haiku" },
                  { icon: "🔒", role: "Security",     model: "Claude Opus" },
                ].map((r, ri) => (
                  <div class={styles.teamRole} style={{ "animation-delay": `${0.1 + ri * 0.08}s` }}>
                    <span class={styles.teamRoleIcon}>{r.icon}</span>
                    <span class={styles.teamRoleName}>{r.role}</span>
                    <span class={styles.teamModel}>{r.model}</span>
                  </div>
                ))}
              </div>
            </div>
            <ul class={styles.vizBullets}>
              <li>Right model for each role</li>
              <li>Learnings persist across sessions</li>
              <li>Budget limits per role</li>
            </ul>
          </div>

          {/* ── Signal / Noise ──────────────────────────────────── */}
          <div class={styles.vizBlock}>
            <div class={styles.vizLabel}>🎯 Signal, Not Noise</div>
            <div class={styles.funnelDiagram}>
              <div class={styles.funnelLayer} style={{ "--fw": "100%", "--fc": "#334155" }}>
                <span class={styles.funnelIcon}>📨</span>
                <span class={styles.funnelLabel}>All agent events</span>
                <span class={styles.funnelCount}>100%</span>
              </div>
              <div class={styles.funnelArrow}>▼</div>
              <div class={styles.funnelLayer} style={{ "--fw": "60%", "--fc": "#6366f1" }}>
                <span class={styles.funnelIcon}>🤖</span>
                <span class={styles.funnelLabel}>Auto-resolved by agents</span>
                <span class={styles.funnelCount}>~70%</span>
              </div>
              <div class={styles.funnelArrow}>▼</div>
              <div class={styles.funnelLayer} style={{ "--fw": "25%", "--fc": "#f59e0b" }}>
                <span class={styles.funnelIcon}>🎯</span>
                <span class={styles.funnelLabel}>Confidence-scored decisions</span>
                <span class={styles.funnelCount}>~20%</span>
              </div>
              <div class={styles.funnelArrow}>▼</div>
              <div class={styles.funnelLayer} style={{ "--fw": "10%", "--fc": "#22c55e" }}>
                <span class={styles.funnelIcon}>👤</span>
                <span class={styles.funnelLabel}>Human reviews</span>
                <span class={styles.funnelCount}>~10%</span>
              </div>
            </div>
            <ul class={styles.vizBullets}>
              <li>~90% resolved without human input</li>
              <li>Confidence-scored — high confidence auto-approves</li>
            </ul>
          </div>

          {/* ── Rich Previews ───────────────────────────────────── */}
          <div class={styles.vizBlock}>
            <div class={styles.vizLabel}>🌐 Rich Previews</div>
            <div class={styles.previewDemo}>
              <div class={styles.previewPane} style={{ "--pp": "#1e293b" }}>
                <div class={styles.previewPaneTitle}>🔴 Before</div>
                <div class={styles.previewDiffLine} style={{ background: "rgba(239,68,68,0.15)", color: "#fca5a5" }}>- const limit = 100;</div>
                <div class={styles.previewDiffLine} style={{ background: "rgba(34,197,94,0.15)", color: "#86efac" }}>+ const limit = rateLimitFor(user);</div>
                <div class={styles.previewDiffLine} style={{ color: "#475569" }}>  return fetch(url);</div>
              </div>
              <div class={styles.previewPane} style={{ "--pp": "#0f2027" }}>
                <div class={styles.previewPaneTitle}>🌐 Preview</div>
                <div class={styles.previewMockBrowser}>
                  <div class={styles.previewMockBar}>app.example.com/dashboard</div>
                  <div class={styles.previewMockContent}>✨ UI renders here inline</div>
                </div>
              </div>
              <div class={styles.previewActions}>
                <div class={styles.previewActionBtn} style={{ background: "#22c55e" }}>✅ Approve</div>
                <div class={styles.previewActionBtn} style={{ background: "#ef4444" }}>↩️ Reject</div>
              </div>
            </div>
            <ul class={styles.vizBullets}>
              <li>Diff · preview · approve — all in one view</li>
              <li>No context switching to browser or IDE</li>
            </ul>
          </div>

          {/* ── Collaboration ───────────────────────────────────── */}
          <div class={styles.vizBlock}>
            <div class={styles.vizLabel}>🤝 Collaboration</div>

            {/* Problem statement */}
            <p class={styles.collabProblem}>
              10 agents running in parallel — one engineer can't keep up with the review load.
            </p>

            {/* Shared triage queue diagram */}
            <div class={styles.collabDiagram}>
              {/* Center: triage queue node */}
              <div class={styles.collabQueueNode}>
                <span class={styles.collabQueueIcon}>🎯</span>
                <span class={styles.collabQueueLabel}>Shared triage queue</span>
                <div class={styles.collabQueueItems}>
                  <span class={styles.collabQueueItem} style={{ "--qi": "#ef4444" }}>P0 · memory leak blocked</span>
                  <span class={styles.collabQueueItem} style={{ "--qi": "#f59e0b" }}>P1 · planning approval</span>
                  <span class={styles.collabQueueItem} style={{ "--qi": "#6366f1" }}>P2 · testing sign-off</span>
                  <span class={styles.collabQueueItem} style={{ "--qi": "#94a3b8" }}>P3 · schema review</span>
                </div>
              </div>

              {/* Connector lines + engineer cards */}
              <div class={styles.collabConnectors}>
                <div class={styles.collabConnectorLine} />
              </div>

              {/* Engineer nodes */}
              <div class={styles.collabEngineers}>
                {([
                  { name: "Alice", color: "#ef4444", decision: "unblocking memory leak" },
                  { name: "Bob",   color: "#f59e0b", decision: "approving planning" },
                  { name: "Carol", color: "#6366f1", decision: "signing off testing" },
                ] as { name: string; color: string; decision: string }[]).map((e, ei) => (
                  <div
                    class={styles.collabEngineerCard}
                    style={{ "--ec": e.color, "animation-delay": `${0.15 + ei * 0.1}s` }}
                  >
                    <span class={styles.collabEngineerAvatar}>{e.name[0]}</span>
                    <span class={styles.collabEngineerName}>{e.name}</span>
                    <span class={styles.collabEngineerDecision}>{e.decision}</span>
                  </div>
                ))}
              </div>
            </div>

            {/* Capability pills row */}
            <div class={styles.collabCaps}>
              {[
                "Shared triage inbox",
                "Decision assignment",
                "Live presence",
                "Audit trail",
              ].map((label, ci) => (
                <span
                  class={styles.collabCapPill}
                  style={{ "animation-delay": `${0.45 + ci * 0.07}s` }}
                >
                  {label}
                </span>
              ))}
            </div>

            {/* Vision sentence */}
            <p class={styles.collabVision}>
              Scale from 3 agents to 30 without scaling the review bottleneck — the team shares the inbox.
            </p>
          </div>

          {/* Vision */}
          <div class={styles.visionBlock}>
            <span class={styles.visionIcon}>🔭</span>
            <p class={styles.visionText}>
              Morning digest → approve a handful of decisions → agents ship a sprint by end of day.
            </p>
          </div>

        </section>
      </Show>
    </div>
  );
};

export default HowItWorksView;
