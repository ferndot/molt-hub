//! Activity-based health monitoring for agent processes.
//!
//! Tracks output and file-change timestamps for each agent and derives a
//! [`HealthStatus`] by comparing those timestamps to configurable thresholds.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::broadcast;
use tracing::{debug, info};

use molt_hub_core::model::AgentId;

// ---------------------------------------------------------------------------
// ActivityType
// ---------------------------------------------------------------------------

/// The kind of activity an agent can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityType {
    /// The agent produced stdout/stderr output.
    Output,
    /// The agent changed a file in its worktree.
    FileChange,
    /// A heartbeat signal from the process itself.
    ProcessHeartbeat,
}

// ---------------------------------------------------------------------------
// HealthStatus
// ---------------------------------------------------------------------------

/// The derived health status for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Agent is active: recent output or file changes, or within warning threshold.
    Healthy,
    /// Agent has been quiet for longer than `warning_after` but not yet `stuck_after`.
    Warning,
    /// Agent has not produced any output OR file changes for longer than `stuck_after`.
    Stuck,
    /// The agent process is no longer alive.
    Dead,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "Healthy"),
            HealthStatus::Warning => write!(f, "Warning"),
            HealthStatus::Stuck => write!(f, "Stuck"),
            HealthStatus::Dead => write!(f, "Dead"),
        }
    }
}

// ---------------------------------------------------------------------------
// HealthConfig
// ---------------------------------------------------------------------------

/// Thresholds used to derive health status from silence duration.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    /// After this long with no activity, transition to `Warning`.
    pub warning_after: Duration,
    /// After this long with no activity, transition to `Stuck`.
    pub stuck_after: Duration,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            warning_after: Duration::from_secs(60),
            stuck_after: Duration::from_secs(300),
        }
    }
}

// ---------------------------------------------------------------------------
// AgentHealth
// ---------------------------------------------------------------------------

/// Tracked health state for a single agent.
#[derive(Debug, Clone)]
pub struct AgentHealth {
    pub agent_id: AgentId,
    pub last_output_at: Option<Instant>,
    pub last_file_change_at: Option<Instant>,
    /// Set to true when the agent's process is known to have exited.
    pub is_dead: bool,
    /// The most recently computed health status.
    pub status: HealthStatus,
}

impl AgentHealth {
    fn new(agent_id: AgentId) -> Self {
        Self {
            agent_id,
            last_output_at: None,
            last_file_change_at: None,
            is_dead: false,
            status: HealthStatus::Healthy,
        }
    }

