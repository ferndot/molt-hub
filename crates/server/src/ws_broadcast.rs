//! Convenience helpers for broadcasting domain events over WebSocket.
//!
//! Each helper constructs a [`ServerMessage::Event`] with the appropriate topic
//! and serialised payload, then delegates to [`ConnectionManager::broadcast`].

use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use tracing::warn;

use crate::ws::{ConnectionManager, ServerMessage};

// ---------------------------------------------------------------------------
// Generic broadcast helper
// ---------------------------------------------------------------------------

/// Broadcast an arbitrary serialisable payload under `topic`.
fn broadcast_json<T: Serialize>(manager: &ConnectionManager, topic: &str, payload: &T) {
    match serde_json::to_value(payload) {
        Ok(value) => {
            manager.broadcast(
                topic,
                ServerMessage::Event {
                    topic: topic.to_owned(),
                    payload: value,
                },
            );
        }
        Err(e) => {
            warn!(topic, error = %e, "failed to serialise broadcast payload");
        }
    }
}

// ---------------------------------------------------------------------------
// Board events
// ---------------------------------------------------------------------------

/// Payload for a board task update pushed to `board:update`.
#[derive(Debug, Serialize)]
pub struct BoardUpdate {
    pub task_id: String,
    pub stage: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Broadcast a board task update to all clients subscribed to `board:*`.
pub fn broadcast_board_update(
    manager: &ConnectionManager,
    task_id: &str,
    stage: &str,
    status: &str,
) {
    let payload = BoardUpdate {
        task_id: task_id.to_owned(),
        stage: stage.to_owned(),
        status: status.to_owned(),
        priority: None,
        name: None,
        agent_name: None,
        summary: None,
    };
    broadcast_json(manager, "board:update", &payload);
}

/// Broadcast a full board update payload (allows setting all optional fields).
pub fn broadcast_board_update_full(manager: &ConnectionManager, payload: &BoardUpdate) {
    broadcast_json(manager, "board:update", payload);
}

// ---------------------------------------------------------------------------
// Triage events
// ---------------------------------------------------------------------------

/// Payload for a triage item pushed to `triage:new` or `triage:resolved`.
#[derive(Debug, Serialize)]
pub struct TriageItemPayload {
    pub id: String,
    pub task_id: String,
    pub task_name: String,
    pub agent_name: String,
    pub stage: String,
    pub priority: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub created_at: String,
    pub summary: String,
}

/// Broadcast a new triage item to clients subscribed to `triage:*`.
pub fn broadcast_triage_new(manager: &ConnectionManager, item: &TriageItemPayload) {
    broadcast_json(manager, "triage:new", item);
}

/// Broadcast a triage item resolution to clients subscribed to `triage:*`.
pub fn broadcast_triage_resolved(manager: &ConnectionManager, item_id: &str) {
    broadcast_json(manager, "triage:resolved", &serde_json::json!({ "id": item_id }));
}

// ---------------------------------------------------------------------------
// Health / metrics events
// ---------------------------------------------------------------------------

/// Payload for system metrics pushed to `health:metrics`.
#[derive(Debug, Serialize)]
pub struct MetricsPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_agent_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_usage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
}

/// Broadcast metrics data to clients subscribed to `health:metrics`.
pub fn broadcast_metrics(manager: &ConnectionManager, metrics: &MetricsPayload) {
    broadcast_json(manager, "health:metrics", metrics);
}

// ---------------------------------------------------------------------------
// Agent output events
// ---------------------------------------------------------------------------

/// Payload for an agent output line pushed to `agent:{id}`.
///
/// Wire format matches the spec:
/// ```json
/// {"type": "agent_output", "agent_id": "...", "line": "...", "timestamp": "..."}
/// ```
#[derive(Debug, Serialize)]
pub struct AgentOutputPayload {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub agent_id: String,
    pub line: String,
    pub timestamp: String,
}

/// Broadcast an agent output line to clients subscribed to `agent:{id}`.
///
/// Also writes to the provided [`AgentOutputBuffer`] so late-joining clients
/// can fetch recent output via REST.
pub fn broadcast_agent_output(manager: &ConnectionManager, agent_id: &str, line: &str) {
    let payload = AgentOutputPayload {
        msg_type: "agent_output".into(),
        agent_id: agent_id.to_owned(),
        line: line.to_owned(),
        timestamp: Utc::now().to_rfc3339(),
    };
    let topic = format!("agent:{agent_id}");
    broadcast_json(manager, &topic, &payload);
}

// ---------------------------------------------------------------------------
// Metrics update (with pending decisions)
// ---------------------------------------------------------------------------

/// Extended metrics payload including pending decision count.
#[derive(Debug, Serialize)]
pub struct MetricsUpdatePayload {
    #[serde(rename = "activeAgentCount")]
    pub active_agent_count: u32,
    #[serde(rename = "pendingDecisionCount")]
    pub pending_decision_count: u32,
}

/// Broadcast an agent lifecycle metrics update to `metrics:update`.
pub fn broadcast_metrics_update(manager: &ConnectionManager, payload: &MetricsUpdatePayload) {
    broadcast_json(manager, "metrics:update", payload);
}

// ---------------------------------------------------------------------------
// Settings events
// ---------------------------------------------------------------------------

