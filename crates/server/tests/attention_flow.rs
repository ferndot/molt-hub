//! Integration tests for the attention classification and routing flow.
//!
//! These tests verify end-to-end attention workflows:
//! - Domain events from various scenarios get the correct interrupt levels
//! - Classified events flow through NotificationRouter to the store
//! - AttentionSummary counts reflect the routed events correctly
//! - P0 and P3 events arriving together are each classified independently
//! - The full flow: event → classify → route → summarize works correctly

use molt_hub_core::events::types::{DomainEvent, HumanDecisionKind};
use molt_hub_core::model::{AgentId, Priority, TaskOutcome, TaskState};

use molt_hub_server::attention::priority::{attention_tier, AttentionCategory};
use molt_hub_server::attention::router::NotificationStore;
use molt_hub_server::attention::{
    AttentionSummary, InterruptClassifier, InterruptLevel, NotificationRouter,
};

// ---------------------------------------------------------------------------
// Tests: InterruptClassifier classification
// ---------------------------------------------------------------------------

/// Agent failed (TaskBlocked) is classified as P0.
#[test]
fn classify_agent_failed_is_p0() {
    let classifier = InterruptClassifier::new();
    let event = DomainEvent::TaskBlocked {
        reason: "agent crashed unexpectedly".into(),
    };
    assert_eq!(classifier.classify(&event), InterruptLevel::P0);
}

/// Approval requested (TaskStageChanged to AwaitingApproval) is classified as P1.
#[test]
fn classify_approval_requested_is_p1() {
    let classifier = InterruptClassifier::new();
    let event = DomainEvent::TaskStageChanged {
        from_stage: "work".into(),
        to_stage: "review".into(),
        new_state: TaskState::AwaitingApproval {
            approvers: vec!["alice".into()],
            approved_by: vec![],
        },
    };
    assert_eq!(classifier.classify(&event), InterruptLevel::P1);
}

/// Task completed successfully is classified as P2.
#[test]
fn classify_task_completed_is_p2() {
    let classifier = InterruptClassifier::new();
    let event = DomainEvent::TaskCompleted {
        outcome: TaskOutcome::Success,
    };
    assert_eq!(classifier.classify(&event), InterruptLevel::P2);
}

/// Agent output (log line) is classified as P3.
#[test]
fn classify_agent_output_is_p3() {
    let classifier = InterruptClassifier::new();
    let event = DomainEvent::AgentOutput {
        agent_id: AgentId::new(),
        output: "compiling...".into(),
    };
    assert_eq!(classifier.classify(&event), InterruptLevel::P3);
}

/// Rejection decision is classified as P1 (operator must re-assign or abandon).
#[test]
fn classify_human_rejection_is_p1() {
    let classifier = InterruptClassifier::new();
    let event = DomainEvent::HumanDecision {
        decided_by: "alice".into(),
        decision: HumanDecisionKind::Rejected {
            reason: "does not meet standards".into(),
        },
        note: None,
    };
    assert_eq!(classifier.classify(&event), InterruptLevel::P1);
}

// ---------------------------------------------------------------------------
// Tests: NotificationRouter full flow
// ---------------------------------------------------------------------------

/// Route a P0 event → verify it lands in DecisionQueue and shows as pending.
#[test]
fn route_p0_lands_in_decision_queue_as_pending() {
    let router = NotificationRouter::new();
    let event = DomainEvent::TaskBlocked {
        reason: "disk full".into(),
    };
    let notification = router.route(&event);

    assert_eq!(notification.interrupt_level, InterruptLevel::P0);
    assert_eq!(
        notification.attention_category,
        AttentionCategory::DecisionQueue
    );
    assert!(!notification.acknowledged);

    let pending = router.store().list_pending();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].interrupt_level, InterruptLevel::P0);
}

/// Route a P3 event → verify it lands in PassiveDashboard, not in pending.
#[test]
fn route_p3_lands_in_passive_dashboard_not_pending() {
    let router = NotificationRouter::new();
    let event = DomainEvent::TaskCreated {
        title: "New feature".into(),
        description: "Implement X".into(),
        initial_stage: "triage".into(),
        priority: Priority::P3,
        board_id: None,
    };
    let notification = router.route(&event);

    assert_eq!(notification.interrupt_level, InterruptLevel::P3);
    assert_eq!(
        notification.attention_category,
        AttentionCategory::PassiveDashboard
    );

    // P3 events do not show in pending.
    let pending = router.store().list_pending();
    assert!(pending.is_empty());
}

