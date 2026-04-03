//! Process supervisor — spawn, monitor, and reap agent processes.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use molt_hub_core::model::{AgentId, AgentStatus, TaskId};

use crate::acp::AcpInternal;
use crate::adapter::{
    AdapterError, AgentAdapter, AgentEvent, AgentHandle, AgentMessage, SpawnConfig,
};

// ---------------------------------------------------------------------------
// SteerMessage / SteerPriority
// ---------------------------------------------------------------------------

/// Priority level for a steering message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SteerPriority {
    Normal,
    Urgent,
}

impl Default for SteerPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// A message sent to steer a running agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerMessage {
    pub message: String,
    pub priority: SteerPriority,
}

// ---------------------------------------------------------------------------
// SupervisorConfig
// ---------------------------------------------------------------------------

/// Configuration for the process supervisor.
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// Maximum number of concurrent agent processes.
    pub max_agents: usize,
    /// How often to poll each agent's status.
    pub health_check_interval: Duration,
    /// How long to wait for graceful shutdown before issuing a force-kill.
    pub graceful_shutdown_timeout: Duration,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            max_agents: 16,
            health_check_interval: Duration::from_secs(5),
            graceful_shutdown_timeout: Duration::from_secs(30),
        }
    }
}

// ---------------------------------------------------------------------------
// SupervisorError
// ---------------------------------------------------------------------------

/// Errors returned by `Supervisor` operations.
#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("maximum agent limit ({0}) reached")]
    MaxAgentsReached(usize),

    #[error("agent not found: {0}")]
    AgentNotFound(AgentId),

    #[error("agent not running: {0}")]
    AgentNotRunning(AgentId),

    #[error("adapter error: {0}")]
    AdapterError(#[from] AdapterError),

    #[error("graceful shutdown timed out waiting for agent {0}")]
    ShutdownTimeout(AgentId),
}

// ---------------------------------------------------------------------------
// ManagedAgent
// ---------------------------------------------------------------------------

/// Internal bookkeeping for a single supervised agent.
pub struct ManagedAgent {
    pub handle: AgentHandle,
    pub task_id: TaskId,
    pub adapter: Arc<dyn AgentAdapter>,
    pub started_at: DateTime<Utc>,
    pub last_health_check: DateTime<Utc>,
    /// Optional project this agent belongs to. `None` means the global / default context.
    pub project_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

/// Manages the lifecycle of multiple agent processes.
///
/// The supervisor spawns agents via [`AgentAdapter`], tracks their state,
/// performs periodic health checks, and handles both graceful and forced
/// shutdowns.
pub struct Supervisor {
    agents: Arc<DashMap<AgentId, ManagedAgent>>,
    config: SupervisorConfig,
    event_tx: broadcast::Sender<AgentEvent>,
}

impl Supervisor {
    /// Construct a new supervisor.
    pub fn new(config: SupervisorConfig, event_tx: broadcast::Sender<AgentEvent>) -> Self {
        Self {
            agents: Arc::new(DashMap::new()),
            config,
            event_tx,
        }
    }

    /// Spawn a new agent.
    ///
    /// Returns the `AgentId` of the newly started agent. Fails with
    /// [`SupervisorError::MaxAgentsReached`] if the concurrent agent limit is
    /// already at capacity.
    pub async fn spawn_agent(
        &self,
        adapter: Arc<dyn AgentAdapter>,
        mut spawn_config: SpawnConfig,
    ) -> Result<AgentId, SupervisorError> {
        if self.agents.len() >= self.config.max_agents {
            return Err(SupervisorError::MaxAgentsReached(self.config.max_agents));
        }

        // Inject the global event channel so that adapter threads can emit
        // events visible to the WS fanout layer.
        if spawn_config.event_tx.is_none() {
            spawn_config.event_tx = Some(self.event_tx.clone());
        }

        let agent_id = spawn_config.agent_id.clone();
        let task_id = spawn_config.task_id.clone();
        let project_id = spawn_config.project_id.clone();

        info!(
            agent_id = %agent_id,
            task_id  = %task_id,
            adapter  = adapter.adapter_type(),
            "spawning agent"
        );

        let handle = adapter.spawn(spawn_config).await?;
        let now = Utc::now();

        let managed = ManagedAgent {
            handle,
            task_id,
            adapter,
            started_at: now,
            last_health_check: now,
            project_id,
        };

        self.agents.insert(agent_id.clone(), managed);
        debug!(agent_id = %agent_id, "agent inserted into supervisor map");

        Ok(agent_id)
    }

