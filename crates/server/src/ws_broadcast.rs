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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_id: Option<String>,
}

/// Broadcast a board task update to all clients subscribed to `project:{pid}:board:update`.
pub fn broadcast_board_update(
    manager: &ConnectionManager,
    project_id: &str,
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
        board_id: None,
    };
    let topic = format!("project:{project_id}:board:update");
    broadcast_json(manager, &topic, &payload);
}

/// Broadcast a full board update payload (allows setting all optional fields).
pub fn broadcast_board_update_full(
    manager: &ConnectionManager,
    project_id: &str,
    payload: &BoardUpdate,
) {
    let topic = format!("project:{project_id}:board:update");
    broadcast_json(manager, &topic, payload);
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

/// Broadcast a new triage item to clients subscribed to `project:{pid}:triage:new`.
pub fn broadcast_triage_new(
    manager: &ConnectionManager,
    project_id: &str,
    item: &TriageItemPayload,
) {
    let topic = format!("project:{project_id}:triage:new");
    broadcast_json(manager, &topic, item);
}

/// Broadcast a triage item resolution to clients subscribed to `project:{pid}:triage:resolved`.
pub fn broadcast_triage_resolved(manager: &ConnectionManager, project_id: &str, item_id: &str) {
    let topic = format!("project:{project_id}:triage:resolved");
    broadcast_json(manager, &topic, &serde_json::json!({ "id": item_id }));
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
// Agent steering events
// ---------------------------------------------------------------------------

/// Payload for an agent steering notification pushed to `agent:{id}`.
///
/// Wire format:
/// ```json
/// {"type": "agent_steered", "agent_id": "...", "message": "...", "timestamp": "..."}
/// ```
#[derive(Debug, Serialize)]
pub struct AgentSteeredPayload {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub agent_id: String,
    pub message: String,
    pub timestamp: String,
}

/// Broadcast an agent steering event to clients subscribed to `agent:{id}`.
pub fn broadcast_agent_steered(manager: &ConnectionManager, agent_id: &str, message: &str) {
    let payload = AgentSteeredPayload {
        msg_type: "agent_steered".into(),
        agent_id: agent_id.to_owned(),
        message: message.to_owned(),
        timestamp: Utc::now().to_rfc3339(),
    };
    let topic = format!("agent:{agent_id}");
    broadcast_json(manager, &topic, &payload);
}

// ---------------------------------------------------------------------------
// Agent error events
// ---------------------------------------------------------------------------

/// Payload for an agent error pushed to `agent:{id}`.
///
/// Wire format:
/// ```json
/// {"type": "agent_error", "agent_id": "...", "message": "...", "auth_required": true, "timestamp": "..."}
/// ```
#[derive(Debug, Serialize)]
pub struct AgentErrorPayload {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub agent_id: String,
    pub message: String,
    pub auth_required: bool,
    pub timestamp: String,
}

/// Broadcast an agent error event to clients subscribed to `agent:{id}`.
pub fn broadcast_agent_error(
    manager: &ConnectionManager,
    agent_id: &str,
    message: &str,
    auth_required: bool,
) {
    let payload = AgentErrorPayload {
        msg_type: "agent_error".into(),
        agent_id: agent_id.to_owned(),
        message: message.to_owned(),
        auth_required,
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
// Hook fired events
// ---------------------------------------------------------------------------

/// Payload for a hook_fired event pushed to `project:{pid}:hooks`.
#[derive(Debug, Serialize)]
pub struct HookFiredPayload {
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub task_id: String,
    pub stage: String,
    /// One of: "enter", "exit", "on_stall"
    pub trigger: String,
    /// One of: "agent_dispatch", "shell", "webhook", "start_dev_environment", "teardown_dev_environment"
    pub hook_kind: String,
}

/// Broadcast a hook_fired event to clients subscribed to `project:{pid}:hooks`.
pub fn broadcast_hook_fired(
    manager: &ConnectionManager,
    project_id: &str,
    task_id: &str,
    stage: &str,
    trigger: &str,
    hook_kind: &str,
) {
    let payload = HookFiredPayload {
        msg_type: "hook_fired",
        task_id: task_id.to_owned(),
        stage: stage.to_owned(),
        trigger: trigger.to_owned(),
        hook_kind: hook_kind.to_owned(),
    };
    let topic = format!("project:{project_id}:hooks");
    broadcast_json(manager, &topic, &payload);
}

// ---------------------------------------------------------------------------
// Agent tool call / result events
// ---------------------------------------------------------------------------

/// Payload for a tool call initiated by an agent, pushed to `agent:{id}`.
#[derive(Debug, Serialize)]
pub struct AgentToolCallPayload {
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub agent_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub timestamp: String,
}

/// Broadcast a tool_call event to clients subscribed to `agent:{id}`.
pub fn broadcast_tool_call(
    manager: &ConnectionManager,
    agent_id: &str,
    call_id: &str,
    tool_name: &str,
    input: serde_json::Value,
    timestamp: &str,
) {
    let payload = AgentToolCallPayload {
        msg_type: "tool_call",
        agent_id: agent_id.to_owned(),
        call_id: call_id.to_owned(),
        tool_name: tool_name.to_owned(),
        input,
        timestamp: timestamp.to_owned(),
    };
    let topic = format!("agent:{agent_id}");
    broadcast_json(manager, &topic, &payload);
}

/// Payload for the result of a tool call, pushed to `agent:{id}`.
#[derive(Debug, Serialize)]
pub struct AgentToolResultPayload {
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub agent_id: String,
    pub call_id: String,
    pub output: serde_json::Value,
    pub is_error: bool,
    pub timestamp: String,
}

/// Broadcast a tool_result event to clients subscribed to `agent:{id}`.
pub fn broadcast_tool_result(
    manager: &ConnectionManager,
    agent_id: &str,
    call_id: &str,
    output: serde_json::Value,
    is_error: bool,
    timestamp: &str,
) {
    let payload = AgentToolResultPayload {
        msg_type: "tool_result",
        agent_id: agent_id.to_owned(),
        call_id: call_id.to_owned(),
        output,
        is_error,
        timestamp: timestamp.to_owned(),
    };
    let topic = format!("agent:{agent_id}");
    broadcast_json(manager, &topic, &payload);
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

    fn setup() -> (
        Arc<ConnectionManager>,
        mpsc::UnboundedReceiver<ServerMessage>,
    ) {
        let manager = Arc::new(ConnectionManager::new());
        let id = ConnectionId::new();
        let (tx, rx) = mpsc::unbounded_channel();
        manager.register(id, tx);
        manager.subscribe(id, "project:default:board:update");
        manager.subscribe(id, "project:default:triage:new");
        manager.subscribe(id, "project:default:triage:resolved");
        manager.subscribe(id, "health:metrics");
        manager.subscribe(id, "metrics:update");
        manager.subscribe(id, "settings:changed");
        (manager, rx)
    }

    fn setup_with_agent(
        agent_id: &str,
    ) -> (
        Arc<ConnectionManager>,
        mpsc::UnboundedReceiver<ServerMessage>,
    ) {
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
        broadcast_board_update(&manager, "default", "task-1", "in-progress", "running");
        let msg = rx.try_recv().expect("should receive message");
        match msg {
            ServerMessage::Event { topic, payload } => {
                assert_eq!(topic, "project:default:board:update");
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
        broadcast_triage_new(&manager, "default", &item);
        let msg = rx.try_recv().expect("should receive message");
        match msg {
            ServerMessage::Event { topic, payload } => {
                assert_eq!(topic, "project:default:triage:new");
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
    fn broadcast_agent_steered_sends_event() {
        let (manager, mut rx) = setup_with_agent("agent-99");
        broadcast_agent_steered(&manager, "agent-99", "focus on error handling");
        let msg = rx.try_recv().expect("should receive message");
        match msg {
            ServerMessage::Event { topic, payload } => {
                assert_eq!(topic, "agent:agent-99");
                assert_eq!(payload["type"], "agent_steered");
                assert_eq!(payload["agent_id"], "agent-99");
                assert_eq!(payload["message"], "focus on error handling");
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