    /// Return the most recent activity instant across all tracked dimensions.
    fn most_recent_activity(&self) -> Option<Instant> {
        match (self.last_output_at, self.last_file_change_at) {
            (Some(a), Some(b)) => Some(if a > b { a } else { b }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    /// Compute the health status given the current wall-clock and config.
    ///
    /// Logic:
    /// - `Dead`    — process has exited
    /// - `Healthy` — file changes happening (agent is working silently) OR last
    ///               activity within `warning_after`
    /// - `Warning` — last activity between `warning_after` and `stuck_after`
    /// - `Stuck`   — no activity of any kind for longer than `stuck_after`, AND
    ///               no file changes are happening
    fn compute_status(&self, now: Instant, config: &HealthConfig) -> HealthStatus {
        if self.is_dead {
            return HealthStatus::Dead;
        }

        let Some(last) = self.most_recent_activity() else {
            // No activity recorded at all — treat as healthy (just started).
            return HealthStatus::Healthy;
        };

        let silence = now.saturating_duration_since(last);

        if silence < config.warning_after {
            return HealthStatus::Healthy;
        }

        // Silent for at least warning_after. Check whether file changes are
        // keeping the agent "alive" even without output.
        if let Some(fc) = self.last_file_change_at {
            let file_silence = now.saturating_duration_since(fc);
            if file_silence < config.warning_after {
                // File changes are recent — agent is working silently.
                return HealthStatus::Healthy;
            }
        }

        if silence >= config.stuck_after {
            HealthStatus::Stuck
        } else {
            HealthStatus::Warning
        }
    }
}

// ---------------------------------------------------------------------------
// HealthMonitor
// ---------------------------------------------------------------------------

/// Tracks activity-based health for a collection of agents.
///
/// Internally uses a [`DashMap`] so it can be shared cheaply across threads.
pub struct HealthMonitor {
    agents: Arc<DashMap<AgentId, AgentHealth>>,
    config: HealthConfig,
}

impl HealthMonitor {
    /// Create a new monitor with the given thresholds.
    pub fn new(config: HealthConfig) -> Self {
        Self {
            agents: Arc::new(DashMap::new()),
            config,
        }
    }

    /// Register an agent to be monitored.
    pub fn register(&self, agent_id: AgentId) {
        debug!(agent_id = %agent_id, "registering agent in health monitor");
        self.agents
            .insert(agent_id.clone(), AgentHealth::new(agent_id));
    }

    /// Remove an agent from monitoring.
    pub fn unregister(&self, agent_id: &AgentId) {
        debug!(agent_id = %agent_id, "unregistering agent from health monitor");
        self.agents.remove(agent_id);
    }

    /// Record an activity event for an agent.
    ///
    /// If the agent is not registered, the call is a no-op.
    pub fn record_activity(&self, agent_id: &AgentId, activity: ActivityType) {
        let Some(mut entry) = self.agents.get_mut(agent_id) else {
            return;
        };

        let now = Instant::now();
        match activity {
            ActivityType::Output | ActivityType::ProcessHeartbeat => {
                entry.last_output_at = Some(now);
            }
            ActivityType::FileChange => {
                entry.last_file_change_at = Some(now);
            }
        }

        // Eagerly re-compute so `.status` is always fresh.
        let new_status = entry.compute_status(now, &self.config);
        entry.status = new_status;
    }

    /// Mark an agent's process as dead.
    pub fn mark_dead(&self, agent_id: &AgentId) {
        let Some(mut entry) = self.agents.get_mut(agent_id) else {
            return;
        };
        entry.is_dead = true;
        entry.status = HealthStatus::Dead;
    }

    /// Compute and return the current health status for an agent.
    ///
    /// Returns `None` if the agent is not registered.
    pub fn check_health(&self, agent_id: &AgentId) -> Option<HealthStatus> {
        let mut entry = self.agents.get_mut(agent_id)?;
        let now = Instant::now();
        let status = entry.compute_status(now, &self.config);
        entry.status = status;
        Some(status)
    }

    /// Snapshot the current health for every monitored agent.
    pub fn check_all(&self) -> Vec<AgentHealth> {
        let now = Instant::now();
        self.agents
            .iter_mut()
            .map(|mut e| {
                let status = e.compute_status(now, &self.config);
                e.status = status;
                e.clone()
            })
            .collect()
    }

    /// Start a background task that periodically checks all agents and
    /// broadcasts [`HealthStatusChange`] events whenever a status changes.
    ///
    /// Returns a [`broadcast::Receiver`] and a [`tokio::task::JoinHandle`].
    /// Call `.abort()` on the handle to stop the loop.
    pub fn run_health_checks(
        self: &Arc<Self>,
        interval: Duration,
    ) -> (
        broadcast::Receiver<HealthStatusChange>,
        tokio::task::JoinHandle<()>,
    ) {
        let (tx, rx) = broadcast::channel(64);
        let monitor = Arc::clone(self);

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            // Track previous statuses to emit changes only.
            let mut prev: std::collections::HashMap<AgentId, HealthStatus> =
                std::collections::HashMap::new();

            loop {
                ticker.tick().await;

                let now = Instant::now();
                for mut entry in monitor.agents.iter_mut() {
                    let new_status = entry.compute_status(now, &monitor.config);
                    entry.status = new_status;
                    let agent_id = entry.key().clone();

                    let old_status = prev.get(&agent_id).copied();
                    if old_status != Some(new_status) {
                        if let Some(old) = old_status {
                            info!(
                                agent_id = %agent_id,
                                from     = %old,
                                to       = %new_status,
                                "health status changed"
                            );
                        } else {
                            debug!(agent_id = %agent_id, status = %new_status, "initial health status");
                        }

                        let change = HealthStatusChange {
                            agent_id: agent_id.clone(),
                            old_status,
                            new_status,
                        };
                        // Best-effort broadcast; ignore lag errors.
                        let _ = tx.send(change);
                    }

                    prev.insert(agent_id, new_status);
                }

                // Prune agents that are no longer registered.
                prev.retain(|id, _| monitor.agents.contains_key(id));
            }
        });

        (rx, handle)
    }

    /// Return an [`Arc`]-clone of the underlying agent map (useful for sharing
    /// the monitor across components without cloning the whole struct).
    pub fn agents_arc(&self) -> Arc<DashMap<AgentId, AgentHealth>> {
        Arc::clone(&self.agents)
    }
}

// ---------------------------------------------------------------------------
// HealthStatusChange
// ---------------------------------------------------------------------------

/// An event emitted by the health-check loop when an agent's status changes.
#[derive(Debug, Clone)]
pub struct HealthStatusChange {
    pub agent_id: AgentId,
    /// `None` on the very first observation of an agent.
    pub old_status: Option<HealthStatus>,
    pub new_status: HealthStatus,
}

// ---------------------------------------------------------------------------
// Arc<HealthMonitor> convenience
// ---------------------------------------------------------------------------

impl HealthMonitor {
    /// Wrap `self` in an [`Arc`] for sharing across threads.
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use molt_hub_core::model::AgentId;
    use std::time::Duration;