    /// Gracefully terminate an agent.
    ///
    /// Calls [`AgentAdapter::terminate`], waits up to
    /// `graceful_shutdown_timeout`, then calls [`AgentAdapter::abort`] if the
    /// agent has not stopped.
    pub async fn terminate_agent(&self, agent_id: &AgentId) -> Result<(), SupervisorError> {
        let managed = self
            .agents
            .get(agent_id)
            .ok_or_else(|| SupervisorError::AgentNotFound(agent_id.clone()))?;

        info!(agent_id = %agent_id, "requesting graceful termination");
        managed.adapter.terminate(&managed.handle).await?;

        let deadline = tokio::time::Instant::now() + self.config.graceful_shutdown_timeout;
        let poll_interval = Duration::from_millis(200);

        // Poll until the agent is no longer running or we hit the deadline.
        loop {
            if tokio::time::Instant::now() >= deadline {
                warn!(
                    agent_id = %agent_id,
                    "graceful shutdown timed out, aborting"
                );
                managed.adapter.abort(&managed.handle).await?;
                break;
            }

            match managed.adapter.status(&managed.handle).await {
                Ok(AgentStatus::Terminated) | Ok(AgentStatus::Crashed { .. }) => {
                    debug!(agent_id = %agent_id, "agent stopped after graceful termination");
                    break;
                }
                Ok(_) => {
                    tokio::time::sleep(poll_interval).await;
                }
                Err(e) => {
                    error!(agent_id = %agent_id, error = %e, "status check failed during shutdown");
                    break;
                }
            }
        }

        // Release the borrow before removing.
        drop(managed);
        self.agents.remove(agent_id);
        Ok(())
    }

    /// Pause a running agent.
    ///
    /// Sends a [`AgentMessage::Pause`] to the adapter. The exact semantics
    /// depend on the adapter implementation (e.g. status-only tracking or
    /// SIGSTOP on Unix).
    pub async fn pause_agent(&self, agent_id: &AgentId) -> Result<(), SupervisorError> {
        let managed = self
            .agents
            .get(agent_id)
            .ok_or_else(|| SupervisorError::AgentNotFound(agent_id.clone()))?;

        info!(agent_id = %agent_id, "pausing agent");
        managed
            .adapter
            .send(&managed.handle, crate::adapter::AgentMessage::Pause)
            .await?;

        Ok(())
    }

    /// Resume a previously paused agent.
    ///
    /// Sends a [`AgentMessage::Resume`] to the adapter.
    pub async fn resume_agent(&self, agent_id: &AgentId) -> Result<(), SupervisorError> {
        let managed = self
            .agents
            .get(agent_id)
            .ok_or_else(|| SupervisorError::AgentNotFound(agent_id.clone()))?;

        info!(agent_id = %agent_id, "resuming agent");
        managed
            .adapter
            .send(&managed.handle, crate::adapter::AgentMessage::Resume)
            .await?;

        Ok(())
    }

