//! WebSocket — real-time event streaming to connected UI clients.
//!
//! A single multiplexed WebSocket connection supports multiple topic subscriptions.
//! Clients send [`ClientMessage`] frames to subscribe/unsubscribe; the server pushes
//! [`ServerMessage`] frames for matching events.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use ulid::Ulid;

// ---------------------------------------------------------------------------
// Protocol types
// ---------------------------------------------------------------------------

/// Messages sent from the client to the server.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Subscribe { topic: String },
    Unsubscribe { topic: String },
}

/// Messages sent from the server to the client.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Event {
        topic: String,
        payload: serde_json::Value,
    },
    Error {
        message: String,
    },
    Subscribed {
        topic: String,
    },
    Unsubscribed {
        topic: String,
    },
}

// ---------------------------------------------------------------------------
// ConnectionId
// ---------------------------------------------------------------------------

/// Opaque identifier for a single WebSocket connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(Ulid);

impl ConnectionId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// ConnectionManager
// ---------------------------------------------------------------------------

/// Thread-safe registry of active WebSocket connections and their topic subscriptions.
///
/// Two indexes are maintained in lock-step:
/// - `subscriptions`: connection → set of topics it listens to
/// - `topic_subscribers`: topic → set of connections listening to it
///
/// The "global" pseudo-topic receives every broadcast regardless of explicit subscription.
pub struct ConnectionManager {
    subscriptions: DashMap<ConnectionId, HashSet<String>>,
    topic_subscribers: DashMap<String, HashSet<ConnectionId>>,
    senders: DashMap<ConnectionId, mpsc::UnboundedSender<ServerMessage>>,
}

impl ConnectionManager {
    /// Create an empty [`ConnectionManager`].
    pub fn new() -> Self {
        Self {
            subscriptions: DashMap::new(),
            topic_subscribers: DashMap::new(),
            senders: DashMap::new(),
        }
    }

    /// Register a new connection and return its sender half.
    pub fn register(
        &self,
        id: ConnectionId,
        sender: mpsc::UnboundedSender<ServerMessage>,
    ) {
        self.subscriptions.insert(id, HashSet::new());
        self.senders.insert(id, sender);
        info!(conn = %id, "connection registered");
    }

    /// Remove a connection and all of its subscriptions.
    pub fn unregister(&self, id: ConnectionId) {
        if let Some((_, topics)) = self.subscriptions.remove(&id) {
            for topic in topics {
                if let Some(mut subscribers) = self.topic_subscribers.get_mut(&topic) {
                    subscribers.remove(&id);
                }
            }
        }
        self.senders.remove(&id);
        info!(conn = %id, "connection unregistered");
    }

    /// Subscribe a connection to a topic.
    pub fn subscribe(&self, id: ConnectionId, topic: &str) {
        self.subscriptions
            .entry(id)
            .or_default()
            .insert(topic.to_owned());
        self.topic_subscribers
            .entry(topic.to_owned())
            .or_default()
            .insert(id);
        debug!(conn = %id, topic, "subscribed");
    }

    /// Unsubscribe a connection from a topic.
    pub fn unsubscribe(&self, id: ConnectionId, topic: &str) {
        if let Some(mut topics) = self.subscriptions.get_mut(&id) {
            topics.remove(topic);
        }
        if let Some(mut subscribers) = self.topic_subscribers.get_mut(topic) {
            subscribers.remove(&id);
        }
        debug!(conn = %id, topic, "unsubscribed");
    }

    /// Broadcast a [`ServerMessage`] to all connections subscribed to `topic`
    /// and to all connections subscribed to the `"global"` pseudo-topic.
    pub fn broadcast(&self, topic: &str, message: ServerMessage) {
        let mut target_ids: HashSet<ConnectionId> = HashSet::new();

        // Collect explicit topic subscribers.
        if let Some(subscribers) = self.topic_subscribers.get(topic) {
            target_ids.extend(subscribers.iter().copied());
        }

        // Collect "global" subscribers.
        if let Some(global_subscribers) = self.topic_subscribers.get("global") {
            target_ids.extend(global_subscribers.iter().copied());
        }

        for id in target_ids {
            if let Some(sender) = self.senders.get(&id) {
                if let Err(e) = sender.send(message.clone()) {
                    warn!(conn = %id, error = %e, "failed to send to connection");
                }
            }
        }
    }

    /// Return the number of currently active connections.
    pub fn connection_count(&self) -> usize {
        self.senders.len()
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Axum WebSocket handler
// ---------------------------------------------------------------------------

/// Axum handler that upgrades an HTTP request to a WebSocket connection.
///
/// # Usage
/// ```ignore
/// let app = Router::new()
///     .route("/ws", get(ws_handler))
///     .with_state(Arc::new(ConnectionManager::new()));
/// ```
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(manager): State<Arc<ConnectionManager>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, manager))
}

async fn handle_socket(mut socket: WebSocket, manager: Arc<ConnectionManager>) {
    let id = ConnectionId::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();

    manager.register(id, tx);

    loop {
        tokio::select! {
            // Outbound: forward queued ServerMessages to the WebSocket.
            Some(msg) = rx.recv() => {
                match serde_json::to_string(&msg) {
                    Ok(text) => {
                        if let Err(e) = socket.send(Message::Text(text.into())).await {
                            debug!(conn = %id, error = %e, "ws write error — closing");
                            break;
                        }
                    }
                    Err(e) => {
                        error!(conn = %id, error = %e, "failed to serialise ServerMessage");
                    }
                }
            }

            // Inbound: parse ClientMessages and update subscription state.
            result = socket.recv() => {
                match result {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientMessage>(&text) {
                            Ok(ClientMessage::Subscribe { topic }) => {
                                manager.subscribe(id, &topic);
                                if let Some(sender) = manager.senders.get(&id) {
                                    let _ = sender.send(ServerMessage::Subscribed { topic });
                                }
                            }
                            Ok(ClientMessage::Unsubscribe { topic }) => {
                                manager.unsubscribe(id, &topic);
                                if let Some(sender) = manager.senders.get(&id) {
                                    let _ = sender.send(ServerMessage::Unsubscribed { topic });
                                }
                            }
                            Err(e) => {
                                warn!(conn = %id, error = %e, "invalid client message");
                                if let Some(sender) = manager.senders.get(&id) {
                                    let _ = sender.send(ServerMessage::Error {
                                        message: format!("invalid message: {e}"),
                                    });
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        debug!(conn = %id, "client sent close frame");
                        break;
                    }
                    Some(Ok(_)) => {
                        // Ignore binary / ping / pong frames.
                    }
                    Some(Err(e)) => {
                        debug!(conn = %id, error = %e, "ws receive error");
                        break;
                    }
                    None => {
                        debug!(conn = %id, "ws stream ended");
                        break;
                    }
                }
            }
        }
    }

    manager.unregister(id);
}