    fn make_monitor(warning: u64, stuck: u64) -> HealthMonitor {
        HealthMonitor::new(HealthConfig {
            warning_after: Duration::from_secs(warning),
            stuck_after: Duration::from_secs(stuck),
        })
    }

    // -----------------------------------------------------------------------
    // compute_status unit tests (time-shifted via `AgentHealth` manipulation)
    // -----------------------------------------------------------------------

    fn health_with_last_activity(offset_secs: u64) -> AgentHealth {
        let mut h = AgentHealth::new(AgentId::new());
        // Simulate an activity that happened `offset_secs` ago.
        h.last_output_at = Some(Instant::now() - Duration::from_secs(offset_secs));
        h
    }

    #[test]
    fn test_healthy_within_warning_threshold() {
        let config = HealthConfig {
            warning_after: Duration::from_secs(60),
            stuck_after: Duration::from_secs(300),
        };
        let h = health_with_last_activity(30); // 30s ago — well within threshold
        assert_eq!(
            h.compute_status(Instant::now(), &config),
            HealthStatus::Healthy
        );
    }

    #[test]
    fn test_warning_between_thresholds() {
        let config = HealthConfig {
            warning_after: Duration::from_secs(10),
            stuck_after: Duration::from_secs(60),
        };
        let h = health_with_last_activity(20); // 20s ago — past warning, not stuck
        assert_eq!(
            h.compute_status(Instant::now(), &config),
            HealthStatus::Warning
        );
    }

    #[test]
    fn test_stuck_past_stuck_threshold() {
        let config = HealthConfig {
            warning_after: Duration::from_secs(5),
            stuck_after: Duration::from_secs(10),
        };
        let h = health_with_last_activity(15); // 15s ago — past stuck threshold
        assert_eq!(
            h.compute_status(Instant::now(), &config),
            HealthStatus::Stuck
        );
    }

    #[test]
    fn test_dead_overrides_all() {
        let config = HealthConfig {
            warning_after: Duration::from_secs(60),
            stuck_after: Duration::from_secs(300),
        };
        let mut h = health_with_last_activity(0); // Just active
        h.is_dead = true;
        assert_eq!(
            h.compute_status(Instant::now(), &config),
            HealthStatus::Dead
        );
    }

    #[test]
    fn test_file_activity_keeps_agent_healthy_without_output() {
        let config = HealthConfig {
            warning_after: Duration::from_secs(60),
            stuck_after: Duration::from_secs(300),
        };
        let mut h = AgentHealth::new(AgentId::new());
        // No output for 90s (past warning threshold).
        h.last_output_at = Some(Instant::now() - Duration::from_secs(90));
        // But file change just happened.
        h.last_file_change_at = Some(Instant::now() - Duration::from_secs(5));

        // Should still be Healthy because file changes are recent.
        assert_eq!(
            h.compute_status(Instant::now(), &config),
            HealthStatus::Healthy
        );
    }

    #[test]
    fn test_no_activity_at_start_is_healthy() {
        let config = HealthConfig::default();
        let h = AgentHealth::new(AgentId::new());
        // Freshly registered — no activity timestamps at all.
        assert_eq!(
            h.compute_status(Instant::now(), &config),
            HealthStatus::Healthy
        );
    }

    // -----------------------------------------------------------------------
    // HealthMonitor integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_register_and_check_health_healthy() {
        let monitor = make_monitor(60, 300);
        let id = AgentId::new();
        monitor.register(id.clone());

        // No activity yet — should be Healthy (just started).
        assert_eq!(monitor.check_health(&id), Some(HealthStatus::Healthy));
    }

    #[test]
    fn test_record_output_keeps_healthy() {
        let monitor = make_monitor(60, 300);
        let id = AgentId::new();
        monitor.register(id.clone());
        monitor.record_activity(&id, ActivityType::Output);
        assert_eq!(monitor.check_health(&id), Some(HealthStatus::Healthy));
    }