    /// Send a steering message to a running agent.
    ///
    /// The message is delivered as an [`AgentMessage::Instruction`] via the
    /// adapter's `send` method. Returns [`SupervisorError::AgentNotRunning`] if
    /// the agent exists but is not in a running state.
    pub async fn steer(
        &self,
        agent_id: &AgentId,
        steer_msg: SteerMessage,
    ) -> Result<(), SupervisorError> {
        let managed = self
            .agents
            .get(agent_id)
            .ok_or_else(|| SupervisorError::AgentNotFound(agent_id.clone()))?;

        // Check the agent is actually running.
        let status = managed.adapter.status(&managed.handle).await?;
        match status {
            AgentStatus::Running => {}
            _ => return Err(SupervisorError::AgentNotRunning(agent_id.clone())),
        }

        info!(agent_id = %agent_id, priority = ?steer_msg.priority, "steering agent");

        managed
            .adapter
            .send(
                &managed.handle,
                AgentMessage::Instruction(steer_msg.message),
            )
            .await?;

        Ok(())
    }

    /// Send a tool-use approval decision to a waiting agent.
    ///
    /// Returns `Ok(())` even if the agent has no pending approval (send to
    /// a broadcast channel with no active receivers is a no-op).
    pub async fn approve_tool(
        &self,
        agent_id: &AgentId,
        approved: bool,
    ) -> Result<(), SupervisorError> {
        let managed = self
            .agents
            .get(agent_id)
            .ok_or_else(|| SupervisorError::AgentNotFound(agent_id.clone()))?;
        if let Some(internal) = managed.handle.downcast_internal::<AcpInternal>() {
            let _ = internal.approve_tx.send(approved);
        }
        Ok(())
    }

    /// Immediately kill an agent without waiting for cleanup.
    pub async fn abort_agent(&self, agent_id: &AgentId) -> Result<(), SupervisorError> {
        let managed = self
            .agents
            .get(agent_id)
            .ok_or_else(|| SupervisorError::AgentNotFound(agent_id.clone()))?;

        warn!(agent_id = %agent_id, "aborting agent immediately");
        managed.adapter.abort(&managed.handle).await?;
        drop(managed);
        self.agents.remove(agent_id);
        Ok(())
    }

    /// Query the current status of an agent without removing it from the map.
    pub async fn get_status(&self, agent_id: &AgentId) -> Option<AgentStatus> {
        let managed = self.agents.get(agent_id)?;
        managed.adapter.status(&managed.handle).await.ok()
    }

    /// Return a snapshot of all active agents: `(AgentId, TaskId, AgentStatus)`.
    pub async fn list_agents(&self) -> Vec<(AgentId, TaskId, AgentStatus)> {
        let mut result = Vec::with_capacity(self.agents.len());

        for entry in self.agents.iter() {
            let agent_id = entry.key().clone();
            let task_id = entry.value().task_id.clone();
            let status = entry
                .value()
                .adapter
                .status(&entry.value().handle)
                .await
                .unwrap_or(AgentStatus::Crashed {
                    error: "status unavailable".into(),
                });
            result.push((agent_id, task_id, status));
        }

        result
    }

    /// Return a snapshot of all active agents with their project IDs:
    /// `(AgentId, TaskId, AgentStatus, Option<String>)`.
    pub async fn list_agents_with_project(
        &self,
    ) -> Vec<(AgentId, TaskId, AgentStatus, Option<String>)> {
        let mut result = Vec::with_capacity(self.agents.len());

        for entry in self.agents.iter() {
            let agent_id = entry.key().clone();
            let task_id = entry.value().task_id.clone();
            let project_id = entry.value().project_id.clone();
            let status = entry
                .value()
                .adapter
                .status(&entry.value().handle)
                .await
                .unwrap_or(AgentStatus::Crashed {
                    error: "status unavailable".into(),
                });
            result.push((agent_id, task_id, status, project_id));
        }

        result
    }

    /// Gracefully shut down every supervised agent.
    pub async fn shutdown_all(&self) {
        info!("supervisor shutting down all agents");

        // Collect IDs first to avoid holding the map lock while awaiting.
        let ids: Vec<AgentId> = self.agents.iter().map(|e| e.key().clone()).collect();

        for agent_id in ids {
            if let Err(e) = self.terminate_agent(&agent_id).await {
                error!(
                    agent_id = %agent_id,
                    error   = %e,
                    "error during shutdown_all for agent"
                );
            }
        }

        info!("supervisor shutdown complete");
    }

