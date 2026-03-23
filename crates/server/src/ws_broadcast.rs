//! Convenience helpers for broadcasting domain events over WebSocket.
//!
//! Each helper constructs a [`ServerMessage::Event`] with the appropriate topic
//! and serialised payload, then delegates to [`ConnectionManager::broadcast`].

use std::sync::Arc;

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
        manager.subscribe(id, "settings:changed");
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
}
