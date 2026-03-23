//! Audit writer — isolated background task for non-blocking audit logging.
//!
//! # Architecture
//!
//! ```text
//! [caller]  --log()-->  [mpsc tx]  -->  [background task]  -->  [VecDeque / future store]
//! ```
//!
//! The background task is the sole writer; callers never block.  The channel
//! is bounded so callers experience backpressure if the writer is overwhelmed.
//!
//! Entries are also stored in a shared `VecDeque` (capped at 1000) so they
//! can be queried via the audit HTTP endpoint.

use std::collections::VecDeque;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info};

// ---------------------------------------------------------------------------
// AuditAction
// ---------------------------------------------------------------------------

/// The kind of operation being audited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// An agent process was spawned.
    Spawn,
    /// A message was sent to an agent.
    Send,
    /// An agent process was terminated.
    Terminate,
    /// An issue was imported from an external system.
    Import,
}

impl AuditAction {
    /// Parse from a string (case-insensitive, accepts both snake_case and PascalCase).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "spawn" => Some(Self::Spawn),
            "send" => Some(Self::Send),
            "terminate" => Some(Self::Terminate),
            "import" => Some(Self::Import),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// AuditEntry
// ---------------------------------------------------------------------------

/// A single audit log record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Wall-clock time the event was logged.
    pub timestamp: DateTime<Utc>,
    /// What happened.
    pub action: AuditAction,
    /// The actor that performed the action (agent ID, user, system component, …).
    pub actor_id: String,
    /// Free-form JSON details specific to the action.
    pub details: Value,
}

impl AuditEntry {
    /// Convenience constructor that stamps `timestamp` automatically.
    pub fn now(action: AuditAction, actor_id: impl Into<String>, details: Value) -> Self {
        Self {
            timestamp: Utc::now(),
            action,
            actor_id: actor_id.into(),
            details,
        }
    }
}

// ---------------------------------------------------------------------------
// Shared entry storage
// ---------------------------------------------------------------------------

/// Maximum number of audit entries kept in memory.
const MAX_ENTRIES: usize = 1000;

/// Shared, bounded ring buffer of audit entries.
pub type SharedEntries = Arc<RwLock<VecDeque<AuditEntry>>>;

fn new_shared_entries() -> SharedEntries {
    Arc::new(RwLock::new(VecDeque::with_capacity(MAX_ENTRIES)))
}

// ---------------------------------------------------------------------------
// AuditWriter — the background task
// ---------------------------------------------------------------------------

/// Background task that drains the audit channel and persists entries.
///
/// In this implementation entries are written to tracing (INFO level) and
/// accumulated in the shared ring buffer.
struct AuditWriter {
    rx: mpsc::Receiver<AuditEntry>,
    entries: SharedEntries,
}

impl AuditWriter {
    fn new(rx: mpsc::Receiver<AuditEntry>, entries: SharedEntries) -> Self {
        Self { rx, entries }
    }

    async fn run(mut self) {
        info!("audit writer started");

        while let Some(entry) = self.rx.recv().await {
            let json = serde_json::to_string(&entry).unwrap_or_else(|e| {
                error!(error = %e, "failed to serialise audit entry");
                String::new()
            });
            info!(audit = %json, "AUDIT");

            let mut guard = self.entries.write().await;
            if guard.len() >= MAX_ENTRIES {
                guard.pop_front();
            }
            guard.push_back(entry);
        }

        let count = self.entries.read().await.len();
        info!(count, "audit writer stopped");
    }
}

// ---------------------------------------------------------------------------
// AuditHandle — caller-side handle
// ---------------------------------------------------------------------------

/// A cloneable, non-blocking handle for submitting audit log entries.
///
/// Obtained from [`start_audit_writer`].  Cloning the handle shares the same
/// underlying channel and entry storage.
#[derive(Clone)]
pub struct AuditHandle {
    tx: mpsc::Sender<AuditEntry>,
    entries: SharedEntries,
}

