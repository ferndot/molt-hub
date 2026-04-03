//! AgentAdapter trait — common interface for all agent backends.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::broadcast::error::RecvError;

use molt_hub_core::model::{AgentId, AgentStatus, SessionId, TaskId};

// ---------------------------------------------------------------------------
// SpawnConfig
// ---------------------------------------------------------------------------

/// Configuration passed to an adapter when spawning a new agent process.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    pub agent_id: AgentId,
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub working_dir: PathBuf,
    pub instructions: String,
    pub env: HashMap<String, String>,
    pub timeout: Option<Duration>,
    pub adapter_config: serde_json::Value,
    /// Optional project this agent belongs to. `None` means the global / default context.
    pub project_id: Option<String>,
    /// Global event broadcast channel. When provided, agent events are sent here
    /// so that the WS fanout layer can relay them to clients.
    pub event_tx: Option<tokio::sync::broadcast::Sender<AgentEvent>>,
}

// ---------------------------------------------------------------------------
// AgentHandle
// ---------------------------------------------------------------------------

/// An opaque handle returned by `AgentAdapter::spawn`.
///
/// Adapters may store their own internal state in `internal`; callers can
/// retrieve it via `downcast_internal`.
pub struct AgentHandle {
    pub agent_id: AgentId,
    pub pid: Option<u32>,
    internal: Box<dyn std::any::Any + Send + Sync>,
}

impl AgentHandle {
    /// Construct a new handle.  `internal` is adapter-specific state.
    pub fn new(
        agent_id: AgentId,
        pid: Option<u32>,
        internal: Box<dyn std::any::Any + Send + Sync>,
    ) -> Self {
        Self {
            agent_id,
            pid,
            internal,
        }
    }

    /// Returns the agent ID associated with this handle.
    pub fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    /// Returns the OS process ID, if applicable.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Attempt to downcast the internal state to a concrete type.
    pub fn downcast_internal<T: std::any::Any>(&self) -> Option<&T> {
        self.internal.downcast_ref::<T>()
    }
}

// ---------------------------------------------------------------------------
// AgentMessage
// ---------------------------------------------------------------------------

/// Messages that can be sent to a running agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    /// Send a free-form instruction string to the agent.
    Instruction(String),
    /// Ask the agent to pause execution.
    Pause,
    /// Resume a previously paused agent.
    Resume,
    /// Send structured data to the agent.
    Data(serde_json::Value),
}

// ---------------------------------------------------------------------------
// AgentEvent
// ---------------------------------------------------------------------------

/// Events emitted by a running agent and consumed by the supervisor layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// A line / chunk of text output from the agent.
    Output {
        agent_id: AgentId,
        content: String,
        timestamp: DateTime<Utc>,
    },
    /// The agent transitioned to a new status.
    StatusChanged {
        agent_id: AgentId,
        previous: AgentStatus,
        current: AgentStatus,
        timestamp: DateTime<Utc>,
    },
    /// The agent finished successfully.
    Completed {
        agent_id: AgentId,
        exit_code: Option<i32>,
        timestamp: DateTime<Utc>,
    },
    /// The agent encountered an error.
    Error {
        agent_id: AgentId,
        message: String,
        timestamp: DateTime<Utc>,
    },
    /// A progress update from the agent (0–100).
    Progress {
        agent_id: AgentId,
        percent: u8,
        message: Option<String>,
        timestamp: DateTime<Utc>,
    },
    /// Signals that the agent finished one prompt/turn (output for this turn is complete).
    TurnEnd {
        agent_id: AgentId,
        timestamp: DateTime<Utc>,
    },
    /// The agent initiated a tool call.
    ToolCall {
        agent_id: AgentId,
        call_id: String,
        tool_name: String,
        input: serde_json::Value,
        timestamp: DateTime<Utc>,
    },
    /// A tool call reached a terminal state (completed or failed).
    ToolResult {
        agent_id: AgentId,
        call_id: String,
        output: serde_json::Value,
        is_error: bool,
        timestamp: DateTime<Utc>,
    },
}