    #[test]
    fn test_record_file_change_keeps_healthy() {
        let monitor = make_monitor(60, 300);
        let id = AgentId::new();
        monitor.register(id.clone());
        monitor.record_activity(&id, ActivityType::FileChange);
        assert_eq!(monitor.check_health(&id), Some(HealthStatus::Healthy));
    }

    #[test]
    fn test_record_heartbeat_updates_output_timestamp() {
        let monitor = make_monitor(60, 300);
        let id = AgentId::new();
        monitor.register(id.clone());
        monitor.record_activity(&id, ActivityType::ProcessHeartbeat);

        let entry = monitor.agents.get(&id).unwrap();
        assert!(
            entry.last_output_at.is_some(),
            "heartbeat should set last_output_at"
        );
    }

    #[test]
    fn test_mark_dead_returns_dead_status() {
        let monitor = make_monitor(60, 300);
        let id = AgentId::new();
        monitor.register(id.clone());
        monitor.record_activity(&id, ActivityType::Output);
        monitor.mark_dead(&id);
        assert_eq!(monitor.check_health(&id), Some(HealthStatus::Dead));
    }

    #[test]
    fn test_unregistered_agent_returns_none() {
        let monitor = make_monitor(60, 300);
        let id = AgentId::new();
        assert_eq!(monitor.check_health(&id), None);
    }

    #[test]
    fn test_unregister_removes_agent() {
        let monitor = make_monitor(60, 300);
        let id = AgentId::new();
        monitor.register(id.clone());
        assert!(monitor.check_health(&id).is_some());
        monitor.unregister(&id);
        assert!(monitor.check_health(&id).is_none());
    }

    #[test]
    fn test_check_all_returns_all_agents() {
        let monitor = make_monitor(60, 300);
        let id1 = AgentId::new();
        let id2 = AgentId::new();
        monitor.register(id1.clone());
        monitor.register(id2.clone());

        let all = monitor.check_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_configurable_thresholds_warning() {
        let monitor = make_monitor(10, 60);
        let id = AgentId::new();
        monitor.register(id.clone());

        // Manually backdate last_output_at to 20s ago (past 10s warning, before 60s stuck).
        {
            let mut entry = monitor.agents.get_mut(&id).unwrap();
            entry.last_output_at = Some(Instant::now() - Duration::from_secs(20));
        }

        assert_eq!(monitor.check_health(&id), Some(HealthStatus::Warning));
    }

    #[test]
    fn test_configurable_thresholds_stuck() {
        let monitor = make_monitor(5, 10);
        let id = AgentId::new();
        monitor.register(id.clone());

        {
            let mut entry = monitor.agents.get_mut(&id).unwrap();
            entry.last_output_at = Some(Instant::now() - Duration::from_secs(15));
        }

        assert_eq!(monitor.check_health(&id), Some(HealthStatus::Stuck));
    }

    #[test]
    fn test_file_activity_prevents_stuck_even_with_old_output() {
        let monitor = make_monitor(5, 10);
        let id = AgentId::new();
        monitor.register(id.clone());

        {
            let mut entry = monitor.agents.get_mut(&id).unwrap();
            // Output was 20s ago — past stuck threshold.
            entry.last_output_at = Some(Instant::now() - Duration::from_secs(20));
            // File change was 2s ago — within warning threshold.
            entry.last_file_change_at = Some(Instant::now() - Duration::from_secs(2));
        }

        // File change is recent, so Healthy.
        assert_eq!(monitor.check_health(&id), Some(HealthStatus::Healthy));
    }

    // -----------------------------------------------------------------------
    // Async run_health_checks test
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_run_health_checks_emits_status_change() {
        let monitor = Arc::new(make_monitor(5, 10));
        let id = AgentId::new();
        monitor.register(id.clone());

        // Backdate output to trigger Warning/Stuck.
        {
            let mut entry = monitor.agents.get_mut(&id).unwrap();
            entry.last_output_at = Some(Instant::now() - Duration::from_secs(15));
        }

        let (mut rx, handle) = monitor.run_health_checks(Duration::from_millis(50));

        // Wait for at least one event.
        let event = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .expect("timed out waiting for health event")
            .expect("channel closed");

        assert_eq!(event.agent_id, id);
        assert_eq!(event.new_status, HealthStatus::Stuck);

        handle.abort();
    }
}