impl AuditHandle {
    /// Log an audit entry.
    ///
    /// This is a non-blocking, best-effort send.  If the channel is full the
    /// entry is silently dropped (logged at WARN level).  Callers must not
    /// treat audit failures as fatal.
    pub fn log(&self, entry: AuditEntry) {
        match self.tx.try_send(entry) {
            Ok(_) => {}
            Err(mpsc::error::TrySendError::Full(e)) => {
                tracing::warn!(action = ?e.action, "audit channel full; dropping entry");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!("audit channel closed; dropping entry");
            }
        }
    }

    /// Convenience: log a Spawn action.
    pub fn log_spawn(&self, actor_id: impl Into<String>, details: Value) {
        self.log(AuditEntry::now(AuditAction::Spawn, actor_id, details));
    }

    /// Convenience: log a Send action.
    pub fn log_send(&self, actor_id: impl Into<String>, details: Value) {
        self.log(AuditEntry::now(AuditAction::Send, actor_id, details));
    }

    /// Convenience: log a Terminate action.
    pub fn log_terminate(&self, actor_id: impl Into<String>, details: Value) {
        self.log(AuditEntry::now(AuditAction::Terminate, actor_id, details));
    }

    /// Convenience: log an Import action.
    pub fn log_import(&self, actor_id: impl Into<String>, details: Value) {
        self.log(AuditEntry::now(AuditAction::Import, actor_id, details));
    }