/// Route a mixed batch: one P0 and one P3 — both are classified correctly.
#[test]
fn route_mixed_p0_and_p3_both_classified_correctly() {
    let router = NotificationRouter::new();

    let p0_event = DomainEvent::TaskBlocked {
        reason: "network timeout".into(),
    };
    let p3_event = DomainEvent::AgentOutput {
        agent_id: AgentId::new(),
        output: "log output".into(),
    };

    let n_p0 = router.route(&p0_event);
    let n_p3 = router.route(&p3_event);

    assert_eq!(n_p0.interrupt_level, InterruptLevel::P0);
    assert_eq!(n_p3.interrupt_level, InterruptLevel::P3);

    // P0 in decision queue.
    let decision_queue = router
        .store()
        .list_by_tier(AttentionCategory::DecisionQueue);
    assert_eq!(decision_queue.len(), 1);
    assert_eq!(decision_queue[0].interrupt_level, InterruptLevel::P0);

    // P3 in passive dashboard.
    let passive = router
        .store()
        .list_by_tier(AttentionCategory::PassiveDashboard);
    assert_eq!(passive.len(), 1);
    assert_eq!(passive[0].interrupt_level, InterruptLevel::P3);
}

// ---------------------------------------------------------------------------
// Tests: AttentionSummary from routed events
// ---------------------------------------------------------------------------

/// Full pipeline: route several events, then compute the AttentionSummary.
/// Verify counts and pending_decisions match what was routed.
#[test]
fn attention_summary_reflects_routed_events() {
    let router = NotificationRouter::new();

    // Route: 1 P0, 1 P1, 2 P2, 1 P3
    router.route(&DomainEvent::TaskBlocked {
        reason: "agent crashed".into(),
    });
    router.route(&DomainEvent::TaskStageChanged {
        from_stage: "work".into(),
        to_stage: "review".into(),
        new_state: TaskState::AwaitingApproval {
            approvers: vec![],
            approved_by: vec![],
        },
    });
    router.route(&DomainEvent::AgentCompleted {
        agent_id: AgentId::new(),
        summary: Some("done".into()),
    });
    router.route(&DomainEvent::TaskCompleted {
        outcome: TaskOutcome::Success,
    });
    router.route(&DomainEvent::TaskPriorityChanged {
        from: Priority::P3,
        to: Priority::P1,
    });

    let summary = AttentionSummary::from_store(router.store());
    assert_eq!(summary.p0_count, 1);
    assert_eq!(summary.p1_count, 1);
    assert_eq!(summary.p2_count, 2);
    assert_eq!(summary.p3_count, 1);
    assert_eq!(summary.total(), 5);

    // P0 + P1 = 2 unacked pending decisions.
    assert_eq!(summary.pending_decisions, 2);
    assert!(summary.needs_attention());
}

/// Acknowledging a P0 reduces pending_decisions count.
#[test]
fn acknowledging_p0_reduces_pending_decisions() {
    let router = NotificationRouter::new();

    let n = router.route(&DomainEvent::TaskBlocked {
        reason: "something broke".into(),
    });
    let id = n.id;

    let summary_before = AttentionSummary::from_store(router.store());
    assert_eq!(summary_before.pending_decisions, 1);

    router.store().mark_acknowledged(id);

    let summary_after = AttentionSummary::from_store(router.store());
    assert_eq!(summary_after.pending_decisions, 0);
    // p0_count still counts the notification even though it is acknowledged.
    assert_eq!(summary_after.p0_count, 1);
}

/// When only P2 and P3 notifications exist, needs_attention() returns false.
#[test]
fn needs_attention_false_with_only_p2_p3() {
    let router = NotificationRouter::new();

    router.route(&DomainEvent::AgentCompleted {
        agent_id: AgentId::new(),
        summary: None,
    });
    router.route(&DomainEvent::AgentOutput {
        agent_id: AgentId::new(),
        output: "log".into(),
    });

    let summary = AttentionSummary::from_store(router.store());
    assert!(!summary.needs_attention());
}

/// Empty store gives a zeroed summary.
#[test]
fn empty_router_gives_zero_summary() {
    let router = NotificationRouter::new();
    let summary = AttentionSummary::from_store(router.store());
    assert_eq!(summary, AttentionSummary::default());
    assert_eq!(summary.total(), 0);
    assert!(!summary.needs_attention());
}

// ---------------------------------------------------------------------------
// Tests: tier mapping integration
// ---------------------------------------------------------------------------

/// attention_tier mapping: P0/P1 → DecisionQueue, P2 → NotificationDigest, P3 → PassiveDashboard.
#[test]
fn tier_mapping_all_levels() {
    assert_eq!(
        attention_tier(InterruptLevel::P0),
        AttentionCategory::DecisionQueue
    );
    assert_eq!(
        attention_tier(InterruptLevel::P1),
        AttentionCategory::DecisionQueue
    );
    assert_eq!(
        attention_tier(InterruptLevel::P2),
        AttentionCategory::NotificationDigest
    );
    assert_eq!(
        attention_tier(InterruptLevel::P3),
        AttentionCategory::PassiveDashboard
    );
}

// ---------------------------------------------------------------------------
// Tests: notification summary text is meaningful
// ---------------------------------------------------------------------------

/// When a TaskBlocked event is routed, the notification summary contains the reason.
#[test]
fn routed_notification_summary_contains_event_detail() {
    let router = NotificationRouter::new();
    let reason = "critical dependency missing";
    let event = DomainEvent::TaskBlocked {
        reason: reason.into(),
    };
    let notification = router.route(&event);
    assert!(
        notification.summary.contains(reason),
        "notification summary '{}' should contain the block reason",
        notification.summary
    );
}
