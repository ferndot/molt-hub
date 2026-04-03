//! In-memory ring buffer for agent output lines.
//!
//! Stores the last N output lines per agent so that late-joining clients can
//! fetch recent output via the REST API instead of relying solely on
//! WebSocket streaming.

use std::collections::VecDeque;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use molt_hub_harness::adapter::AgentEvent;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::debug;

use super::history_store;

/// Default capacity per agent (number of lines retained).
const DEFAULT_CAPACITY: usize = 500;

/// A single buffered output line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputLine {
    pub line: String,
    pub timestamp: DateTime<Utc>,
}

/// Thread-safe output buffer for all agents.
pub struct AgentOutputBuffer {
    /// Per-agent ring buffers, keyed by agent ID string.
    buffers: DashMap<String, VecDeque<OutputLine>>,
    /// Maximum lines kept per agent.
    capacity: usize,
}

impl AgentOutputBuffer {
    /// Create a buffer with the default capacity (500 lines per agent).
    pub fn new() -> Self {
        Self {
            buffers: DashMap::new(),
            capacity: DEFAULT_CAPACITY,
        }
    }

    /// Create a buffer with a custom capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffers: DashMap::new(),
            capacity,
        }
    }

    /// Append an output line for an agent.
    pub fn push(&self, agent_id: &str, line: String) {
        let mut entry = self.buffers.entry(agent_id.to_owned()).or_default();
        if entry.len() >= self.capacity {
            entry.pop_front();
        }
        entry.push_back(OutputLine {
            line,
            timestamp: Utc::now(),
        });
    }

    /// Retrieve all buffered lines for an agent (oldest first).
    pub fn get_lines(&self, agent_id: &str) -> Vec<OutputLine> {
        self.buffers
            .get(agent_id)
            .map(|buf| buf.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Remove all buffered output for an agent (e.g. after termination).
    pub fn clear_agent(&self, agent_id: &str) {
        self.buffers.remove(agent_id);
    }

    /// Return the number of agents with buffered output.
    pub fn agent_count(&self) -> usize {
        self.buffers.len()
    }
}

impl Default for AgentOutputBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a shared output buffer wrapped in `Arc`.
pub fn shared_output_buffer() -> Arc<AgentOutputBuffer> {
    Arc::new(AgentOutputBuffer::new())
}

/// Subscribe to supervisor/agent [`AgentEvent`] stream and append `Output` lines to `buffer`.
///
/// When `pool` is `Some`, each completed line is also persisted to the `agent_output` table
/// via a fire-and-forget `tokio::spawn` so the write never blocks the streaming path.
///
/// Late subscribers miss prior messages; this task only needs live `Output` events for REST
/// `GET /api/agents/:id/output`.
pub fn spawn_agent_output_buffer_task(
    mut rx: broadcast::Receiver<AgentEvent>,
    buffer: Arc<AgentOutputBuffer>,
    pool: Option<sqlx::SqlitePool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut partial: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        loop {
            match rx.recv().await {
                Ok(AgentEvent::Output { agent_id, content, .. }) => {
                    let id = agent_id.to_string();
                    if content.is_empty() {
                        continue;
                    }
                    let buf = partial.entry(id.clone()).or_default();
                    buf.push_str(&content);
                    while let Some(nl) = buf.find('\n') {
                        let line = buf[..nl].to_string();
                        *buf = buf[nl + 1..].to_string();
                        if !line.is_empty() {
                            let ts = Utc::now();
                            buffer.push(&id, line.clone());
                            if let Some(ref p) = pool {
                                let p = p.clone();
                                let id2 = id.clone();
                                let line2 = line.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = history_store::insert_output_line(
                                        &p, &id2, None, "default", &line2, ts,
                                    )
                                    .await
                                    {
                                        debug!(error = %e, "failed to persist output line");
                                    }
                                });
                            }
                        }
                    }
                }
                Ok(AgentEvent::TurnEnd { ref agent_id, .. }) => {
                    let id = agent_id.to_string();
                    if let Some(buf) = partial.remove(&id) {
                        let trimmed = buf.trim_end_matches('\r').to_string();
                        if !trimmed.is_empty() {
                            let ts = Utc::now();
                            buffer.push(&id, trimmed.clone());
                            if let Some(ref p) = pool {
                                let p = p.clone();
                                let id2 = id.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = history_store::insert_output_line(
                                        &p, &id2, None, "default", &trimmed, ts,
                                    )
                                    .await
                                    {
                                        debug!(error = %e, "failed to persist output line (turn end)");
                                    }
                                });
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    debug!(
                        skipped,
                        "agent output buffer subscriber lagged; dropped events"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_get_lines() {
        let buf = AgentOutputBuffer::new();
        buf.push("agent-1", "line one".into());
        buf.push("agent-1", "line two".into());

        let lines = buf.get_lines("agent-1");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line, "line one");
        assert_eq!(lines[1].line, "line two");
    }

    #[test]
    fn get_lines_empty_agent() {
        let buf = AgentOutputBuffer::new();
        let lines = buf.get_lines("nonexistent");
        assert!(lines.is_empty());
    }

    #[test]
    fn capacity_eviction() {
        let buf = AgentOutputBuffer::with_capacity(3);
        buf.push("a", "1".into());
        buf.push("a", "2".into());
        buf.push("a", "3".into());
        buf.push("a", "4".into());

        let lines = buf.get_lines("a");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].line, "2");
        assert_eq!(lines[1].line, "3");
        assert_eq!(lines[2].line, "4");
    }

    #[test]
    fn clear_agent() {
        let buf = AgentOutputBuffer::new();
        buf.push("a", "data".into());
        assert_eq!(buf.agent_count(), 1);

        buf.clear_agent("a");
        assert_eq!(buf.agent_count(), 0);
        assert!(buf.get_lines("a").is_empty());
    }

    #[test]
    fn multiple_agents_isolated() {
        let buf = AgentOutputBuffer::new();
        buf.push("a", "alpha".into());
        buf.push("b", "beta".into());

        assert_eq!(buf.get_lines("a").len(), 1);
        assert_eq!(buf.get_lines("b").len(), 1);
        assert_eq!(buf.get_lines("a")[0].line, "alpha");
        assert_eq!(buf.get_lines("b")[0].line, "beta");
    }

    #[test]
    fn output_line_has_timestamp() {
        let buf = AgentOutputBuffer::new();
        buf.push("a", "hello".into());
        let lines = buf.get_lines("a");
        assert!(lines[0].timestamp <= Utc::now());
    }
}