/// Broadcast a settings change notification so other connected clients can
/// refresh their local copy.
pub fn broadcast_settings_changed(manager: &Arc<ConnectionManager>) {
    broadcast_json(
        manager,
        "settings:changed",
        &serde_json::json!({ "action": "updated" }),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::ConnectionId;
    use tokio::sync::mpsc;

    fn setup() -> (Arc<ConnectionManager>, mpsc::UnboundedReceiver<ServerMessage>) {
        let manager = Arc::new(ConnectionManager::new());
        let id = ConnectionId::new();
        let (tx, rx) = mpsc::unbounded_channel();
        manager.register(id, tx);
        manager.subscribe(id, "board:update");
        manager.subscribe(id, "triage:new");
        manager.subscribe(id, "triage:resolved");
        manager.subscribe(id, "health:metrics");
        manager.subscribe(id, "metrics:update");
        manager.subscribe(id, "settings:changed");
        (manager, rx)
    }

    fn setup_with_agent(agent_id: &str) -> (Arc<ConnectionManager>, mpsc::UnboundedReceiver<ServerMessage>) {
        let manager = Arc::new(ConnectionManager::new());
        let id = ConnectionId::new();
        let (tx, rx) = mpsc::unbounded_channel();
        manager.register(id, tx);
        manager.subscribe(id, &format!("agent:{agent_id}"));
        (manager, rx)
    }

    #[test]
    fn broadcast_board_update_sends_event() {
        let (manager, mut rx) = setup();
        broadcast_board_update(&manager, "task-1", "in-progress", "running");
        let msg = rx.try_recv().expect("should receive message");
        match msg {
            ServerMessage::Event { topic, payload } => {
                assert_eq!(topic, "board:update");
                assert_eq!(payload["task_id"], "task-1");
                assert_eq!(payload["stage"], "in-progress");
                assert_eq!(payload["status"], "running");
            }
            other => panic!("expected Event, got {:?}", other),
        }
    }

    #[test]
    fn broadcast_triage_new_sends_event() {
        let (manager, mut rx) = setup();
        let item = TriageItemPayload {
            id: "ti-1".into(),
            task_id: "t-1".into(),
            task_name: "Fix bug".into(),
            agent_name: "agent-a".into(),
            stage: "code-review".into(),
            priority: "p0".into(),
            item_type: "decision".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            summary: "Needs review".into(),
        };
        broadcast_triage_new(&manager, &item);
        let msg = rx.try_recv().expect("should receive message");
        match msg {
            ServerMessage::Event { topic, payload } => {
                assert_eq!(topic, "triage:new");
                assert_eq!(payload["id"], "ti-1");
            }
            other => panic!("expected Event, got {:?}", other),
        }
    }

    #[test]
    fn broadcast_metrics_sends_event() {
        let (manager, mut rx) = setup();
        let metrics = MetricsPayload {
            active_agent_count: Some(5),
            cpu_usage: Some(42.5),
            memory_bytes: None,
        };
        broadcast_metrics(&manager, &metrics);
        let msg = rx.try_recv().expect("should receive message");
        match msg {
            ServerMessage::Event { topic, payload } => {
                assert_eq!(topic, "health:metrics");
                assert_eq!(payload["active_agent_count"], 5);
                assert_eq!(payload["cpu_usage"], 42.5);
                assert!(payload.get("memory_bytes").is_none());
            }
            other => panic!("expected Event, got {:?}", other),
        }
    }

    #[test]
    fn broadcast_settings_changed_sends_event() {
        let (manager, mut rx) = setup();
        broadcast_settings_changed(&manager);
        let msg = rx.try_recv().expect("should receive message");
        match msg {
            ServerMessage::Event { topic, payload } => {
                assert_eq!(topic, "settings:changed");
                assert_eq!(payload["action"], "updated");
            }
            other => panic!("expected Event, got {:?}", other),
        }
    }

    #[test]
    fn broadcast_agent_output_sends_event() {
        let (manager, mut rx) = setup_with_agent("agent-42");
        broadcast_agent_output(&manager, "agent-42", "Running tests...");
        let msg = rx.try_recv().expect("should receive message");
        match msg {
            ServerMessage::Event { topic, payload } => {
                assert_eq!(topic, "agent:agent-42");
                assert_eq!(payload["type"], "agent_output");
                assert_eq!(payload["agent_id"], "agent-42");
                assert_eq!(payload["line"], "Running tests...");
                assert!(payload["timestamp"].is_string());
            }
            other => panic!("expected Event, got {:?}", other),
        }
    }

    #[test]
    fn broadcast_metrics_update_sends_event() {
        let (manager, mut rx) = setup();
        let payload = MetricsUpdatePayload {
            active_agent_count: 3,
            pending_decision_count: 2,
        };
        broadcast_metrics_update(&manager, &payload);
        let msg = rx.try_recv().expect("should receive message");
        match msg {
            ServerMessage::Event { topic, payload } => {
                assert_eq!(topic, "metrics:update");
                assert_eq!(payload["activeAgentCount"], 3);
                assert_eq!(payload["pendingDecisionCount"], 2);
            }
            other => panic!("expected Event, got {:?}", other),
        }
    }
}
