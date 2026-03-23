//! Notification routing — classifies events and stores them by attention tier.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use ulid::Ulid;

use molt_hub_core::events::types::DomainEvent;

use crate::attention::classifier::InterruptClassifier;
use crate::attention::priority::{attention_tier, AttentionCategory, InterruptLevel};

// ---------------------------------------------------------------------------
// Notification
// ---------------------------------------------------------------------------

/// A classified notification derived from a domain event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Unique identifier for this notification.
    pub id: Ulid,
    /// The interrupt level assigned by the classifier.
    pub interrupt_level: InterruptLevel,
    /// The attention tier (derived from interrupt level).
    pub attention_category: AttentionCategory,
    /// Short human-readable summary of the event.
    pub summary: String,
    /// When the notification was created.
    pub created_at: DateTime<Utc>,
    /// Whether a human has acknowledged this notification.
    pub acknowledged: bool,
}

impl Notification {
    /// Create a new unacknowledged notification.
    pub fn new(
        interrupt_level: InterruptLevel,
        summary: impl Into<String>,
    ) -> Self {
        let attention_category = attention_tier(interrupt_level);
        Self {
            id: Ulid::new(),
            interrupt_level,
            attention_category,
            summary: summary.into(),
            created_at: Utc::now(),
            acknowledged: false,
        }
    }
}

// ---------------------------------------------------------------------------
// NotificationStore trait
// ---------------------------------------------------------------------------

/// Storage and query interface for notifications.
pub trait NotificationStore: Send + Sync {
    /// Store a new notification.
    fn push(&self, notification: Notification);

    /// List all notifications in a given attention tier.
    fn list_by_tier(&self, category: AttentionCategory) -> Vec<Notification>;

    /// List pending (unacknowledged) P0 and P1 notifications.
    fn list_pending(&self) -> Vec<Notification>;

    /// Mark a notification as acknowledged by ID.
    ///
    /// Returns `true` if the notification was found and updated.
    fn mark_acknowledged(&self, id: Ulid) -> bool;

    /// Count notifications by interrupt level.
    fn count_by_level(&self, level: InterruptLevel) -> usize;
}

// ---------------------------------------------------------------------------
// InMemoryNotificationStore
// ---------------------------------------------------------------------------

/// Thread-safe in-memory implementation of `NotificationStore`.
#[derive(Default)]
pub struct InMemoryNotificationStore {
    notifications: Mutex<Vec<Notification>>,
}

impl InMemoryNotificationStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl NotificationStore for InMemoryNotificationStore {
    fn push(&self, notification: Notification) {
        self.notifications.lock().unwrap().push(notification);
    }

    fn list_by_tier(&self, category: AttentionCategory) -> Vec<Notification> {
        self.notifications
            .lock()
            .unwrap()
            .iter()
            .filter(|n| n.attention_category == category)
            .cloned()
            .collect()
    }

    fn list_pending(&self) -> Vec<Notification> {
        self.notifications
            .lock()
            .unwrap()
            .iter()
            .filter(|n| {
                !n.acknowledged
                    && matches!(n.interrupt_level, InterruptLevel::P0 | InterruptLevel::P1)
            })
            .cloned()
            .collect()
    }

    fn mark_acknowledged(&self, id: Ulid) -> bool {
        let mut notifications = self.notifications.lock().unwrap();
        if let Some(n) = notifications.iter_mut().find(|n| n.id == id) {
            n.acknowledged = true;
            true
        } else {
            false
        }
    }

    fn count_by_level(&self, level: InterruptLevel) -> usize {
        self.notifications
            .lock()
            .unwrap()
            .iter()
            .filter(|n| n.interrupt_level == level)
            .count()
    }
}

// ---------------------------------------------------------------------------
// NotificationRouter
// ---------------------------------------------------------------------------

/// Routes classified domain events to the appropriate notification store tier.
pub struct NotificationRouter<S: NotificationStore = InMemoryNotificationStore> {
    classifier: InterruptClassifier,
    store: S,
}

impl NotificationRouter<InMemoryNotificationStore> {
    /// Create a router backed by an in-memory store.
    pub fn new() -> Self {
        Self {
            classifier: InterruptClassifier::new(),
            store: InMemoryNotificationStore::new(),
        }
    }
}