// ---------------------------------------------------------------------------
// AdapterError
// ---------------------------------------------------------------------------

/// Errors returned by `AgentAdapter` operations.
#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("failed to spawn agent: {0}")]
    SpawnFailed(String),

    #[error("agent not found")]
    AgentNotFound,

    #[error("agent is already terminated")]
    AlreadyTerminated,

    #[error("failed to send message to agent: {0}")]
    SendFailed(String),

    #[error("operation timed out")]
    Timeout,

    #[error("internal adapter error: {0}")]
    Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}

// ---------------------------------------------------------------------------
// AgentAdapter
// ---------------------------------------------------------------------------

/// Trait implemented by every agent backend (Claude CLI, local shell, …).
///
/// All methods are `async` and the trait is object-safe via `async_trait`.
#[async_trait]
pub trait AgentAdapter: Send + Sync + 'static {
    /// Spawn a new agent according to `config` and return an opaque handle.
    async fn spawn(&self, config: SpawnConfig) -> Result<AgentHandle, AdapterError>;

    /// Send a message to a running agent.
    async fn send(&self, handle: &AgentHandle, message: AgentMessage) -> Result<(), AdapterError>;

    /// Query the current status of an agent.
    async fn status(&self, handle: &AgentHandle) -> Result<AgentStatus, AdapterError>;

    /// Request a graceful termination of the agent.
    async fn terminate(&self, handle: &AgentHandle) -> Result<(), AdapterError>;

    /// Forcibly abort the agent without waiting for cleanup.
    async fn abort(&self, handle: &AgentHandle) -> Result<(), AdapterError>;

    /// A short identifier for this adapter implementation (e.g. `"claude-cli"`).
    fn adapter_type(&self) -> &str;
}

// ---------------------------------------------------------------------------
// One-shot output collection (print / stdin-once flows)
// ---------------------------------------------------------------------------

fn tail_for_error_message(acc: &str, max_chars: usize) -> String {
    let t = acc.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.chars().count() <= max_chars {
        return t.to_string();
    }
    let skip = t.chars().count().saturating_sub(max_chars);
    format!("…{}", t.chars().skip(skip).collect::<String>())
}

/// Drain [`AgentEvent`]s from a broadcast receiver until this `agent_id` completes or errors.
///
/// Used by adapters that spawn a subprocess once, subscribe before the reader task runs,
/// and need the concatenated [`AgentEvent::Output`] text.
pub async fn collect_agent_print_output(
    rx: &mut tokio::sync::broadcast::Receiver<AgentEvent>,
    agent_id: &AgentId,
) -> Result<String, AdapterError> {
    let mut acc = String::new();
    loop {
        match rx.recv().await {
            Ok(AgentEvent::Output {
                agent_id: id,
                content,
                ..
            }) if id == *agent_id => {
                acc.push_str(&content);
            }
            Ok(AgentEvent::Completed {
                agent_id: id,
                exit_code,
                ..
            }) if id == *agent_id => {
                if exit_code == Some(0) {
                    return Ok(acc);
                }
                // Claude often reports failures on stdout (stream-json); stderr may be empty.
                let tail = tail_for_error_message(&acc, 3500);
                let detail = if tail.is_empty() {
                    String::new()
                } else {
                    format!(" — captured output: {tail}")
                };
                return Err(AdapterError::SpawnFailed(format!(
                    "process exited with code {:?}{detail}",
                    exit_code
                )));
            }
            Ok(AgentEvent::Error {
                agent_id: id,
                message,
                ..
            }) if id == *agent_id => {
                return Err(AdapterError::SpawnFailed(message));
            }
            Ok(_) => {}
            Err(RecvError::Closed) => {
                return Err(AdapterError::SpawnFailed(
                    "event channel closed before completion".into(),
                ));
            }
            Err(RecvError::Lagged(_)) => {}
        }
    }
}
