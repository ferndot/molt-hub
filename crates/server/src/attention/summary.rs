//! Attention summary — aggregated counts for sidebar badges and indicators.

use serde::{Deserialize, Serialize};

use crate::attention::priority::{AttentionCategory, InterruptLevel};
use crate::attention::router::NotificationStore;

// ---------------------------------------------------------------------------
// AttentionSummary
// ---------------------------------------------------------------------------

/// Aggregated count of notifications by level, used to drive sidebar badges
/// and the top-level attention indicator in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AttentionSummary {
    /// Number of P0 (immediate action required) notifications.
    pub p0_count: usize,
    /// Number of P1 (needs attention soon) notifications.
    pub p1_count: usize,
    /// Number of P2 (informational) notifications.
    pub p2_count: usize,
    /// Number of P3 (passive/log-only) notifications.
    pub p3_count: usize,
    /// Number of unacknowledged notifications in the decision queue (P0 + P1).
    pub pending_decisions: usize,
}

impl AttentionSummary {
    /// Compute a summary from any `NotificationStore`.
    pub fn from_store(store: &dyn NotificationStore) -> Self {
        let p0_count = store.count_by_level(InterruptLevel::P0);
        let p1_count = store.count_by_level(InterruptLevel::P1);
        let p2_count = store.count_by_level(InterruptLevel::P2);
        let p3_count = store.count_by_level(InterruptLevel::P3);
        let pending_decisions = store
            .list_by_tier(AttentionCategory::DecisionQueue)
            .iter()
            .filter(|n| !n.acknowledged)
            .count();

        Self {
            p0_count,
            p1_count,
            p2_count,
            p3_count,
            pending_decisions,
        }
    }

    /// Returns `true` if any P0 or P1 notifications exist.
    pub fn needs_attention(&self) -> bool {
        self.p0_count > 0 || self.p1_count > 0
    }

    /// Total number of all notifications.
    pub fn total(&self) -> usize {
        self.p0_count + self.p1_count + self.p2_count + self.p3_count
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::router::{InMemoryNotificationStore, Notification};

    fn make_store_with(levels: &[InterruptLevel]) -> InMemoryNotificationStore {
        let store = InMemoryNotificationStore::new();
        for &level in levels {
            store.push(Notification::new(level, "test"));
        }
        store
    }

    #[test]
    fn empty_store_gives_zero_summary() {
        let store = InMemoryNotificationStore::new();
        let summary = AttentionSummary::from_store(&store);
        assert_eq!(summary, AttentionSummary::default());
        assert_eq!(summary.total(), 0);
        assert!(!summary.needs_attention());
    }

    #[test]
    fn counts_all_levels_correctly() {
        let store = make_store_with(&[
            InterruptLevel::P0,
            InterruptLevel::P0,
            InterruptLevel::P1,
            InterruptLevel::P2,
            InterruptLevel::P2,
            InterruptLevel::P2,
            InterruptLevel::P3,
        ]);

        let summary = AttentionSummary::from_store(&store);
        assert_eq!(summary.p0_count, 2);
        assert_eq!(summary.p1_count, 1);
        assert_eq!(summary.p2_count, 3);
        assert_eq!(summary.p3_count, 1);
        assert_eq!(summary.total(), 7);
    }

    #[test]
    fn pending_decisions_counts_unacked_p0_and_p1() {
        let store = InMemoryNotificationStore::new();
        let n_p0 = Notification::new(InterruptLevel::P0, "p0");
        let n_p1 = Notification::new(InterruptLevel::P1, "p1");
        let n_p1_acked_id = {
            let n = Notification::new(InterruptLevel::P1, "p1-acked");
            let id = n.id;
            store.push(n);
            id
        };
        store.push(n_p0);
        store.push(n_p1);

        // Acknowledge one P1
        store.mark_acknowledged(n_p1_acked_id);

        let summary = AttentionSummary::from_store(&store);
        // Total P1 = 2, total P0 = 1, but one P1 is acknowledged
        // pending_decisions = P0 unacked (1) + P1 unacked (1) = 2
        assert_eq!(summary.pending_decisions, 2);
    }

    #[test]
    fn needs_attention_true_when_p0_exists() {
        let store = make_store_with(&[InterruptLevel::P0]);
        let summary = AttentionSummary::from_store(&store);
        assert!(summary.needs_attention());
    }

    #[test]
    fn needs_attention_true_when_p1_exists() {
        let store = make_store_with(&[InterruptLevel::P1]);
        let summary = AttentionSummary::from_store(&store);
        assert!(summary.needs_attention());
    }

    #[test]
    fn needs_attention_false_when_only_p2_p3() {
        let store = make_store_with(&[InterruptLevel::P2, InterruptLevel::P3]);
        let summary = AttentionSummary::from_store(&store);
        assert!(!summary.needs_attention());
    }

    #[test]
    fn summary_serde_round_trip() {
        let summary = AttentionSummary {
            p0_count: 1,
            p1_count: 2,
            p2_count: 3,
            p3_count: 4,
            pending_decisions: 3,
        };
        let json = serde_json::to_string(&summary).unwrap();
        let restored: AttentionSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(summary, restored);
    }
}
