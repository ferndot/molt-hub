//! Interrupt classification — routing agent-raised attention signals to the appropriate handler.
//!
//! # Architecture
//!
//! Four-level interrupt classification system for agent events:
//!
//! - **P0** — Immediate action required (task blocked, agent crashed)
//! - **P1** — Needs attention soon (approval requested, rejection)
//! - **P2** — Informational (agent completed, stage changed)
//! - **P3** — Passive / log-only (agent output, priority changed, task created)
//!
//! Events are routed to one of three attention tiers:
//! - `DecisionQueue` (P0, P1) — requires human decision
//! - `NotificationDigest` (P2) — review at your convenience
//! - `PassiveDashboard` (P3) — background log
//!
//! See `AttentionSummary` for the aggregate view that drives sidebar badges.

pub mod classifier;
pub mod priority;
pub mod router;
pub mod summary;

// Convenience re-exports
pub use classifier::InterruptClassifier;
pub use priority::{attention_tier, AttentionCategory, InterruptLevel};
pub use router::{InMemoryNotificationStore, Notification, NotificationRouter, NotificationStore};
pub use summary::AttentionSummary;