impl Default for NotificationRouter<InMemoryNotificationStore> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: NotificationStore> NotificationRouter<S> {
    /// Create a router with a custom store (for testing or alternate backends).
    pub fn with_store(store: S) -> Self {
        Self {
            classifier: InterruptClassifier::new(),
            store,
        }
    }

    /// Classify a domain event and route it to the notification store.
    ///
    /// Returns the notification that was created.
    pub fn route(&self, event: &DomainEvent) -> Notification {
        let level = self.classifier.classify(event);
        let summary = event_summary(event);
        let notification = Notification::new(level, summary);
        self.store.push(notification.clone());
        notification
    }

    /// Expose the underlying store for queries.
    pub fn store(&self) -> &S {
        &self.store
    }
}

// ---------------------------------------------------------------------------
// Event summary helper
// ---------------------------------------------------------------------------

/// Generate a short human-readable summary for a domain event.
fn event_summary(event: &DomainEvent) -> String {
    match event {
        DomainEvent::TaskCreated { title, .. } => format!("Task created: {title}"),
        DomainEvent::TaskStageChanged {
            from_stage,
            to_stage,
            ..
        } => format!("Task moved from '{from_stage}' to '{to_stage}'"),
        DomainEvent::TaskPriorityChanged { from, to } => {
            format!("Priority changed from {from:?} to {to:?}")
        }
        DomainEvent::TaskBlocked { reason } => format!("Task blocked: {reason}"),
        DomainEvent::TaskUnblocked { resolution } => match resolution {
            Some(r) => format!("Task unblocked: {r}"),
            None => "Task unblocked".into(),
        },
        DomainEvent::TaskCompleted { outcome } => format!("Task completed: {outcome:?}"),
        DomainEvent::AgentAssigned { agent_name, .. } => {
            format!("Agent assigned: {agent_name}")
        }
        DomainEvent::AgentOutput { .. } => "Agent output received".into(),
        DomainEvent::AgentCompleted { summary, .. } => match summary {
            Some(s) => format!("Agent completed: {s}"),
            None => "Agent completed".into(),
        },
        DomainEvent::HumanDecision {
            decided_by,
            decision,
            ..
        } => {
            format!("Decision by {decided_by}: {decision:?}")
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use molt_hub_core::events::types::HumanDecisionKind;
    use molt_hub_core::model::{AgentId, Priority, TaskState};

    fn router() -> NotificationRouter {
        NotificationRouter::new()
    }

    // ── push and list_by_tier ─────────────────────────────────────────────────

    #[test]
    fn push_and_list_by_tier() {
        let store = InMemoryNotificationStore::new();
        let p0 = Notification::new(InterruptLevel::P0, "urgent");
        let p2 = Notification::new(InterruptLevel::P2, "info");

        store.push(p0);
        store.push(p2);

        let decision_queue = store.list_by_tier(AttentionCategory::DecisionQueue);
        assert_eq!(decision_queue.len(), 1);
        assert_eq!(decision_queue[0].interrupt_level, InterruptLevel::P0);

        let digest = store.list_by_tier(AttentionCategory::NotificationDigest);
        assert_eq!(digest.len(), 1);
        assert_eq!(digest[0].interrupt_level, InterruptLevel::P2);
    }

    #[test]
    fn list_by_tier_empty_returns_empty_vec() {
        let store = InMemoryNotificationStore::new();
        let result = store.list_by_tier(AttentionCategory::DecisionQueue);
        assert!(result.is_empty());
    }

    // ── list_pending ─────────────────────────────────────────────────────────

    #[test]
    fn list_pending_returns_unacked_p0_and_p1() {
        let store = InMemoryNotificationStore::new();
        store.push(Notification::new(InterruptLevel::P0, "p0"));
        store.push(Notification::new(InterruptLevel::P1, "p1"));
        store.push(Notification::new(InterruptLevel::P2, "p2"));
        store.push(Notification::new(InterruptLevel::P3, "p3"));

        let pending = store.list_pending();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|n| matches!(
            n.interrupt_level,
            InterruptLevel::P0 | InterruptLevel::P1
        )));
    }

    #[test]
    fn list_pending_excludes_acknowledged() {
        let store = InMemoryNotificationStore::new();
        let n = Notification::new(InterruptLevel::P0, "p0");
        let id = n.id;
        store.push(n);

        assert_eq!(store.list_pending().len(), 1);
        store.mark_acknowledged(id);
        assert_eq!(store.list_pending().len(), 0);
    }

    // ── mark_acknowledged ────────────────────────────────────────────────────

    #[test]
    fn mark_acknowledged_returns_true_when_found() {
        let store = InMemoryNotificationStore::new();
        let n = Notification::new(InterruptLevel::P1, "test");
        let id = n.id;
        store.push(n);

        assert!(store.mark_acknowledged(id));
    }

    #[test]
    fn mark_acknowledged_returns_false_when_not_found() {
        let store = InMemoryNotificationStore::new();
        assert!(!store.mark_acknowledged(Ulid::new()));
    }

    // ── count_by_level ───────────────────────────────────────────────────────

    #[test]
    fn count_by_level_zero_when_empty() {
        let store = InMemoryNotificationStore::new();
        assert_eq!(store.count_by_level(InterruptLevel::P0), 0);
    }

    #[test]
    fn count_by_level_counts_correctly() {
        let store = InMemoryNotificationStore::new();
        store.push(Notification::new(InterruptLevel::P0, "a"));
        store.push(Notification::new(InterruptLevel::P0, "b"));
        store.push(Notification::new(InterruptLevel::P1, "c"));

        assert_eq!(store.count_by_level(InterruptLevel::P0), 2);
        assert_eq!(store.count_by_level(InterruptLevel::P1), 1);
        assert_eq!(store.count_by_level(InterruptLevel::P2), 0);
    }

    // ── NotificationRouter.route ─────────────────────────────────────────────

    #[test]
    fn route_task_blocked_creates_p0_notification() {
        let r = router();
        let event = DomainEvent::TaskBlocked {
            reason: "awaiting dependency".into(),
        };
        let n = r.route(&event);
        assert_eq!(n.interrupt_level, InterruptLevel::P0);
        assert_eq!(n.attention_category, AttentionCategory::DecisionQueue);
    }

    #[test]
    fn route_task_created_creates_p3_notification() {
        let r = router();
        let event = DomainEvent::TaskCreated {
            title: "New task".into(),
            description: "Desc".into(),
            initial_stage: "triage".into(),
            priority: Priority::P2,
        };
        let n = r.route(&event);
        assert_eq!(n.interrupt_level, InterruptLevel::P3);
        assert_eq!(n.attention_category, AttentionCategory::PassiveDashboard);
    }

    #[test]
    fn route_awaiting_approval_transition_is_p1() {
        let r = router();
        let event = DomainEvent::TaskStageChanged {
            from_stage: "work".into(),
            to_stage: "review".into(),
            new_state: TaskState::AwaitingApproval {
                approvers: vec![],
                approved_by: vec![],
            },
        };
        let n = r.route(&event);
        assert_eq!(n.interrupt_level, InterruptLevel::P1);
    }

    #[test]
    fn route_stores_notification_in_store() {
        let r = router();
        let event = DomainEvent::AgentOutput {
            agent_id: AgentId::new(),
            output: "log line".into(),
        };
        r.route(&event);

        let passive = r.store().list_by_tier(AttentionCategory::PassiveDashboard);
        assert_eq!(passive.len(), 1);
    }

    #[test]
    fn route_agent_completed_goes_to_notification_digest() {
        let r = router();
        let event = DomainEvent::AgentCompleted {
            agent_id: AgentId::new(),
            summary: Some("done".into()),
        };
        r.route(&event);

        let digest = r.store().list_by_tier(AttentionCategory::NotificationDigest);
        assert_eq!(digest.len(), 1);
    }

    #[test]
    fn route_rejected_decision_is_p1_in_decision_queue() {
        let r = router();
        let event = DomainEvent::HumanDecision {
            decided_by: "alice".into(),
            decision: HumanDecisionKind::Rejected {
                reason: "not acceptable".into(),
            },
            note: None,
        };
        let n = r.route(&event);
        assert_eq!(n.interrupt_level, InterruptLevel::P1);
        assert_eq!(n.attention_category, AttentionCategory::DecisionQueue);
    }

    #[test]
    fn notification_summary_is_populated() {
        let r = router();
        let event = DomainEvent::TaskBlocked {
            reason: "some reason".into(),
        };
        let n = r.route(&event);
        assert!(n.summary.contains("some reason"));
    }
}