    /// Return the current number of supervised agents.
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    // -----------------------------------------------------------------------
    // Health monitor
    // -----------------------------------------------------------------------

    /// Start a background health-check loop.
    ///
    /// The returned `JoinHandle` runs until all clones of the supervisor's
    /// internal map are dropped.  Call `.abort()` on the handle to stop it.
    pub fn start_health_monitor(&self) -> tokio::task::JoinHandle<()> {
        let agents = Arc::clone(&self.agents);
        let interval = self.config.health_check_interval;
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                ticker.tick().await;

                let ids: Vec<AgentId> = agents.iter().map(|e| e.key().clone()).collect();

                for agent_id in ids {
                    let Some(mut entry) = agents.get_mut(&agent_id) else {
                        continue;
                    };

                    match entry.adapter.status(&entry.handle).await {
                        Ok(AgentStatus::Crashed { ref error }) => {
                            error!(
                                agent_id = %agent_id,
                                error   = %error,
                                "health monitor detected crashed agent"
                            );
                            let _ = event_tx.send(AgentEvent::Error {
                                agent_id: agent_id.clone(),
                                message: format!("agent crashed: {error}"),
                                timestamp: Utc::now(),
                            });
                            // Release entry before removing to avoid deadlock.
                            drop(entry);
                            agents.remove(&agent_id);
                        }
                        Ok(_) => {
                            entry.last_health_check = Utc::now();
                        }
                        Err(e) => {
                            warn!(
                                agent_id = %agent_id,
                                error   = %e,
                                "health check returned error"
                            );
                        }
                    }
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{AgentMessage, SpawnConfig};
    use async_trait::async_trait;
    use molt_hub_core::model::{SessionId, TaskId};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // -----------------------------------------------------------------------
    // MockAdapter
    // -----------------------------------------------------------------------

    /// A deterministic adapter that never actually spawns processes.
    struct MockAdapter {
        spawn_count: Arc<AtomicUsize>,
        terminate_count: Arc<AtomicUsize>,
        abort_count: Arc<AtomicUsize>,
        /// The status that `status()` will return for every handle.
        fixed_status: AgentStatus,
        /// If true, `spawn()` returns an error.
        fail_spawn: bool,
    }

    impl MockAdapter {
        fn new(fixed_status: AgentStatus) -> Self {
            Self {
                spawn_count: Arc::new(AtomicUsize::new(0)),
                terminate_count: Arc::new(AtomicUsize::new(0)),
                abort_count: Arc::new(AtomicUsize::new(0)),
                fixed_status,
                fail_spawn: false,
            }
        }

        fn failing() -> Self {
            let mut a = Self::new(AgentStatus::Idle);
            a.fail_spawn = true;
            a
        }
    }

    #[async_trait]
    impl AgentAdapter for MockAdapter {
        async fn spawn(&self, config: SpawnConfig) -> Result<AgentHandle, AdapterError> {
            if self.fail_spawn {
                return Err(AdapterError::SpawnFailed("mock failure".into()));
            }
            self.spawn_count.fetch_add(1, Ordering::SeqCst);
            Ok(AgentHandle::new(config.agent_id, None, Box::new(())))
        }

        async fn send(
            &self,
            _handle: &AgentHandle,
            _message: AgentMessage,
        ) -> Result<(), AdapterError> {
            Ok(())
        }

        async fn status(&self, _handle: &AgentHandle) -> Result<AgentStatus, AdapterError> {
            Ok(self.fixed_status.clone())
        }

        async fn terminate(&self, _handle: &AgentHandle) -> Result<(), AdapterError> {
            self.terminate_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn abort(&self, _handle: &AgentHandle) -> Result<(), AdapterError> {
            self.abort_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn adapter_type(&self) -> &str {
            "mock"
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_config(max_agents: usize) -> SupervisorConfig {
        SupervisorConfig {
            max_agents,
            health_check_interval: Duration::from_secs(60),
            graceful_shutdown_timeout: Duration::from_millis(100),
        }
    }

    fn make_spawn_config() -> SpawnConfig {
        SpawnConfig {
            agent_id: AgentId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            working_dir: PathBuf::from("/tmp"),
            instructions: "do something".into(),
            env: HashMap::new(),
            timeout: None,
            adapter_config: serde_json::Value::Null,
            project_id: None,
            event_tx: None,
        }
    }

    fn make_supervisor(max_agents: usize) -> (Supervisor, broadcast::Receiver<AgentEvent>) {
        let (tx, rx) = broadcast::channel(64);
        let sup = Supervisor::new(make_config(max_agents), tx);
        (sup, rx)
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_spawn_and_count() {
        let (sup, _rx) = make_supervisor(4);
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::new(AgentStatus::Running));

        let id = sup
            .spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();

        assert_eq!(sup.agent_count(), 1);
        assert!(sup.get_status(&id).await.is_some());
    }

    #[tokio::test]
    async fn test_spawn_respects_max_agents() {
        let (sup, _rx) = make_supervisor(2);
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::new(AgentStatus::Running));

        sup.spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();
        sup.spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();

        // Third spawn should fail.
        let err = sup
            .spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap_err();

        assert!(
            matches!(err, SupervisorError::MaxAgentsReached(2)),
            "expected MaxAgentsReached, got {err:?}"
        );
        assert_eq!(sup.agent_count(), 2);
    }

    #[tokio::test]
    async fn test_terminate_removes_from_map() {
        let (sup, _rx) = make_supervisor(4);
        // Return Terminated so the poll loop exits immediately.
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::new(AgentStatus::Terminated));

        let id = sup
            .spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();

        assert_eq!(sup.agent_count(), 1);
        sup.terminate_agent(&id).await.unwrap();
        assert_eq!(sup.agent_count(), 0);
    }

    #[tokio::test]
    async fn test_terminate_unknown_agent() {
        let (sup, _rx) = make_supervisor(4);
        let unknown = AgentId::new();
        let err = sup.terminate_agent(&unknown).await.unwrap_err();
        assert!(matches!(err, SupervisorError::AgentNotFound(_)));
    }

    #[tokio::test]
    async fn test_abort_removes_from_map() {
        let (sup, _rx) = make_supervisor(4);
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::new(AgentStatus::Running));

        let id = sup
            .spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();

        sup.abort_agent(&id).await.unwrap();
        assert_eq!(sup.agent_count(), 0);
    }

    #[tokio::test]
    async fn test_list_agents_returns_correct_data() {
        let (sup, _rx) = make_supervisor(4);
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::new(AgentStatus::Running));

        let cfg1 = make_spawn_config();
        let id1 = cfg1.agent_id.clone();
        let task1 = cfg1.task_id.clone();

        let cfg2 = make_spawn_config();
        let id2 = cfg2.agent_id.clone();
        let task2 = cfg2.task_id.clone();

        sup.spawn_agent(Arc::clone(&adapter), cfg1).await.unwrap();
        sup.spawn_agent(Arc::clone(&adapter), cfg2).await.unwrap();

        let list = sup.list_agents().await;
        assert_eq!(list.len(), 2);

        let find = |id: &AgentId| list.iter().find(|(a, _, _)| a == id).cloned();

        let (_, t1, s1) = find(&id1).expect("agent 1 not in list");
        assert_eq!(t1, task1);
        assert_eq!(s1, AgentStatus::Running);

        let (_, t2, s2) = find(&id2).expect("agent 2 not in list");
        assert_eq!(t2, task2);
        assert_eq!(s2, AgentStatus::Running);
    }

    #[tokio::test]
    async fn test_shutdown_all_clears_map() {
        let (sup, _rx) = make_supervisor(4);
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::new(AgentStatus::Terminated));

        sup.spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();
        sup.spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();
        sup.spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();

        assert_eq!(sup.agent_count(), 3);
        sup.shutdown_all().await;
        assert_eq!(sup.agent_count(), 0);
    }

    #[tokio::test]
    async fn test_spawn_adapter_failure_propagates() {
        let (sup, _rx) = make_supervisor(4);
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::failing());

        let err = sup
            .spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap_err();

        assert!(matches!(err, SupervisorError::AdapterError(_)));
        // Nothing should have been inserted.
        assert_eq!(sup.agent_count(), 0);
    }

