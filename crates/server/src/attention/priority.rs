//! Interrupt priority levels and attention tier classification.

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// InterruptLevel
// ---------------------------------------------------------------------------

/// Four-level interrupt classification for agent-raised events.
///
/// Rendered as two levels in v0 UI (needs attention / doesn't), but the full
/// four levels are stored from day 1 to support richer future UIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptLevel {
    /// Immediate action required. Agent failed/crashed, task blocked,
    /// approval timeout about to expire.
    P0,
    /// Needs attention soon. Approval requested, agent completed with errors.
    P1,
    /// Informational. Agent completed successfully, task stage changed, agent
    /// assigned.
    P2,
    /// Passive / log-only. Agent output streaming, priority changed, task
    /// created.
    P3,
}

impl fmt::Display for InterruptLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InterruptLevel::P0 => write!(f, "P0"),
            InterruptLevel::P1 => write!(f, "P1"),
            InterruptLevel::P2 => write!(f, "P2"),
            InterruptLevel::P3 => write!(f, "P3"),
        }
    }
}

// ---------------------------------------------------------------------------
// AttentionCategory
// ---------------------------------------------------------------------------

/// Three-tier attention model mapping interrupt levels to display channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionCategory {
    /// Decision required from the human (P0 and P1).
    DecisionQueue,
    /// Informational digest — review at your convenience (P2).
    NotificationDigest,
    /// Background log — visible on the passive dashboard only (P3).
    PassiveDashboard,
}

impl fmt::Display for AttentionCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AttentionCategory::DecisionQueue => write!(f, "DecisionQueue"),
            AttentionCategory::NotificationDigest => write!(f, "NotificationDigest"),
            AttentionCategory::PassiveDashboard => write!(f, "PassiveDashboard"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tier mapping
// ---------------------------------------------------------------------------

/// Map an `InterruptLevel` to its corresponding `AttentionCategory`.
///
/// - P0, P1 → `DecisionQueue`
/// - P2     → `NotificationDigest`
/// - P3     → `PassiveDashboard`
pub fn attention_tier(level: InterruptLevel) -> AttentionCategory {
    match level {
        InterruptLevel::P0 | InterruptLevel::P1 => AttentionCategory::DecisionQueue,
        InterruptLevel::P2 => AttentionCategory::NotificationDigest,
        InterruptLevel::P3 => AttentionCategory::PassiveDashboard,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_mapping_p0_to_decision_queue() {
        assert_eq!(attention_tier(InterruptLevel::P0), AttentionCategory::DecisionQueue);
    }

    #[test]
    fn tier_mapping_p1_to_decision_queue() {
        assert_eq!(attention_tier(InterruptLevel::P1), AttentionCategory::DecisionQueue);
    }

    #[test]
    fn tier_mapping_p2_to_notification_digest() {
        assert_eq!(attention_tier(InterruptLevel::P2), AttentionCategory::NotificationDigest);
    }

    #[test]
    fn tier_mapping_p3_to_passive_dashboard() {
        assert_eq!(attention_tier(InterruptLevel::P3), AttentionCategory::PassiveDashboard);
    }

    #[test]
    fn interrupt_level_display() {
        assert_eq!(InterruptLevel::P0.to_string(), "P0");
        assert_eq!(InterruptLevel::P1.to_string(), "P1");
        assert_eq!(InterruptLevel::P2.to_string(), "P2");
        assert_eq!(InterruptLevel::P3.to_string(), "P3");
    }

    #[test]
    fn interrupt_level_ordering() {
        // P0 < P1 < P2 < P3 (lower numeric = higher urgency)
        assert!(InterruptLevel::P0 < InterruptLevel::P1);
        assert!(InterruptLevel::P1 < InterruptLevel::P2);
        assert!(InterruptLevel::P2 < InterruptLevel::P3);
    }

    #[test]
    fn attention_category_display() {
        assert_eq!(AttentionCategory::DecisionQueue.to_string(), "DecisionQueue");
        assert_eq!(AttentionCategory::NotificationDigest.to_string(), "NotificationDigest");
        assert_eq!(AttentionCategory::PassiveDashboard.to_string(), "PassiveDashboard");
    }

    #[test]
    fn interrupt_level_serde_round_trip() {
        for level in [
            InterruptLevel::P0,
            InterruptLevel::P1,
            InterruptLevel::P2,
            InterruptLevel::P3,
        ] {
            let serialized = serde_json::to_string(&level).unwrap();
            let deserialized: InterruptLevel = serde_json::from_str(&serialized).unwrap();
            assert_eq!(level, deserialized);
        }
    }
}