    /// Return the most recent audit entries, optionally filtered by action.
    ///
    /// Entries are returned in reverse chronological order (newest first).
    pub async fn recent(
        &self,
        limit: usize,
        action_filter: Option<AuditAction>,
    ) -> Vec<AuditEntry> {
        let guard = self.entries.read().await;
        guard
            .iter()
            .rev()
            .filter(|e| {
                action_filter
                    .as_ref()
                    .map_or(true, |a| e.action == *a)
            })
            .take(limit)
            .cloned()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Public constructor
// ---------------------------------------------------------------------------

/// Capacity of the bounded audit channel.
const CHANNEL_CAPACITY: usize = 512;

/// Spawn the audit writer background task and return a handle.
///
/// The background task runs until all [`AuditHandle`] clones are dropped (the
/// sender side closes), at which point it drains remaining entries and exits.
pub fn start_audit_writer() -> AuditHandle {
    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
    let entries = new_shared_entries();
    let writer = AuditWriter::new(rx, Arc::clone(&entries));
    tokio::spawn(writer.run());
    AuditHandle { tx, entries }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    /// Helper: create a handle backed by a direct receiver (no tokio task).
    fn direct_channel(capacity: usize) -> (AuditHandle, mpsc::Receiver<AuditEntry>) {
        let (tx, rx) = mpsc::channel(capacity);
        let entries = new_shared_entries();
        (AuditHandle { tx, entries }, rx)
    }

    #[test]
    fn audit_entry_now_stamps_timestamp() {
        let entry = AuditEntry::now(AuditAction::Spawn, "agent-1", serde_json::json!({}));
        assert_eq!(entry.action, AuditAction::Spawn);
        assert_eq!(entry.actor_id, "agent-1");
        // Timestamp should be very recent.
        let delta = Utc::now() - entry.timestamp;
        assert!(delta.num_seconds() < 2);
    }

    #[tokio::test]
    async fn handle_log_sends_entry_to_channel() {
        let (handle, mut rx) = direct_channel(4);
        handle.log_spawn("agent-1", serde_json::json!({ "pid": 42 }));

        let entry = rx.recv().await.expect("should receive entry");
        assert_eq!(entry.action, AuditAction::Spawn);
        assert_eq!(entry.actor_id, "agent-1");
        assert_eq!(entry.details["pid"], 42);
    }

    #[tokio::test]
    async fn handle_log_import_sends_import_action() {
        let (handle, mut rx) = direct_channel(4);
        handle.log_import("system", serde_json::json!({ "jira_key": "PROJ-1" }));

        let entry = rx.recv().await.expect("should receive entry");
        assert_eq!(entry.action, AuditAction::Import);
        assert_eq!(entry.details["jira_key"], "PROJ-1");
    }

    #[tokio::test]
    async fn handle_log_drops_when_channel_full() {
        // Channel capacity = 0 — every send should fail gracefully.
        let (tx, _rx) = mpsc::channel::<AuditEntry>(1);
        let handle = AuditHandle {
            tx,
            entries: new_shared_entries(),
        };

        // Fill it up
        handle.log_spawn("a", serde_json::json!({}));
        // This one should be silently dropped (full).
        handle.log_spawn("b", serde_json::json!({}));
        // No panic — test passes.
    }

    #[tokio::test]
    async fn audit_writer_drains_all_entries() {
        let (tx, rx) = mpsc::channel(8);
        let entries = new_shared_entries();
        let writer = AuditWriter::new(rx, Arc::clone(&entries));
        let handle = tokio::spawn(writer.run());

        // Send 3 entries then drop sender so the writer stops.
        for i in 0..3u32 {
            tx.send(AuditEntry::now(
                AuditAction::Send,
                format!("agent-{i}"),
                serde_json::json!({ "seq": i }),
            ))
            .await
            .unwrap();
        }
        drop(tx);

        // Writer task should exit cleanly.
        handle.await.expect("writer task panicked");

        // Shared entries should have all 3.
        assert_eq!(entries.read().await.len(), 3);
    }

    #[test]
    fn audit_action_serialises_snake_case() {
        let json = serde_json::to_string(&AuditAction::Import).unwrap();
        assert_eq!(json, r#""import""#);

        let json = serde_json::to_string(&AuditAction::Spawn).unwrap();
        assert_eq!(json, r#""spawn""#);
    }

    #[test]
    fn audit_entry_serialises_to_json() {
        let entry = AuditEntry {
            timestamp: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            action: AuditAction::Terminate,
            actor_id: "agent-x".into(),
            details: serde_json::json!({ "reason": "timeout" }),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("terminate"));
        assert!(json.contains("agent-x"));
        assert!(json.contains("timeout"));
    }

    #[tokio::test]
    async fn start_audit_writer_returns_working_handle() {
        let handle = start_audit_writer();
        // Should not panic — just log and continue.
        handle.log_spawn("test-agent", serde_json::json!({ "test": true }));
        // Give the background task a moment.
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // ── recent() tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn recent_returns_entries_newest_first() {
        let handle = start_audit_writer();
        handle.log_spawn("a1", serde_json::json!({"seq": 1}));
        handle.log_send("a2", serde_json::json!({"seq": 2}));
        handle.log_terminate("a3", serde_json::json!({"seq": 3}));

        // Give writer time to drain
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let entries = handle.recent(10, None).await;
        assert_eq!(entries.len(), 3);
        // Newest first
        assert_eq!(entries[0].actor_id, "a3");
        assert_eq!(entries[2].actor_id, "a1");
    }

    #[tokio::test]
    async fn recent_filters_by_action() {
        let handle = start_audit_writer();
        handle.log_spawn("s1", serde_json::json!({}));
        handle.log_send("d1", serde_json::json!({}));
        handle.log_spawn("s2", serde_json::json!({}));

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let spawns = handle.recent(100, Some(AuditAction::Spawn)).await;
        assert_eq!(spawns.len(), 2);
        for e in &spawns {
            assert_eq!(e.action, AuditAction::Spawn);
        }
    }

    #[tokio::test]
    async fn recent_respects_limit() {
        let handle = start_audit_writer();
        for i in 0..10 {
            handle.log_spawn(format!("a{i}"), serde_json::json!({}));
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let entries = handle.recent(3, None).await;
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn audit_action_from_str_loose() {
        assert_eq!(AuditAction::from_str_loose("Spawn"), Some(AuditAction::Spawn));
        assert_eq!(AuditAction::from_str_loose("send"), Some(AuditAction::Send));
        assert_eq!(AuditAction::from_str_loose("TERMINATE"), Some(AuditAction::Terminate));
        assert_eq!(AuditAction::from_str_loose("import"), Some(AuditAction::Import));
        assert_eq!(AuditAction::from_str_loose("unknown"), None);
    }
}
