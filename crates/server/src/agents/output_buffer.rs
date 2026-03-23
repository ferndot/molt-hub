//! In-memory ring buffer for agent output lines.
//!
//! Stores the last N output lines per agent so that late-joining clients can
//! fetch recent output via the REST API instead of relying solely on
//! WebSocket streaming.

use std::collections::VecDeque;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

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