    #[tokio::test]
    async fn test_pause_agent() {
        let (sup, _rx) = make_supervisor(4);
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::new(AgentStatus::Running));

        let id = sup
            .spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();

        // Pause should succeed without error.
        sup.pause_agent(&id).await.unwrap();
    }

    #[tokio::test]
    async fn test_pause_unknown_agent() {
        let (sup, _rx) = make_supervisor(4);
        let unknown = AgentId::new();
        let err = sup.pause_agent(&unknown).await.unwrap_err();
        assert!(matches!(err, SupervisorError::AgentNotFound(_)));
    }

    #[tokio::test]
    async fn test_resume_agent() {
        let (sup, _rx) = make_supervisor(4);
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::new(AgentStatus::Paused));

        let id = sup
            .spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();

        // Resume should succeed without error.
        sup.resume_agent(&id).await.unwrap();
    }

    #[tokio::test]
    async fn test_resume_unknown_agent() {
        let (sup, _rx) = make_supervisor(4);
        let unknown = AgentId::new();
        let err = sup.resume_agent(&unknown).await.unwrap_err();
        assert!(matches!(err, SupervisorError::AgentNotFound(_)));
    }

    #[tokio::test]
    async fn test_get_status_returns_none_for_unknown() {
        let (sup, _rx) = make_supervisor(4);
        let unknown = AgentId::new();
        assert!(sup.get_status(&unknown).await.is_none());
    }

    // -----------------------------------------------------------------------
    // Steer tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_steer_running_agent_succeeds() {
        let (sup, _rx) = make_supervisor(4);
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::new(AgentStatus::Running));

        let id = sup
            .spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();

        let msg = SteerMessage {
            message: "focus on tests".into(),
            priority: SteerPriority::Normal,
        };
        sup.steer(&id, msg).await.unwrap();
    }

    #[tokio::test]
    async fn test_steer_unknown_agent_returns_not_found() {
        let (sup, _rx) = make_supervisor(4);
        let unknown = AgentId::new();
        let msg = SteerMessage {
            message: "hello".into(),
            priority: SteerPriority::Normal,
        };
        let err = sup.steer(&unknown, msg).await.unwrap_err();
        assert!(matches!(err, SupervisorError::AgentNotFound(_)));
    }

    #[tokio::test]
    async fn test_steer_non_running_agent_returns_not_running() {
        let (sup, _rx) = make_supervisor(4);
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::new(AgentStatus::Paused));

        let id = sup
            .spawn_agent(Arc::clone(&adapter), make_spawn_config())
            .await
            .unwrap();

        let msg = SteerMessage {
            message: "hello".into(),
            priority: SteerPriority::Urgent,
        };
        let err = sup.steer(&id, msg).await.unwrap_err();
        assert!(
            matches!(err, SupervisorError::AgentNotRunning(_)),
            "expected AgentNotRunning, got {err:?}"
        );
    }

    #[test]
    fn test_steer_message_serialization() {
        let msg = SteerMessage {
            message: "do the thing".into(),
            priority: SteerPriority::Urgent,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"message\":\"do the thing\""));
        assert!(json.contains("\"priority\":\"urgent\""));

        let roundtrip: SteerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.message, "do the thing");
        assert_eq!(roundtrip.priority, SteerPriority::Urgent);
    }

    #[test]
    fn test_steer_priority_default() {
        assert_eq!(SteerPriority::default(), SteerPriority::Normal);
    }
}
